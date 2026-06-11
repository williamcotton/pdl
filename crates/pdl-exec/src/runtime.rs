// `runtime.rs` is the top-level module for `pdl-exec`'s execution layer. It
// keeps the `Runtime` struct and the pipeline-stage dispatch methods on its
// impl block. Per the v0.42 split, free helpers live in sibling modules:
//
// * [`runtime::native_lowering`] — IR-to-`pdl-data` expression/aggregate/window
//   translation.
// * [`runtime::native_planning`] — native eligibility checks and the native
//   pipeline orchestration entry points (`try_execute_native`, etc.).
// * [`runtime::row_eval`] — row-runtime cell, scalar, and window evaluation.
// * [`runtime::stages`] — stage-specific row transformations
//   (`pivot_longer`, `complete`, joins, unions) and schema-compatibility
//   checks.

use pdl_core::{codes, Diagnostic, Span};
use pdl_data::{
    read_table_from_bytes, sniff_format_from_bytes, DataBackend, DataFormat,
    NullsOrder as DataNullsOrder, Row, SortDirection as DataSortDirection, SortSpec, Table, Value,
};
use pdl_driver::{
    DriverIo, FormatDecision, OsDriverIo, PlanInputSource, PlanOutputSink, PreparedProgram,
    SinkDescriptor, SourceDescriptor,
};
use pdl_semantics::{
    decode_context_column_ref_ir, AggItemIr, CompleteFillItemIr, ContextKindIr, ExprIr, JoinKindIr,
    MutateItemIr, NullsOrderIr, PipelineIr, PipelineStartIr, SortDirectionIr, StageIr,
};
use std::collections::BTreeMap;

use crate::output::{emit_stdout, write_output};
use crate::planning::{plan_prepared, PlannedEngine, PlanningOptions};

mod native_lowering;
mod native_planning;
mod row_eval;
mod stages;

#[cfg(test)]
use native_planning::check_native_program_eligibility;
use native_planning::try_execute_native;
use row_eval::{eval_aggregate, eval_row_expr, ExprRole};
use stages::{
    combine_rows, complete, ensure_key_types_compatible, ensure_union_compatible, join_columns,
    join_index, join_semi_anti, pivot_longer, right_non_key_indices, right_only_row, row_join_key,
};

#[derive(Clone, Debug)]
pub struct RunOptions {
    pub stdout_format: Option<String>,
    pub dry_run: bool,
    pub allow_binary_stdout: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ExecutionEngine {
    #[default]
    Auto,
    Row,
    /// Row execution plus a strict guarantee: no pipeline in the run may
    /// silently use native lowering. Hosts verify the returned backend is
    /// `PortableRows` and treat anything else as an error.
    RowStrict,
    Native,
    NativeStrict,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            stdout_format: None,
            dry_run: false,
            allow_binary_stdout: true,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct RunResult {
    pub stdout: Option<Vec<u8>>,
    pub named_outputs: Vec<NamedOutput>,
    pub diagnostics: Vec<Diagnostic>,
    pub backend: DataBackend,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NamedOutput {
    pub name: String,
    pub table: Table,
}

pub fn run_prepared(prepared: &PreparedProgram, options: RunOptions) -> RunResult {
    let io = OsDriverIo;
    run_prepared_with_io(prepared, options, &io)
}

pub fn run_prepared_with_io(
    prepared: &PreparedProgram,
    options: RunOptions,
    io: &dyn DriverIo,
) -> RunResult {
    run_prepared_with_io_and_context(prepared, options, io, BTreeMap::new())
}

pub fn run_prepared_with_engine(
    prepared: &PreparedProgram,
    options: RunOptions,
    engine: ExecutionEngine,
) -> RunResult {
    let io = OsDriverIo;
    run_prepared_with_io_and_context_and_engine(prepared, options, &io, BTreeMap::new(), engine)
}

pub fn run_prepared_with_io_and_context(
    prepared: &PreparedProgram,
    options: RunOptions,
    io: &dyn DriverIo,
    context: BTreeMap<String, Value>,
) -> RunResult {
    run_prepared_with_io_and_context_and_engine(
        prepared,
        options,
        io,
        context,
        ExecutionEngine::Auto,
    )
}

pub fn run_prepared_with_io_and_context_and_engine(
    prepared: &PreparedProgram,
    options: RunOptions,
    io: &dyn DriverIo,
    context: BTreeMap<String, Value>,
    engine: ExecutionEngine,
) -> RunResult {
    let plan = match plan_prepared(
        prepared,
        PlanningOptions {
            stdout_format: options.stdout_format.clone(),
            dry_run: options.dry_run,
            allow_binary_stdout: options.allow_binary_stdout,
            engine: match engine {
                ExecutionEngine::Auto => PlannedEngine::Auto,
                ExecutionEngine::Row => PlannedEngine::Row,
                ExecutionEngine::RowStrict => PlannedEngine::RowStrict,
                ExecutionEngine::Native => PlannedEngine::Native,
                ExecutionEngine::NativeStrict => PlannedEngine::NativeStrict,
            },
        },
    ) {
        Ok(plan) => plan,
        Err(diagnostics) => {
            return RunResult {
                stdout: None,
                named_outputs: Vec::new(),
                diagnostics,
                backend: DataBackend::PortableRows,
            };
        }
    };

    let Some(ir) = prepared.analysis.ir.as_ref() else {
        let mut diagnostics = prepared.diagnostics();
        diagnostics.push(Diagnostic::error(
            codes::E1505,
            "semantic IR is unavailable for execution",
            Span::zero(),
        ));
        return RunResult {
            stdout: None,
            named_outputs: Vec::new(),
            diagnostics,
            backend: DataBackend::PortableRows,
        };
    };
    let mut diagnostics = prepared.diagnostics();
    let context = build_context_values(ir, context, &mut diagnostics);
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == pdl_core::Severity::Error)
    {
        return RunResult {
            stdout: None,
            named_outputs: Vec::new(),
            diagnostics,
            backend: DataBackend::PortableRows,
        };
    }

    if matches!(engine, ExecutionEngine::Auto)
        && plan.observability.selected_engine == PlannedEngine::Mixed
    {
        return execute_mixed_outputs(prepared, ir, &plan, &context, diagnostics, io);
    }

    let should_try_native = matches!(
        engine,
        ExecutionEngine::Native | ExecutionEngine::NativeStrict
    ) || (matches!(engine, ExecutionEngine::Auto)
        && plan.observability.selected_engine == PlannedEngine::Native);
    if should_try_native {
        match try_execute_native(prepared, ir, &plan, &context, io) {
            Ok(result) => return result,
            Err(diagnostic)
                if matches!(
                    engine,
                    ExecutionEngine::Native | ExecutionEngine::NativeStrict
                ) =>
            {
                return RunResult {
                    stdout: None,
                    named_outputs: Vec::new(),
                    diagnostics: {
                        let mut diagnostics = prepared.diagnostics();
                        diagnostics.push(diagnostic);
                        diagnostics
                    },
                    backend: DataBackend::NativePolars,
                };
            }
            Err(_) => {}
        }
    }

    let mut runtime = Runtime {
        prepared,
        diagnostics,
        cache: BTreeMap::new(),
        active_bindings: Vec::new(),
        context,
        dry_run: plan.dry_run,
        stdout: None,
        io,
    };

    let mut named_outputs = Vec::new();
    let final_table = if ir.outputs.is_empty() {
        let Some(main) = &ir.main else {
            runtime.diagnostics.push(Diagnostic::error(
                codes::E1502,
                "no runnable main pipeline",
                Span::zero(),
            ));
            return RunResult {
                stdout: None,
                named_outputs,
                diagnostics: runtime.diagnostics,
                backend: DataBackend::PortableRows,
            };
        };
        match runtime.execute_pipeline(main) {
            Ok(table) => Some(table),
            Err(diagnostic) => {
                runtime.diagnostics.push(diagnostic);
                return RunResult {
                    stdout: None,
                    named_outputs,
                    diagnostics: runtime.diagnostics,
                    backend: DataBackend::PortableRows,
                };
            }
        }
    } else {
        let mut last = None;
        for output in &ir.outputs {
            match runtime.execute_pipeline(&output.pipeline) {
                Ok(table) => {
                    last = Some(table.clone());
                    named_outputs.push(NamedOutput {
                        name: output.name.clone(),
                        table,
                    });
                }
                Err(diagnostic) => {
                    runtime.diagnostics.push(diagnostic);
                    return RunResult {
                        stdout: None,
                        named_outputs,
                        diagnostics: runtime.diagnostics,
                        backend: DataBackend::PortableRows,
                    };
                }
            }
        }
        last
    };

    let stdout = if let Some(format) = plan.stdout_format {
        final_table
            .as_ref()
            .and_then(|table| match emit_stdout(format, table) {
                Ok(bytes) => Some(bytes),
                Err(diagnostic) => {
                    runtime.diagnostics.push(diagnostic);
                    None
                }
            })
    } else {
        runtime.stdout.take()
    };

    RunResult {
        stdout,
        named_outputs,
        diagnostics: runtime.diagnostics,
        backend: DataBackend::PortableRows,
    }
}

fn build_context_values(
    ir: &pdl_semantics::ProgramIr,
    mut overrides: BTreeMap<String, Value>,
    diagnostics: &mut Vec<Diagnostic>,
) -> BTreeMap<String, Value> {
    let mut values = BTreeMap::new();
    for context in &ir.contexts {
        let default = literal_ir_value(&context.default).unwrap_or(Value::Null);
        let value = match overrides.remove(&context.name) {
            Some(value) => {
                if context_value_type_matches(&default, &value) {
                    value
                } else {
                    diagnostics.push(Diagnostic::error(
                        codes::E2005,
                        format!(
                            "external value for {} `{}` has the wrong type",
                            context_kind_label(context.kind),
                            context.name
                        ),
                        context.span,
                    ));
                    default.clone()
                }
            }
            None => default.clone(),
        };
        values.insert(context.name.clone(), value);
    }
    for name in overrides.into_keys() {
        diagnostics.push(Diagnostic::error(
            codes::E2002,
            format!("unknown context value `{name}`"),
            Span::zero(),
        ));
    }
    values
}

fn literal_ir_value(expr: &ExprIr) -> Option<Value> {
    match expr {
        ExprIr::Quoted { value, .. } => Some(Value::String(value.clone())),
        ExprIr::Number { value, .. } => Some(Value::Number(*value)),
        ExprIr::Bool { value, .. } => Some(Value::Bool(*value)),
        ExprIr::Null { .. } => Some(Value::Null),
        ExprIr::Ident { .. }
        | ExprIr::Context { .. }
        | ExprIr::Call { .. }
        | ExprIr::Window { .. }
        | ExprIr::Unary { .. }
        | ExprIr::Binary { .. } => None,
    }
}

fn context_value_type_matches(default: &Value, value: &Value) -> bool {
    matches!(
        (default, value),
        (Value::Null, Value::Null)
            | (Value::Bool(_), Value::Bool(_))
            | (Value::Number(_), Value::Number(_))
            | (Value::String(_), Value::String(_))
    )
}

fn context_kind_label(kind: ContextKindIr) -> &'static str {
    match kind {
        ContextKindIr::Param => "parameter",
        ContextKindIr::State => "state",
    }
}

fn execute_mixed_outputs(
    prepared: &PreparedProgram,
    ir: &pdl_semantics::ProgramIr,
    plan: &crate::planning::ExecutionPlan,
    context: &BTreeMap<String, Value>,
    diagnostics: Vec<Diagnostic>,
    io: &dyn DriverIo,
) -> RunResult {
    let mut runtime = Runtime {
        prepared,
        diagnostics,
        cache: BTreeMap::new(),
        active_bindings: Vec::new(),
        context: context.clone(),
        dry_run: plan.dry_run,
        stdout: None,
        io,
    };
    let mut native_stdout = None;
    let mut native_active_bindings = Vec::new();
    let mut native_binding_cache = BTreeMap::new();
    let mut named_outputs = Vec::new();

    for output in &ir.outputs {
        let output_engine = plan
            .observability
            .outputs
            .iter()
            .find(|observability| observability.name == output.name)
            .map(|observability| observability.selected_engine)
            .unwrap_or(PlannedEngine::Row);

        if output_engine == PlannedEngine::Native {
            let native_result = {
                let mut native_context = native_planning::NativeExecutionContext {
                    prepared,
                    ir,
                    execution_plan: plan,
                    context,
                    io,
                    stdout: &mut native_stdout,
                    active_bindings: &mut native_active_bindings,
                    binding_cache: &mut native_binding_cache,
                };
                native_planning::execute_native_pipeline(&mut native_context, &output.pipeline)
                    .and_then(|data_plan| data_plan.collect())
            };
            match native_result {
                Ok(table) => named_outputs.push(NamedOutput {
                    name: output.name.clone(),
                    table,
                }),
                Err(diagnostic) => {
                    runtime.diagnostics.push(diagnostic);
                    return RunResult {
                        stdout: native_stdout.or_else(|| runtime.stdout.take()),
                        named_outputs,
                        diagnostics: runtime.diagnostics,
                        backend: DataBackend::NativePolars,
                    };
                }
            }
            if native_stdout.is_some() {
                runtime.stdout = native_stdout.clone();
            }
            continue;
        }

        match runtime.execute_pipeline(&output.pipeline) {
            Ok(table) => {
                named_outputs.push(NamedOutput {
                    name: output.name.clone(),
                    table,
                });
                if runtime.stdout.is_some() {
                    native_stdout = runtime.stdout.clone();
                }
            }
            Err(diagnostic) => {
                runtime.diagnostics.push(diagnostic);
                return RunResult {
                    stdout: runtime.stdout.take().or(native_stdout),
                    named_outputs,
                    diagnostics: runtime.diagnostics,
                    backend: DataBackend::NativePolars,
                };
            }
        }
    }

    RunResult {
        stdout: runtime.stdout.take().or(native_stdout),
        named_outputs,
        diagnostics: runtime.diagnostics,
        backend: DataBackend::NativePolars,
    }
}

struct Runtime<'a> {
    prepared: &'a PreparedProgram,
    diagnostics: Vec<Diagnostic>,
    cache: BTreeMap<String, Table>,
    active_bindings: Vec<String>,
    context: BTreeMap<String, Value>,
    dry_run: bool,
    stdout: Option<Vec<u8>>,
    io: &'a dyn DriverIo,
}

impl Runtime<'_> {
    fn execute_pipeline(&mut self, pipeline: &PipelineIr) -> Result<Table, Diagnostic> {
        let mut table = match &pipeline.start {
            PipelineStartIr::Load { format, span, .. } => {
                self.execute_load(*span, format.as_deref())?
            }
            PipelineStartIr::Binding { name, span } => self.execute_binding(name, *span)?,
        };
        let mut grouping: Option<Vec<String>> = None;

        for stage in &pipeline.stages {
            match stage {
                StageIr::Filter { expr, .. } => {
                    table = self.filter(table, expr)?;
                    grouping = None;
                }
                StageIr::Select { items, .. } => {
                    let selection: Vec<(String, String)> = items
                        .iter()
                        .map(|item| {
                            Ok((
                                self.resolve_column_name(&item.source, item.span)?,
                                self.resolve_column_name(&item.output, item.span)?,
                            ))
                        })
                        .collect::<Result<_, Diagnostic>>()?;
                    table = table.select(&selection);
                    grouping = None;
                }
                StageIr::Drop { columns, span } => {
                    let columns = self.resolve_column_names(columns, *span)?;
                    table = table.drop_columns(&columns);
                    grouping = None;
                }
                StageIr::Rename { items, .. } => {
                    let renames: Vec<(String, String)> = items
                        .iter()
                        .map(|item| {
                            Ok((
                                self.resolve_column_name(&item.old, item.span)?,
                                self.resolve_column_name(&item.new, item.span)?,
                            ))
                        })
                        .collect::<Result<_, Diagnostic>>()?;
                    table = table.rename_columns(&renames);
                    grouping = None;
                }
                StageIr::Mutate { items, .. } => {
                    table = self.mutate(table, items)?;
                    grouping = None;
                }
                StageIr::GroupBy { columns, span } => {
                    grouping = Some(self.resolve_column_names(columns, *span)?);
                }
                StageIr::Agg { items, .. } => {
                    table = self.aggregate(&table, grouping.take().unwrap_or_default(), items)?;
                }
                StageIr::Sort { items, .. } => {
                    let specs = items
                        .iter()
                        .map(|item| {
                            let direction = match item.direction {
                                SortDirectionIr::Asc => DataSortDirection::Asc,
                                SortDirectionIr::Desc => DataSortDirection::Desc,
                            };
                            let nulls = item
                                .nulls
                                .map(|nulls| match nulls {
                                    NullsOrderIr::First => DataNullsOrder::First,
                                    NullsOrderIr::Last => DataNullsOrder::Last,
                                })
                                .unwrap_or(match direction {
                                    DataSortDirection::Asc => DataNullsOrder::Last,
                                    DataSortDirection::Desc => DataNullsOrder::First,
                                });
                            Ok(SortSpec {
                                column: self.resolve_column_name(&item.column, item.span)?,
                                direction,
                                nulls,
                            })
                        })
                        .collect::<Result<Vec<_>, Diagnostic>>()?;
                    table.stable_sort(&specs);
                }
                StageIr::Limit { n, .. } => {
                    table = table.limit(*n);
                }
                StageIr::Join {
                    source,
                    source_span,
                    keys,
                    kind,
                    span,
                    ..
                } => {
                    let right = self.execute_binding(source, *source_span)?;
                    let keys = keys
                        .iter()
                        .map(|key| {
                            Ok((
                                self.resolve_column_name(&key.left, *span)?,
                                self.resolve_column_name(&key.right, *span)?,
                            ))
                        })
                        .collect::<Result<Vec<_>, Diagnostic>>()?;
                    table = self.join(table, right, &keys, *kind, *span)?;
                    grouping = None;
                }
                StageIr::Union {
                    source,
                    source_span,
                    by_name,
                    distinct,
                    span,
                } => {
                    let right = self.execute_binding(source, *source_span)?;
                    table = self.union(table, right, *by_name, *distinct, *span)?;
                    grouping = None;
                }
                StageIr::Distinct { columns, span } => {
                    let columns = self.resolve_column_names(columns, *span)?;
                    table = table.distinct(&columns);
                    grouping = None;
                }
                StageIr::PivotLonger {
                    columns,
                    names_to,
                    values_to,
                    span,
                } => {
                    let columns = self.resolve_column_names(columns, *span)?;
                    let names_to = self.resolve_column_name(names_to, *span)?;
                    let values_to = self.resolve_column_name(values_to, *span)?;
                    table = pivot_longer(table, &columns, &names_to, &values_to, *span)?;
                    grouping = None;
                }
                StageIr::Complete { keys, fills, span } => {
                    let keys = self.resolve_column_names(keys, *span)?;
                    let fills = fills
                        .iter()
                        .map(|fill| {
                            Ok(CompleteFillItemIr {
                                column: self.resolve_column_name(&fill.column, fill.span)?,
                                expr: fill.expr.clone(),
                                span: fill.span,
                            })
                        })
                        .collect::<Result<Vec<_>, Diagnostic>>()?;
                    table = complete(table, &keys, &fills, *span, &self.context)?;
                    grouping = None;
                }
                StageIr::Save { format, span, .. } => {
                    self.execute_save(*span, format.as_deref(), &table)?;
                }
                StageIr::Unsupported { name, span } => {
                    return Err(Diagnostic::error(
                        codes::E1211,
                        format!("stage `{name}` is deferred in 0.26.0"),
                        *span,
                    ));
                }
            }
        }

        Ok(table)
    }

    fn resolve_column_names(
        &self,
        columns: &[String],
        span: Span,
    ) -> Result<Vec<String>, Diagnostic> {
        columns
            .iter()
            .map(|column| self.resolve_column_name(column, span))
            .collect()
    }

    fn resolve_column_name(&self, column: &str, span: Span) -> Result<String, Diagnostic> {
        let Some((kind, name)) = decode_context_column_ref_ir(column) else {
            return Ok(column.to_string());
        };
        let Some(value) = self.context.get(name) else {
            return Err(Diagnostic::error(
                codes::E2002,
                format!("unknown {} `{name}`", context_kind_label(kind)),
                span,
            ));
        };
        match value {
            Value::String(value) => Ok(value.clone()),
            _ => Err(Diagnostic::error(
                codes::E2004,
                format!("context value `{name}` must be a string to resolve a column name"),
                span,
            )),
        }
    }

    fn execute_binding(&mut self, name: &str, reference_span: Span) -> Result<Table, Diagnostic> {
        if let Some(table) = self.cache.get(name) {
            return Ok(table.clone());
        }
        if let Some(index) = self
            .active_bindings
            .iter()
            .position(|active| active == name)
        {
            let mut path = self.active_bindings[index..].to_vec();
            path.push(name.to_string());
            return Err(Diagnostic::error(
                codes::E1501,
                format!("binding dependency cycle: {}", path.join(" -> ")),
                reference_span,
            ));
        }
        let binding = self
            .prepared
            .analysis
            .ir
            .as_ref()
            .and_then(|ir| ir.bindings.iter().find(|binding| binding.name == name))
            .ok_or_else(|| {
                Diagnostic::error(
                    codes::E1007,
                    format!("unknown binding `{name}`"),
                    reference_span,
                )
            })?;
        self.active_bindings.push(name.to_string());
        let table = self.execute_pipeline(&binding.pipeline)?;
        self.active_bindings.pop();
        self.cache.insert(name.to_string(), table.clone());
        Ok(table)
    }

    fn execute_load(
        &self,
        stage_span: Span,
        explicit_format: Option<&str>,
    ) -> Result<Table, Diagnostic> {
        let Some(input) = self.prepared.driver_plan.input_for_stage_span(stage_span) else {
            return Err(Diagnostic::error(
                codes::E1505,
                "driver source facts are unavailable for execution",
                stage_span,
            ));
        };
        match &input.source {
            SourceDescriptor::Path { resolved_path, .. } => {
                let bytes = self.io.read_path_bytes(resolved_path)?;
                let format =
                    resolve_input_format(input, explicit_format, None, Some(&bytes), stage_span)?;
                read_table_from_bytes(resolved_path, format, &bytes)
            }
            SourceDescriptor::Stdin => {
                let owned_bytes;
                let bytes = if let Some(bytes) = self.prepared.stdin_bytes.as_deref() {
                    bytes
                } else {
                    owned_bytes = self.io.read_stdin_bytes()?;
                    &owned_bytes
                };
                let format = resolve_input_format(
                    input,
                    explicit_format,
                    self.prepared.stdin_format.as_deref(),
                    Some(bytes),
                    stage_span,
                )?;
                read_table_from_bytes(std::path::Path::new("stdin"), format, bytes)
            }
        }
    }

    fn execute_save(
        &mut self,
        stage_span: Span,
        explicit_format: Option<&str>,
        table: &Table,
    ) -> Result<(), Diagnostic> {
        if self.dry_run {
            return Ok(());
        }
        let Some(sink) = self.prepared.driver_plan.sink_for_stage_span(stage_span) else {
            return Err(Diagnostic::error(
                codes::E1505,
                "driver sink facts are unavailable for execution",
                stage_span,
            ));
        };
        let format = resolve_output_format(sink, explicit_format, stage_span)?;
        match &sink.sink {
            SinkDescriptor::Path { resolved_path, .. } => {
                write_output(resolved_path, format, table)
            }
            SinkDescriptor::Stdout => {
                let bytes = emit_stdout(format, table)?;
                self.stdout = Some(bytes);
                Ok(())
            }
        }
    }

    fn filter(&self, table: Table, expr: &ExprIr) -> Result<Table, Diagnostic> {
        let rows = table
            .rows
            .iter()
            .filter_map(|row| {
                match eval_row_expr(
                    expr,
                    &table,
                    row,
                    ExprRole::PredicateRoot,
                    None,
                    &self.context,
                ) {
                    Ok(value) if value.is_truthy_true() => Some(Ok(row.clone())),
                    Ok(_) => None,
                    Err(diagnostic) => Some(Err(diagnostic)),
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Table {
            columns: table.columns,
            rows,
        })
    }

    fn aggregate(
        &self,
        table: &Table,
        group_keys: Vec<String>,
        items: &[AggItemIr],
    ) -> Result<Table, Diagnostic> {
        let mut grouped: BTreeMap<Vec<String>, Vec<&Row>> = BTreeMap::new();
        if group_keys.is_empty() {
            grouped.insert(Vec::new(), table.rows.iter().collect());
        } else {
            for row in &table.rows {
                let key = group_keys
                    .iter()
                    .map(|column| {
                        table
                            .value(row, column)
                            .unwrap_or(&Value::Null)
                            .to_csv_cell()
                    })
                    .collect::<Vec<_>>();
                grouped.entry(key).or_default().push(row);
            }
        }

        let mut columns = group_keys.clone();
        columns.extend(items.iter().map(|item| item.alias.clone()));
        let mut rows = Vec::new();

        for (key, group_rows) in grouped {
            let mut values = key.into_iter().map(Value::String).collect::<Vec<_>>();
            for item in items {
                values.push(eval_aggregate(item, table, &group_rows, &self.context)?);
            }
            rows.push(Row { values });
        }

        Ok(Table { columns, rows })
    }

    fn mutate(&self, table: Table, items: &[MutateItemIr]) -> Result<Table, Diagnostic> {
        let input_columns = table.columns.clone();
        let mut columns = input_columns.clone();
        for item in items {
            if !columns.iter().any(|column| column == &item.column) {
                columns.push(item.column.clone());
            }
        }

        let rows = table
            .rows
            .iter()
            .enumerate()
            .map(|(row_index, row)| {
                let mut values = row.values.clone();
                for item in items {
                    let value = eval_row_expr(
                        &item.expr,
                        &table,
                        row,
                        ExprRole::Default,
                        Some(row_index),
                        &self.context,
                    )?;
                    if let Some(index) = input_columns
                        .iter()
                        .position(|column| column == &item.column)
                    {
                        values[index] = value;
                    } else {
                        values.push(value);
                    }
                }
                Ok(Row { values })
            })
            .collect::<Result<Vec<_>, Diagnostic>>()?;

        Ok(Table { columns, rows })
    }

    fn join(
        &self,
        left: Table,
        right: Table,
        keys: &[(String, String)],
        kind: JoinKindIr,
        span: Span,
    ) -> Result<Table, Diagnostic> {
        if keys.is_empty() {
            return Err(Diagnostic::error(
                codes::E1203,
                "join requires at least one key",
                span,
            ));
        }
        for (left_key, right_key) in keys {
            ensure_key_types_compatible(&left, left_key, &right, right_key, span)?;
        }
        let output_columns = join_columns(&left.columns, &right.columns, keys, kind, span)?;
        if matches!(kind, JoinKindIr::Semi | JoinKindIr::Anti) {
            return Ok(join_semi_anti(left, &right, keys, kind));
        }

        let left_key_indices = keys
            .iter()
            .map(|(left_key, _)| {
                left.column_index(left_key).ok_or_else(|| {
                    Diagnostic::error(codes::E1005, format!("unknown column `{left_key}`"), span)
                })
            })
            .collect::<Result<Vec<_>, Diagnostic>>()?;
        let right_key_indices = keys
            .iter()
            .map(|(_, right_key)| {
                right.column_index(right_key).ok_or_else(|| {
                    Diagnostic::error(codes::E1005, format!("unknown column `{right_key}`"), span)
                })
            })
            .collect::<Result<Vec<_>, Diagnostic>>()?;
        let left_matches = join_index(&left, &left_key_indices);
        let right_matches = join_index(&right, &right_key_indices);
        let right_value_indices = right_non_key_indices(&right.columns, keys);
        let mut rows = Vec::new();

        match kind {
            JoinKindIr::Inner | JoinKindIr::Left | JoinKindIr::Full => {
                let mut matched_right = vec![false; right.rows.len()];
                for left_row in &left.rows {
                    let key = row_join_key(left_row, &left_key_indices);
                    let matches = key.as_ref().and_then(|key| right_matches.get(key));
                    if let Some(matches) = matches {
                        for right_index in matches {
                            matched_right[*right_index] = true;
                            rows.push(combine_rows(
                                left_row,
                                Some(&right.rows[*right_index]),
                                &right_value_indices,
                                left.columns.len(),
                            ));
                        }
                    } else if matches!(kind, JoinKindIr::Left | JoinKindIr::Full) {
                        rows.push(combine_rows(
                            left_row,
                            None,
                            &right_value_indices,
                            left.columns.len(),
                        ));
                    }
                }
                if matches!(kind, JoinKindIr::Full) {
                    let mut unmatched_right = right
                        .rows
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| !matched_right[*index])
                        .collect::<Vec<_>>();
                    unmatched_right.sort_by(|(_, left_row), (_, right_row)| {
                        row_join_key(left_row, &right_key_indices)
                            .cmp(&row_join_key(right_row, &right_key_indices))
                    });
                    for (_, right_row) in unmatched_right {
                        rows.push(right_only_row(
                            right_row,
                            &right_key_indices,
                            &left_key_indices,
                            left.columns.len(),
                            &right_value_indices,
                        ));
                    }
                }
            }
            JoinKindIr::Right => {
                for right_row in &right.rows {
                    let key = row_join_key(right_row, &right_key_indices);
                    let matches = key.as_ref().and_then(|key| left_matches.get(key));
                    if let Some(matches) = matches {
                        for left_index in matches {
                            rows.push(combine_rows(
                                &left.rows[*left_index],
                                Some(right_row),
                                &right_value_indices,
                                left.columns.len(),
                            ));
                        }
                    } else {
                        rows.push(right_only_row(
                            right_row,
                            &right_key_indices,
                            &left_key_indices,
                            left.columns.len(),
                            &right_value_indices,
                        ));
                    }
                }
            }
            JoinKindIr::Semi | JoinKindIr::Anti => unreachable!("handled earlier"),
        }

        Ok(Table {
            columns: output_columns,
            rows,
        })
    }

    fn union(
        &self,
        left: Table,
        right: Table,
        by_name: bool,
        distinct: bool,
        span: Span,
    ) -> Result<Table, Diagnostic> {
        ensure_union_compatible(&left, &right, by_name, span)?;
        let columns = if by_name {
            let mut columns = left.columns.clone();
            for column in &right.columns {
                if !columns.iter().any(|existing| existing == column) {
                    columns.push(column.clone());
                }
            }
            columns
        } else {
            let width = left.columns.len().max(right.columns.len());
            let mut columns = left.columns.clone();
            for index in columns.len()..width {
                columns.push(
                    right
                        .columns
                        .get(index)
                        .cloned()
                        .unwrap_or_else(|| format!("column_{}", index + 1)),
                );
            }
            columns
        };
        let mut rows = left
            .rows
            .iter()
            .map(|row| Row {
                values: (0..columns.len())
                    .map(|index| row.values.get(index).cloned().unwrap_or(Value::Null))
                    .collect(),
            })
            .collect::<Vec<_>>();
        if by_name {
            let right_indices = columns
                .iter()
                .map(|column| right.column_index(column))
                .collect::<Vec<_>>();
            rows.extend(right.rows.iter().map(|row| {
                Row {
                    values: right_indices
                        .iter()
                        .map(|index| {
                            index
                                .and_then(|index| row.values.get(index))
                                .cloned()
                                .unwrap_or(Value::Null)
                        })
                        .collect(),
                }
            }));
        } else {
            rows.extend(right.rows.iter().map(|row| {
                Row {
                    values: (0..columns.len())
                        .map(|index| row.values.get(index).cloned().unwrap_or(Value::Null))
                        .collect(),
                }
            }));
        }
        let table = Table { columns, rows };
        Ok(if distinct { table.distinct(&[]) } else { table })
    }
}

pub(super) fn resolve_input_format(
    input: &PlanInputSource,
    explicit_format: Option<&str>,
    stdin_format: Option<&str>,
    bytes: Option<&[u8]>,
    span: Span,
) -> Result<DataFormat, Diagnostic> {
    if let Some(format) = explicit_format {
        return DataFormat::from_name(format).ok_or_else(|| {
            Diagnostic::error(
                codes::E1215,
                format!("format `{format}` is not supported in 0.26.0"),
                span,
            )
        });
    }
    if matches!(&input.source, SourceDescriptor::Stdin) {
        if let Some(format) = stdin_format {
            return DataFormat::from_name(format).ok_or_else(|| {
                Diagnostic::error(
                    codes::E1215,
                    format!("stdin format `{format}` is not supported in 0.26.0"),
                    input.span,
                )
            });
        }
    }
    if let Some(format) = input.format.inferred_from_path {
        return Ok(format);
    }
    if let Some(bytes) = bytes {
        return sniff_format_from_bytes(bytes);
    }
    Ok(DataFormat::Csv)
}

pub(super) fn resolve_output_format(
    sink: &PlanOutputSink,
    explicit_format: Option<&str>,
    span: Span,
) -> Result<DataFormat, Diagnostic> {
    if let Some(format) = explicit_format {
        return DataFormat::from_name(format).ok_or_else(|| {
            Diagnostic::error(
                codes::E1705,
                format!("output format `{format}` is not supported in 0.26.0"),
                span,
            )
        });
    }
    format_from_decision(&sink.format).ok_or_else(|| {
        Diagnostic::error(
            codes::E1705,
            "could not infer supported output format",
            sink.span,
        )
    })
}

fn format_from_decision(decision: &FormatDecision) -> Option<DataFormat> {
    decision
        .explicit
        .as_deref()
        .and_then(DataFormat::from_name)
        .or(decision.inferred_from_path)
        .or(Some(DataFormat::Csv))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdl_driver::{
        prepare_source_for_run_with_io, prepare_source_with_io, InMemoryDriverIo, OsDriverIo,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn runs_csv_stdin_with_explicit_format() {
        let io = InMemoryDriverIo::default()
            .with_stdin_bytes("status,amount\ncompleted,10\npending,20\n");
        let prepared = prepare_source_for_run_with_io(
            "memory/main.pdl",
            r#"load stdin format "csv"
  | filter status == "completed"
  | select amount"#,
            None,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            String::from_utf8(result.stdout.expect("csv stdout")).expect("utf8 csv"),
            "amount\n10\n"
        );
    }

    /// v0.46 stdin / host-byte promotion: the same program over the same
    /// in-memory input must produce byte-identical stdout on the row,
    /// auto, and forced native engines, with auto selecting native.
    fn assert_byte_backed_native_parity(io: &InMemoryDriverIo, source: &str) {
        let prepared = prepare_source_for_run_with_io("memory/main.pdl", source, None, io);
        let run = |engine| {
            run_prepared_with_io_and_context_and_engine(
                &prepared,
                RunOptions {
                    stdout_format: Some("csv".to_string()),
                    dry_run: false,
                    allow_binary_stdout: true,
                },
                io,
                BTreeMap::new(),
                engine,
            )
        };

        let row = run(ExecutionEngine::Row);
        assert!(row.diagnostics.is_empty(), "{:?}", row.diagnostics);
        let row_stdout = row.stdout.expect("row stdout");

        for engine in [ExecutionEngine::Auto, ExecutionEngine::Native] {
            let candidate = run(engine);
            assert!(
                candidate.diagnostics.is_empty(),
                "{engine:?}: {:?}",
                candidate.diagnostics
            );
            assert_eq!(candidate.backend, DataBackend::NativePolars, "{engine:?}");
            assert_eq!(
                String::from_utf8_lossy(&row_stdout),
                String::from_utf8_lossy(&candidate.stdout.expect("stdout")),
                "{engine:?} stdout differs from the row engine"
            );
        }
    }

    #[test]
    fn native_engine_runs_stdin_csv_with_row_parity() {
        let io = InMemoryDriverIo::default()
            .with_stdin_bytes("status,amount\ncompleted,10\npending,20\ncompleted,5\n");
        assert_byte_backed_native_parity(
            &io,
            r#"load stdin format "csv"
  | filter status == "completed"
  | select amount"#,
        );
    }

    #[test]
    fn native_engine_runs_sniffed_stdin_csv_with_row_parity() {
        // No explicit or CLI format: stdin resolution falls through to
        // sniffing and the CSV fallback, which must use the same
        // byte-backed scan as explicit CSV.
        let io = InMemoryDriverIo::default()
            .with_stdin_bytes("status,amount\ncompleted,10\npending,20\n");
        assert_byte_backed_native_parity(
            &io,
            r#"load stdin
  | filter status == "completed""#,
        );
    }

    #[test]
    fn native_engine_runs_stdin_parquet_with_row_parity() {
        let table = Table::new(
            vec!["status".to_string(), "amount".to_string()],
            vec![
                Row {
                    values: vec![Value::String("completed".to_string()), Value::Number(10.0)],
                },
                Row {
                    values: vec![Value::String("pending".to_string()), Value::Null],
                },
            ],
        );
        let bytes =
            pdl_data::write_table_to_bytes(DataFormat::Parquet, &table).expect("encode parquet");
        let io = InMemoryDriverIo::default().with_stdin_bytes(bytes);
        assert_byte_backed_native_parity(
            &io,
            r#"load stdin format "parquet"
  | filter status == "completed""#,
        );
    }

    #[test]
    fn native_engine_runs_host_byte_csv_with_row_parity() {
        let io = InMemoryDriverIo::default().with_file_bytes(
            "memory/orders.csv",
            "status,amount\ncompleted,10\npending,20\n",
        );
        assert_byte_backed_native_parity(
            &io,
            r#"load "orders.csv"
  | filter status == "completed""#,
        );
    }

    #[test]
    fn native_engine_runs_host_byte_parquet_with_row_parity() {
        let table = Table::new(
            vec!["status".to_string(), "amount".to_string()],
            vec![Row {
                values: vec![Value::String("completed".to_string()), Value::Number(10.0)],
            }],
        );
        let bytes =
            pdl_data::write_table_to_bytes(DataFormat::Parquet, &table).expect("encode parquet");
        let io = InMemoryDriverIo::default().with_file_bytes("memory/orders.parquet", bytes);
        assert_byte_backed_native_parity(
            &io,
            r#"load "orders.parquet"
  | filter status == "completed""#,
        );
    }

    #[test]
    fn native_engine_runs_stdin_json_lines_with_row_parity() {
        let io = InMemoryDriverIo::default()
            .with_stdin_bytes("{\"status\":\"completed\",\"amount\":10}\n");
        let prepared = prepare_source_for_run_with_io(
            "memory/main.pdl",
            r#"load stdin format "jsonl"
  | select status"#,
            None,
            &io,
        );
        let options = || RunOptions {
            stdout_format: Some("csv".to_string()),
            dry_run: false,
            allow_binary_stdout: true,
        };

        let auto = run_prepared_with_io_and_context_and_engine(
            &prepared,
            options(),
            &io,
            BTreeMap::new(),
            ExecutionEngine::Auto,
        );
        assert!(auto.diagnostics.is_empty(), "{:?}", auto.diagnostics);
        assert_eq!(auto.backend, DataBackend::NativePolars);
        assert_eq!(
            String::from_utf8(auto.stdout.expect("csv stdout")).expect("utf8 csv"),
            "status\ncompleted\n"
        );

        let forced = run_prepared_with_io_and_context_and_engine(
            &prepared,
            options(),
            &io,
            BTreeMap::new(),
            ExecutionEngine::Native,
        );
        assert!(forced.diagnostics.is_empty(), "{:?}", forced.diagnostics);
        assert_eq!(forced.backend, DataBackend::NativePolars);
        assert_eq!(
            String::from_utf8(forced.stdout.expect("csv stdout")).expect("utf8 csv"),
            "status\ncompleted\n"
        );
    }

    #[test]
    fn native_engine_runs_supported_path_backed_pipeline() {
        let workspace = temp_workspace("native-supported");
        fs::write(
            workspace.join("sales.csv"),
            "status,region,amount\ncompleted,West,30\npending,East,10\ncompleted,North,40\n",
        )
        .expect("write csv");
        let program_path = workspace.join("main.pdl");
        let io = OsDriverIo;
        let prepared = prepare_source_with_io(
            &program_path,
            r#"load "sales.csv"
  | filter status == "completed"
  | select region, amount
  | sort amount desc
  | limit 1"#,
            &io,
        );

        let result = run_prepared_with_io_and_context_and_engine(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            },
            &io,
            BTreeMap::new(),
            ExecutionEngine::Native,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(result.backend, DataBackend::NativePolars);
        assert_eq!(
            String::from_utf8(result.stdout.expect("csv stdout")).expect("utf8 csv"),
            "region,amount\nNorth,40\n"
        );
        fs::remove_dir_all(workspace).expect("clean temp workspace");
    }

    #[test]
    fn auto_engine_runs_mixed_named_outputs() {
        let workspace = temp_workspace("mixed-named-outputs");
        fs::write(
            workspace.join("sales.csv"),
            "region,amount\nWest,30\nEast,10\n",
        )
        .expect("write csv");
        fs::write(
            workspace.join("events.jsonl"),
            "{\"region\":\"North\",\"amount\":50}\n",
        )
        .expect("write jsonl");
        let program_path = workspace.join("main.pdl");
        let native_report_path = workspace.join("native_report.csv");
        let row_report_path = workspace.join("row_report.csv");
        let io = OsDriverIo;
        let source = format!(
            r#"output native_report =
  load "sales.csv"
  | sort amount desc
  | save "{}"

output row_report =
  load "events.jsonl"
  | save "{}""#,
            native_report_path.display(),
            row_report_path.display()
        );
        let prepared = prepare_source_with_io(&program_path, &source, &io);

        let result = run_prepared_with_io_and_context_and_engine(
            &prepared,
            RunOptions {
                stdout_format: None,
                dry_run: false,
                allow_binary_stdout: true,
            },
            &io,
            BTreeMap::new(),
            ExecutionEngine::Auto,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(result.backend, DataBackend::NativePolars);
        assert_eq!(
            fs::read_to_string(&native_report_path).expect("native report"),
            "region,amount\nWest,30\nEast,10\n"
        );
        assert_eq!(
            fs::read_to_string(&row_report_path).expect("row report"),
            "region,amount\nNorth,50\n"
        );
        assert_eq!(
            result
                .named_outputs
                .iter()
                .map(|output| output.name.as_str())
                .collect::<Vec<_>>(),
            vec!["native_report", "row_report"]
        );
        fs::remove_dir_all(workspace).expect("clean temp workspace");
    }

    #[test]
    fn native_engine_runs_mixed_class_pivot_longer() {
        let workspace = temp_workspace("native-fallback");
        fs::write(
            workspace.join("sales.csv"),
            "region,amount\nWest,30\nEast,10\n",
        )
        .expect("write csv");
        let program_path = workspace.join("main.pdl");
        let io = OsDriverIo;
        let prepared = prepare_source_with_io(
            &program_path,
            r#"load "sales.csv"
  | pivot_longer region, amount names_to metric values_to value"#,
            &io,
        );

        let auto = run_prepared_with_io_and_context_and_engine(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            },
            &io,
            BTreeMap::new(),
            ExecutionEngine::Auto,
        );
        assert!(auto.diagnostics.is_empty(), "{:?}", auto.diagnostics);
        assert_eq!(auto.backend, DataBackend::NativePolars);
        assert_eq!(
            String::from_utf8(auto.stdout.expect("csv stdout")).expect("utf8 csv"),
            "metric,value\nregion,West\namount,30\nregion,East\namount,10\n"
        );

        let forced = run_prepared_with_io_and_context_and_engine(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            },
            &io,
            BTreeMap::new(),
            ExecutionEngine::Native,
        );
        assert!(forced.diagnostics.is_empty(), "{:?}", forced.diagnostics);
        assert_eq!(forced.backend, DataBackend::NativePolars);
        assert_eq!(
            String::from_utf8(forced.stdout.expect("csv stdout")).expect("utf8 csv"),
            "metric,value\nregion,West\namount,30\nregion,East\namount,10\n"
        );
        fs::remove_dir_all(workspace).expect("clean temp workspace");
    }

    #[test]
    fn native_eligibility_accepts_temporal_functions_before_execution() {
        let workspace = temp_workspace("native-eligibility");
        fs::write(workspace.join("sales.csv"), "amount\n10\n").expect("write csv");
        let program_path = workspace.join("main.pdl");
        let io = OsDriverIo;
        let prepared = prepare_source_with_io(
            &program_path,
            r#"load "sales.csv"
  | mutate amount_day = date(amount)"#,
            &io,
        );
        let plan = plan_prepared(
            &prepared,
            PlanningOptions {
                stdout_format: Some(DataFormat::Csv.canonical_name().to_string()),
                dry_run: false,
                allow_binary_stdout: true,
                engine: PlannedEngine::Auto,
            },
        )
        .expect("execution plan");
        let ir = prepared.analysis.ir.as_ref().expect("ir");

        check_native_program_eligibility(&prepared, ir, &plan, &BTreeMap::new())
            .expect("temporal functions are native-eligible in v0.49");
        fs::remove_dir_all(workspace).expect("clean temp workspace");
    }

    #[test]
    fn native_engine_if_else_matches_rows_for_path_formats() {
        let workspace = temp_workspace("native-if-else");
        let table = Table::new(
            vec!["region".to_string(), "amount".to_string()],
            vec![
                Row {
                    values: vec![Value::String("West".to_string()), Value::Number(30.0)],
                },
                Row {
                    values: vec![Value::String("East".to_string()), Value::Number(10.0)],
                },
                Row {
                    values: vec![Value::String("North".to_string()), Value::Null],
                },
            ],
        );
        for (path, format) in [
            ("sales.csv", DataFormat::Csv),
            ("sales.parquet", DataFormat::Parquet),
            ("sales.arrow", DataFormat::ArrowStream),
        ] {
            pdl_data::write_table_to_path(&workspace.join(path), format, &table)
                .expect("write fixture");
        }

        for (input, format_clause) in [
            ("sales.csv", ""),
            ("sales.parquet", ""),
            ("sales.arrow", r#" format "arrow-stream""#),
        ] {
            let io = OsDriverIo;
            let prepared = prepare_source_with_io(
                workspace.join(format!("{input}.pdl")),
                format!(
                    r#"load "{input}"{format_clause}
  | mutate score = if_else(amount > 20, amount * 2, amount + 1), label = if_else(amount > 20, concat(region, ":high"), "standard")
  | select region, score, label
  | sort region"#
                ),
                &io,
            );
            let options = RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            };
            let row = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options.clone(),
                &io,
                BTreeMap::new(),
                ExecutionEngine::Row,
            );
            let auto = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options.clone(),
                &io,
                BTreeMap::new(),
                ExecutionEngine::Auto,
            );
            let native = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options,
                &io,
                BTreeMap::new(),
                ExecutionEngine::Native,
            );

            assert!(row.diagnostics.is_empty(), "{input}: {:?}", row.diagnostics);
            assert!(
                auto.diagnostics.is_empty(),
                "{input}: {:?}",
                auto.diagnostics
            );
            assert!(
                native.diagnostics.is_empty(),
                "{input}: {:?}",
                native.diagnostics
            );
            assert_eq!(auto.backend, DataBackend::NativePolars, "{input}");
            assert_eq!(native.backend, DataBackend::NativePolars, "{input}");
            let row_csv = String::from_utf8(row.stdout.expect("row csv")).expect("utf8");
            assert_eq!(
                String::from_utf8(auto.stdout.expect("auto csv")).expect("utf8"),
                row_csv,
                "{input}"
            );
            assert_eq!(
                String::from_utf8(native.stdout.expect("native csv")).expect("utf8"),
                row_csv,
                "{input}"
            );
            assert_eq!(
                row_csv,
                "region,score,label\nEast,11,standard\nNorth,,\nWest,60,West:high\n"
            );
        }
        fs::remove_dir_all(workspace).expect("clean temp workspace");
    }

    /// v0.45 `pivot_longer` promotion: single-id, multi-id, and empty-input
    /// pivots run natively across path-backed formats with bytes identical
    /// to the row engine. All-null value columns are asserted via automatic
    /// mode because scan inference may type them as string on some formats,
    /// in which case the mixed-class guard demotes to rows with identical
    /// bytes.
    #[test]
    fn native_engine_pivot_longer_matches_rows_for_path_formats() {
        let workspace = temp_workspace("native-pivot-longer");
        let table = Table::new(
            vec![
                "region".to_string(),
                "segment".to_string(),
                "q1".to_string(),
                "q2".to_string(),
                "q3".to_string(),
            ],
            vec![
                Row {
                    values: vec![
                        Value::String("West".to_string()),
                        Value::String("retail".to_string()),
                        Value::Number(10.0),
                        Value::Number(20.5),
                        Value::Null,
                    ],
                },
                Row {
                    values: vec![
                        Value::String("East".to_string()),
                        Value::String("b2b".to_string()),
                        Value::Number(5.0),
                        Value::Null,
                        Value::Null,
                    ],
                },
            ],
        );
        for (path, format) in [
            ("sales.csv", DataFormat::Csv),
            ("sales.parquet", DataFormat::Parquet),
            ("sales.arrow", DataFormat::ArrowStream),
        ] {
            pdl_data::write_table_to_path(&workspace.join(path), format, &table)
                .expect("write fixture");
        }

        let multi_id = "pivot_longer q1, q2 names_to quarter values_to amount";
        let single_id =
            "drop segment, q3\n  | pivot_longer q2, q1 names_to quarter values_to amount";
        let empty_input =
            "filter region == \"nope\"\n  | pivot_longer q1, q2 names_to quarter values_to amount";
        let all_null = "pivot_longer q3 names_to quarter values_to amount";

        for (input, format_clause) in [
            ("sales.csv", ""),
            ("sales.parquet", ""),
            ("sales.arrow", r#" format "arrow-stream""#),
        ] {
            for (label, stages, forced_native) in [
                ("multi-id", multi_id, true),
                ("single-id", single_id, true),
                ("empty-input", empty_input, true),
                ("all-null", all_null, false),
            ] {
                let io = OsDriverIo;
                let prepared = prepare_source_with_io(
                    workspace.join(format!("{input}.pdl")),
                    format!("load \"{input}\"{format_clause}\n  | {stages}"),
                    &io,
                );
                let options = RunOptions {
                    stdout_format: Some("csv".to_string()),
                    dry_run: false,
                    allow_binary_stdout: true,
                };
                let row = run_prepared_with_io_and_context_and_engine(
                    &prepared,
                    options.clone(),
                    &io,
                    BTreeMap::new(),
                    ExecutionEngine::Row,
                );
                let auto = run_prepared_with_io_and_context_and_engine(
                    &prepared,
                    options.clone(),
                    &io,
                    BTreeMap::new(),
                    ExecutionEngine::Auto,
                );
                assert!(
                    row.diagnostics.is_empty(),
                    "{input} {label}: {:?}",
                    row.diagnostics
                );
                assert!(
                    auto.diagnostics.is_empty(),
                    "{input} {label}: {:?}",
                    auto.diagnostics
                );
                let row_csv = String::from_utf8(row.stdout.expect("row csv")).expect("utf8");
                assert_eq!(
                    String::from_utf8(auto.stdout.expect("auto csv")).expect("utf8"),
                    row_csv,
                    "{input} {label}"
                );
                if forced_native {
                    let native = run_prepared_with_io_and_context_and_engine(
                        &prepared,
                        options,
                        &io,
                        BTreeMap::new(),
                        ExecutionEngine::Native,
                    );
                    assert!(
                        native.diagnostics.is_empty(),
                        "{input} {label}: {:?}",
                        native.diagnostics
                    );
                    assert_eq!(auto.backend, DataBackend::NativePolars, "{input} {label}");
                    assert_eq!(native.backend, DataBackend::NativePolars, "{input} {label}");
                    assert_eq!(
                        String::from_utf8(native.stdout.expect("native csv")).expect("utf8"),
                        row_csv,
                        "{input} {label}"
                    );
                }
                if label == "multi-id" {
                    assert_eq!(
                        row_csv,
                        "region,segment,q3,quarter,amount\n\
                         West,retail,,q1,10\n\
                         West,retail,,q2,20.5\n\
                         East,b2b,,q1,5\n\
                         East,b2b,,q2,\n",
                        "{input}"
                    );
                }
                if label == "single-id" {
                    assert_eq!(
                        row_csv,
                        "region,quarter,amount\n\
                         West,q2,20.5\n\
                         West,q1,10\n\
                         East,q2,\n\
                         East,q1,5\n",
                        "{input}"
                    );
                }
            }
        }
        fs::remove_dir_all(workspace).expect("clean temp workspace");
    }

    /// v0.45 `complete` promotion: single-key, composite-key, explicit-fill,
    /// and empty-input completions run natively across path-backed formats
    /// with bytes identical to the row engine.
    #[test]
    fn native_engine_complete_matches_rows_for_path_formats() {
        let workspace = temp_workspace("native-complete");
        let table = Table::new(
            vec![
                "region".to_string(),
                "day".to_string(),
                "visits".to_string(),
                "note".to_string(),
            ],
            vec![
                Row {
                    values: vec![
                        Value::String("West".to_string()),
                        Value::String("mon".to_string()),
                        Value::Number(12.0),
                        Value::String("ok".to_string()),
                    ],
                },
                Row {
                    values: vec![
                        Value::String("East".to_string()),
                        Value::String("tue".to_string()),
                        Value::Number(4.0),
                        Value::Null,
                    ],
                },
            ],
        );
        for (path, format) in [
            ("visits.csv", DataFormat::Csv),
            ("visits.parquet", DataFormat::Parquet),
            ("visits.arrow", DataFormat::ArrowStream),
        ] {
            pdl_data::write_table_to_path(&workspace.join(path), format, &table)
                .expect("write fixture");
        }

        let single_key = "complete region fill visits = 99";
        let composite_key = "complete region, day fill visits = 0, note = \"missing\"";
        let empty_input = "filter region == \"nope\"\n  | complete region, day fill visits = 0";

        for (input, format_clause) in [
            ("visits.csv", ""),
            ("visits.parquet", ""),
            ("visits.arrow", r#" format "arrow-stream""#),
        ] {
            for (label, stages) in [
                ("single-key", single_key),
                ("composite-key", composite_key),
                ("empty-input", empty_input),
            ] {
                let io = OsDriverIo;
                let prepared = prepare_source_with_io(
                    workspace.join(format!("{input}.pdl")),
                    format!("load \"{input}\"{format_clause}\n  | {stages}"),
                    &io,
                );
                let options = RunOptions {
                    stdout_format: Some("csv".to_string()),
                    dry_run: false,
                    allow_binary_stdout: true,
                };
                let row = run_prepared_with_io_and_context_and_engine(
                    &prepared,
                    options.clone(),
                    &io,
                    BTreeMap::new(),
                    ExecutionEngine::Row,
                );
                let auto = run_prepared_with_io_and_context_and_engine(
                    &prepared,
                    options.clone(),
                    &io,
                    BTreeMap::new(),
                    ExecutionEngine::Auto,
                );
                let native = run_prepared_with_io_and_context_and_engine(
                    &prepared,
                    options,
                    &io,
                    BTreeMap::new(),
                    ExecutionEngine::Native,
                );
                assert!(
                    row.diagnostics.is_empty(),
                    "{input} {label}: {:?}",
                    row.diagnostics
                );
                assert!(
                    auto.diagnostics.is_empty(),
                    "{input} {label}: {:?}",
                    auto.diagnostics
                );
                assert!(
                    native.diagnostics.is_empty(),
                    "{input} {label}: {:?}",
                    native.diagnostics
                );
                assert_eq!(auto.backend, DataBackend::NativePolars, "{input} {label}");
                assert_eq!(native.backend, DataBackend::NativePolars, "{input} {label}");
                let row_csv = String::from_utf8(row.stdout.expect("row csv")).expect("utf8");
                assert_eq!(
                    String::from_utf8(auto.stdout.expect("auto csv")).expect("utf8"),
                    row_csv,
                    "{input} {label}"
                );
                assert_eq!(
                    String::from_utf8(native.stdout.expect("native csv")).expect("utf8"),
                    row_csv,
                    "{input} {label}"
                );
                if label == "composite-key" {
                    assert_eq!(
                        row_csv,
                        "region,day,visits,note\n\
                         West,mon,12,ok\n\
                         West,tue,0,missing\n\
                         East,mon,0,missing\n\
                         East,tue,4,\n",
                        "{input}"
                    );
                }
            }
        }
        fs::remove_dir_all(workspace).expect("clean temp workspace");
    }

    #[test]
    fn native_engine_mutate_matches_rows_for_supported_expression_subset() {
        let workspace = temp_workspace("native-mutate");
        let table = Table::new(
            vec![
                "region".to_string(),
                "score".to_string(),
                "latency_ms".to_string(),
                "active".to_string(),
                "note".to_string(),
                "delta".to_string(),
            ],
            vec![
                Row {
                    values: vec![
                        Value::String(" West ".to_string()),
                        Value::Number(30.0),
                        Value::Number(100.0),
                        Value::Bool(true),
                        Value::Null,
                        Value::Number(-3.5),
                    ],
                },
                Row {
                    values: vec![
                        Value::String("east".to_string()),
                        Value::Number(50.0),
                        Value::Number(125.0),
                        Value::Bool(false),
                        Value::String(" late ".to_string()),
                        Value::Number(2.0),
                    ],
                },
                Row {
                    values: vec![
                        Value::String("North".to_string()),
                        Value::Number(10.0),
                        Value::Number(80.0),
                        Value::Null,
                        Value::String("ok".to_string()),
                        Value::Number(-1.0),
                    ],
                },
            ],
        );
        for (path, format) in [
            ("sales.csv", DataFormat::Csv),
            ("sales.parquet", DataFormat::Parquet),
            ("sales.arrow", DataFormat::ArrowStream),
        ] {
            pdl_data::write_table_to_path(&workspace.join(path), format, &table)
                .expect("write fixture");
        }

        for (input, format_clause) in [
            ("sales.csv", ""),
            ("sales.parquet", ""),
            ("sales.arrow", r#" format "arrow-stream""#),
        ] {
            let program_path = workspace.join(format!("{input}.pdl"));
            let io = OsDriverIo;
            let prepared = prepare_source_with_io(
                &program_path,
                format!(
                    r#"load "{input}"{format_clause}
  | mutate
      score = score + 1,
      score_copy = score,
      score_per_latency = round(score / latency_ms, 3),
      high_score = score >= 40,
      active_or_high = active or score >= 40,
      clean_region = lower(trim(region)),
      note_text = coalesce(note, "missing"),
      label = concat(upper(trim(region)), ":", coalesce(note, "missing")),
      missing_note = is_null(note),
      has_note = not_null(note),
      abs_delta = abs(delta)
  | select region, score, score_copy, score_per_latency, high_score, active_or_high, clean_region, note_text, label, missing_note, has_note, abs_delta
  | sort clean_region"#
                ),
                &io,
            );
            let options = RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            };
            let row = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options.clone(),
                &io,
                BTreeMap::new(),
                ExecutionEngine::Row,
            );
            let auto = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options.clone(),
                &io,
                BTreeMap::new(),
                ExecutionEngine::Auto,
            );
            let native = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options,
                &io,
                BTreeMap::new(),
                ExecutionEngine::Native,
            );

            assert!(row.diagnostics.is_empty(), "{input}: {:?}", row.diagnostics);
            assert!(
                auto.diagnostics.is_empty(),
                "{input}: {:?}",
                auto.diagnostics
            );
            assert!(
                native.diagnostics.is_empty(),
                "{input}: {:?}",
                native.diagnostics
            );
            assert_eq!(auto.backend, DataBackend::NativePolars, "{input}");
            assert_eq!(native.backend, DataBackend::NativePolars, "{input}");
            let row_csv = String::from_utf8(row.stdout.expect("row csv")).expect("utf8");
            assert_eq!(
                String::from_utf8(auto.stdout.expect("auto csv")).expect("utf8"),
                row_csv,
                "{input}"
            );
            assert_eq!(
                String::from_utf8(native.stdout.expect("native csv")).expect("utf8"),
                row_csv,
                "{input}"
            );
            assert!(
                row_csv.contains("score,score_copy"),
                "replacement should preserve existing score position and append score_copy: {row_csv}"
            );
        }
        fs::remove_dir_all(workspace).expect("clean temp workspace");
    }

    #[test]
    fn native_engine_to_number_matches_rows_for_path_formats() {
        let workspace = temp_workspace("native-to-number");
        let table = Table::new(
            vec!["id".to_string(), "raw".to_string()],
            vec![
                Row {
                    values: vec![
                        Value::String("a".to_string()),
                        Value::String(" 42.5 ".to_string()),
                    ],
                },
                Row {
                    values: vec![
                        Value::String("b".to_string()),
                        Value::String("bad".to_string()),
                    ],
                },
                Row {
                    values: vec![Value::String("c".to_string()), Value::Null],
                },
                Row {
                    values: vec![
                        Value::String("d".to_string()),
                        Value::String("-3".to_string()),
                    ],
                },
            ],
        );
        for (path, format) in [
            ("numbers.csv", DataFormat::Csv),
            ("numbers.parquet", DataFormat::Parquet),
            ("numbers.arrow", DataFormat::ArrowStream),
        ] {
            pdl_data::write_table_to_path(&workspace.join(path), format, &table)
                .expect("write fixture");
        }

        for (input, format_clause) in [
            ("numbers.csv", ""),
            ("numbers.parquet", ""),
            ("numbers.arrow", r#" format "arrow-stream""#),
        ] {
            let io = OsDriverIo;
            let prepared = prepare_source_with_io(
                workspace.join(format!("{input}.pdl")),
                format!(
                    r#"load "{input}"{format_clause}
  | mutate parsed = to_number(raw)
  | select id, parsed
  | sort id"#
                ),
                &io,
            );
            let options = RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            };
            let row = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options.clone(),
                &io,
                BTreeMap::new(),
                ExecutionEngine::Row,
            );
            let auto = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options.clone(),
                &io,
                BTreeMap::new(),
                ExecutionEngine::Auto,
            );
            let native = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options,
                &io,
                BTreeMap::new(),
                ExecutionEngine::Native,
            );

            assert!(row.diagnostics.is_empty(), "{input}: {:?}", row.diagnostics);
            assert!(
                auto.diagnostics.is_empty(),
                "{input}: {:?}",
                auto.diagnostics
            );
            assert!(
                native.diagnostics.is_empty(),
                "{input}: {:?}",
                native.diagnostics
            );
            assert_eq!(auto.backend, DataBackend::NativePolars, "{input}");
            assert_eq!(native.backend, DataBackend::NativePolars, "{input}");
            let row_csv = String::from_utf8(row.stdout.expect("row csv")).expect("utf8");
            assert_eq!(
                String::from_utf8(auto.stdout.expect("auto csv")).expect("utf8"),
                row_csv,
                "{input}"
            );
            assert_eq!(
                String::from_utf8(native.stdout.expect("native csv")).expect("utf8"),
                row_csv,
                "{input}"
            );
            assert_eq!(row_csv, "id,parsed\na,42.5\nb,\nc,\nd,-3\n");
        }
        fs::remove_dir_all(workspace).expect("clean temp workspace");
    }

    #[test]
    fn native_engine_extended_scalars_match_rows_for_path_formats() {
        let workspace = temp_workspace("native-extended-scalars");
        let table = Table::new(
            vec![
                "id".to_string(),
                "text".to_string(),
                "pattern".to_string(),
                "prefix".to_string(),
                "replacement".to_string(),
                "boolish".to_string(),
                "flag".to_string(),
                "raw".to_string(),
                "maybe_null".to_string(),
            ],
            vec![
                Row {
                    values: vec![
                        Value::String("a".to_string()),
                        Value::String("alpha-beta".to_string()),
                        Value::String("beta".to_string()),
                        Value::String("alpha".to_string()),
                        Value::String("B".to_string()),
                        Value::String(" true ".to_string()),
                        Value::Bool(true),
                        Value::Number(42.5),
                        Value::Null,
                    ],
                },
                Row {
                    values: vec![
                        Value::String("b".to_string()),
                        Value::String("omega".to_string()),
                        Value::String("alp".to_string()),
                        Value::String("om".to_string()),
                        Value::String("X".to_string()),
                        Value::String("false".to_string()),
                        Value::Bool(false),
                        Value::Number(-3.25),
                        Value::String("xray".to_string()),
                    ],
                },
                Row {
                    values: vec![
                        Value::String("c".to_string()),
                        Value::Null,
                        Value::String("a".to_string()),
                        Value::String("a".to_string()),
                        Value::String("z".to_string()),
                        Value::String("maybe".to_string()),
                        Value::Null,
                        Value::Null,
                        Value::String("none".to_string()),
                    ],
                },
            ],
        );
        for (path, format) in [
            ("scalars.csv", DataFormat::Csv),
            ("scalars.parquet", DataFormat::Parquet),
            ("scalars.arrow", DataFormat::ArrowStream),
        ] {
            pdl_data::write_table_to_path(&workspace.join(path), format, &table)
                .expect("write fixture");
        }

        for (input, format_clause) in [
            ("scalars.csv", ""),
            ("scalars.parquet", ""),
            ("scalars.arrow", r#" format "arrow-stream""#),
        ] {
            let io = OsDriverIo;
            let prepared = prepare_source_with_io(
                workspace.join(format!("{input}.pdl")),
                format!(
                    r#"load "{input}"{format_clause}
  | mutate
      text_out = to_string(text),
      raw_text = to_string(raw),
      flag_text = to_string(flag),
      parsed_bool = to_boolean(boolish),
      flag_bool = to_boolean(flag),
      has_pattern = contains(text, pattern),
      starts_prefix = starts_with(text, prefix),
      swapped = replace(text, "-", " "),
      null_contains = contains(maybe_null, "x")
  | select id, text_out, raw_text, flag_text, parsed_bool, flag_bool, has_pattern, starts_prefix, swapped, null_contains
  | sort id"#
                ),
                &io,
            );
            let options = RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            };
            let row = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options.clone(),
                &io,
                BTreeMap::new(),
                ExecutionEngine::Row,
            );
            let auto = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options.clone(),
                &io,
                BTreeMap::new(),
                ExecutionEngine::Auto,
            );
            let native = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options,
                &io,
                BTreeMap::new(),
                ExecutionEngine::Native,
            );

            assert!(row.diagnostics.is_empty(), "{input}: {:?}", row.diagnostics);
            assert!(
                auto.diagnostics.is_empty(),
                "{input}: {:?}",
                auto.diagnostics
            );
            assert!(
                native.diagnostics.is_empty(),
                "{input}: {:?}",
                native.diagnostics
            );
            assert_eq!(auto.backend, DataBackend::NativePolars, "{input}");
            assert_eq!(native.backend, DataBackend::NativePolars, "{input}");
            let row_csv = String::from_utf8(row.stdout.expect("row csv")).expect("utf8");
            assert_eq!(
                String::from_utf8(auto.stdout.expect("auto csv")).expect("utf8"),
                row_csv,
                "{input}"
            );
            assert_eq!(
                String::from_utf8(native.stdout.expect("native csv")).expect("utf8"),
                row_csv,
                "{input}"
            );
            assert_eq!(
                row_csv,
                "id,text_out,raw_text,flag_text,parsed_bool,flag_bool,has_pattern,starts_prefix,swapped,null_contains\na,alpha-beta,42.5,true,true,true,true,true,alpha beta,\nb,omega,-3.25,false,false,false,false,true,omega,true\nc,,,,,,,,,false\n"
            );
        }
        fs::remove_dir_all(workspace).expect("clean temp workspace");
    }

    #[test]
    fn native_engine_loads_arrow_file_by_path() {
        let workspace = temp_workspace("native-arrow-file-input");
        let table = Table::new(
            vec!["region".to_string(), "amount".to_string()],
            vec![
                Row {
                    values: vec![Value::String("West".to_string()), Value::Number(30.0)],
                },
                Row {
                    values: vec![Value::String("East".to_string()), Value::Number(10.0)],
                },
            ],
        );
        pdl_data::write_table_to_path(
            &workspace.join("sales.arrow"),
            DataFormat::ArrowFile,
            &table,
        )
        .expect("write arrow file");
        let io = OsDriverIo;
        let prepared = prepare_source_with_io(
            workspace.join("main.pdl"),
            r#"load "sales.arrow"
  | mutate doubled = amount * 2
  | select region, doubled
  | sort region"#,
            &io,
        );

        let result = run_prepared_with_io_and_context_and_engine(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            },
            &io,
            BTreeMap::new(),
            ExecutionEngine::Auto,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(result.backend, DataBackend::NativePolars);
        assert_eq!(
            String::from_utf8(result.stdout.expect("csv stdout")).expect("utf8 csv"),
            "region,doubled\nEast,20\nWest,60\n"
        );
        fs::remove_dir_all(workspace).expect("clean temp workspace");
    }

    #[test]
    fn native_engine_loads_arrow_file_from_host_bytes() {
        let table = Table::new(
            vec!["region".to_string(), "amount".to_string()],
            vec![
                Row {
                    values: vec![Value::String("West".to_string()), Value::Number(30.0)],
                },
                Row {
                    values: vec![Value::String("East".to_string()), Value::Number(10.0)],
                },
            ],
        );
        let bytes =
            pdl_data::write_table_to_bytes(DataFormat::ArrowFile, &table).expect("arrow file");
        let io = InMemoryDriverIo::default().with_file_bytes("memory/sales.arrow", bytes);
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "sales.arrow"
  | mutate doubled = amount * 2
  | select region, doubled
  | sort region"#,
            &io,
        );

        let result = run_prepared_with_io_and_context_and_engine(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            },
            &io,
            BTreeMap::new(),
            ExecutionEngine::Auto,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(result.backend, DataBackend::NativePolars);
        assert_eq!(
            String::from_utf8(result.stdout.expect("csv stdout")).expect("utf8 csv"),
            "region,doubled\nEast,20\nWest,60\n"
        );
    }

    #[test]
    fn native_engine_loads_arrow_stream_from_stdin_bytes() {
        let table = Table::new(
            vec!["region".to_string(), "amount".to_string()],
            vec![
                Row {
                    values: vec![Value::String("West".to_string()), Value::Number(30.0)],
                },
                Row {
                    values: vec![Value::String("East".to_string()), Value::Number(10.0)],
                },
            ],
        );
        let stdin =
            pdl_data::write_table_to_bytes(DataFormat::ArrowStream, &table).expect("arrow stream");
        let io = InMemoryDriverIo::default().with_stdin_bytes(stdin);
        let prepared = prepare_source_for_run_with_io(
            "memory/main.pdl",
            r#"load stdin
  | mutate doubled = amount * 2
  | select region, doubled
  | sort region"#,
            None,
            &io,
        );

        let result = run_prepared_with_io_and_context_and_engine(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            },
            &io,
            BTreeMap::new(),
            ExecutionEngine::Auto,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(result.backend, DataBackend::NativePolars);
        assert_eq!(
            String::from_utf8(result.stdout.expect("csv stdout")).expect("utf8 csv"),
            "region,doubled\nEast,20\nWest,60\n"
        );
    }

    #[test]
    fn native_engine_left_join_matches_rows_for_binding_input() {
        let workspace = temp_workspace("native-left-join");
        fs::write(
            workspace.join("sales.csv"),
            "customer_id,amount,segment\nc1,30,old\nc2,10,old\nc3,5,old\n,99,old\n",
        )
        .expect("write sales");
        fs::write(
            workspace.join("customers.csv"),
            "customer_id,segment,region\nc1,enterprise,West\nc2,self-serve,East\n,unknown,NullRegion\n",
        )
        .expect("write customers");
        let io = OsDriverIo;
        let prepared = prepare_source_with_io(
            workspace.join("main.pdl"),
            r#"let customers =
  load "customers.csv"

load "sales.csv"
  | join customers on customer_id kind left
  | select customer_id, amount, segment, segment_right, region
  | sort customer_id, amount"#,
            &io,
        );
        let options = RunOptions {
            stdout_format: Some("csv".to_string()),
            dry_run: false,
            allow_binary_stdout: true,
        };
        let row = run_prepared_with_io_and_context_and_engine(
            &prepared,
            options.clone(),
            &io,
            BTreeMap::new(),
            ExecutionEngine::Row,
        );
        let auto = run_prepared_with_io_and_context_and_engine(
            &prepared,
            options.clone(),
            &io,
            BTreeMap::new(),
            ExecutionEngine::Auto,
        );
        let native = run_prepared_with_io_and_context_and_engine(
            &prepared,
            options,
            &io,
            BTreeMap::new(),
            ExecutionEngine::Native,
        );

        assert!(row.diagnostics.is_empty(), "{:?}", row.diagnostics);
        assert!(auto.diagnostics.is_empty(), "{:?}", auto.diagnostics);
        assert!(native.diagnostics.is_empty(), "{:?}", native.diagnostics);
        assert_eq!(auto.backend, DataBackend::NativePolars);
        assert_eq!(native.backend, DataBackend::NativePolars);
        let row_csv = String::from_utf8(row.stdout.expect("row csv")).expect("utf8");
        assert_eq!(
            String::from_utf8(auto.stdout.expect("auto csv")).expect("utf8"),
            row_csv
        );
        assert_eq!(
            String::from_utf8(native.stdout.expect("native csv")).expect("utf8"),
            row_csv
        );
        assert_eq!(
            row_csv,
            "customer_id,amount,segment,segment_right,region\nc1,30,old,enterprise,West\nc2,10,old,self-serve,East\nc3,5,old,,\n,99,old,,\n"
        );
        fs::remove_dir_all(workspace).expect("clean temp workspace");
    }

    #[test]
    fn native_engine_composite_join_matches_rows_for_binding_input() {
        let workspace = temp_workspace("native-composite-join");
        fs::write(
            workspace.join("sales.csv"),
            "sale_id,customer_id,order_date,sku,region,amount\nS1,C1,2026-01-01,A,West,30\nS2,C1,2026-01-02,A,West,20\nS3,C2,2026-01-01,B,East,15\nS4,C3,2026-01-01,A,West,7\n",
        )
        .expect("write sales");
        fs::write(
            workspace.join("customer_days.csv"),
            "customer_id,order_date,tier\nC1,2026-01-01,gold\nC2,2026-01-01,silver\nC1,2026-01-03,bronze\n",
        )
        .expect("write customer_days");
        fs::write(
            workspace.join("catalog.csv"),
            "product_sku,market,category\nA,West,apparel\nB,East,books\nA,East,regional\n",
        )
        .expect("write catalog");
        let io = OsDriverIo;
        let prepared = prepare_source_with_io(
            workspace.join("main.pdl"),
            r#"let customer_days =
  load "customer_days.csv"

let catalog =
  load "catalog.csv"

load "sales.csv"
  | join customer_days on customer_id, order_date kind left
  | join catalog on (sku, product_sku), (region, market) kind left
  | select sale_id, customer_id, order_date, sku, region, tier, category
  | sort sale_id"#,
            &io,
        );
        let options = RunOptions {
            stdout_format: Some("csv".to_string()),
            dry_run: false,
            allow_binary_stdout: true,
        };
        let row = run_prepared_with_io_and_context_and_engine(
            &prepared,
            options.clone(),
            &io,
            BTreeMap::new(),
            ExecutionEngine::Row,
        );
        let auto = run_prepared_with_io_and_context_and_engine(
            &prepared,
            options.clone(),
            &io,
            BTreeMap::new(),
            ExecutionEngine::Auto,
        );
        let native = run_prepared_with_io_and_context_and_engine(
            &prepared,
            options,
            &io,
            BTreeMap::new(),
            ExecutionEngine::Native,
        );

        assert!(row.diagnostics.is_empty(), "{:?}", row.diagnostics);
        assert!(auto.diagnostics.is_empty(), "{:?}", auto.diagnostics);
        assert!(native.diagnostics.is_empty(), "{:?}", native.diagnostics);
        assert_eq!(auto.backend, DataBackend::NativePolars);
        assert_eq!(native.backend, DataBackend::NativePolars);
        let row_csv = String::from_utf8(row.stdout.expect("row csv")).expect("utf8");
        assert_eq!(
            String::from_utf8(auto.stdout.expect("auto csv")).expect("utf8"),
            row_csv
        );
        assert_eq!(
            String::from_utf8(native.stdout.expect("native csv")).expect("utf8"),
            row_csv
        );
        assert_eq!(
            row_csv,
            "sale_id,customer_id,order_date,sku,region,tier,category\nS1,C1,2026-01-01,A,West,gold,apparel\nS2,C1,2026-01-02,A,West,,apparel\nS3,C2,2026-01-01,B,East,silver,books\nS4,C3,2026-01-01,A,West,,apparel\n"
        );

        fs::remove_dir_all(workspace).expect("clean temp workspace");
    }

    #[test]
    fn native_engine_composite_join_kinds_match_rows_for_binding_input() {
        let workspace = temp_workspace("native-composite-join-kinds");
        fs::write(
            workspace.join("left.csv"),
            "id,region,left_value,shared\nA,West,L-AW,LS-AW\nA,East,L-AE,LS-AE\nB,West,L-BW,LS-BW\nD,West,L-DW,LS-DW\n",
        )
        .expect("write left");
        fs::write(
            workspace.join("right.csv"),
            "right_id,market,right_value,shared\nA,West,R-AW,RS-AW\nA,East,R-AE,RS-AE\nB,East,R-BE,RS-BE\nC,West,R-CW,RS-CW\n,West,R-null,RS-null\n",
        )
        .expect("write right");

        for (kind, expected) in [
            (
                "inner",
                "id,region,left_value,shared,right_value,shared_right\nA,West,L-AW,LS-AW,R-AW,RS-AW\nA,East,L-AE,LS-AE,R-AE,RS-AE\n",
            ),
            (
                "left",
                "id,region,left_value,shared,right_value,shared_right\nA,West,L-AW,LS-AW,R-AW,RS-AW\nA,East,L-AE,LS-AE,R-AE,RS-AE\nB,West,L-BW,LS-BW,,\nD,West,L-DW,LS-DW,,\n",
            ),
            (
                "right",
                "id,region,left_value,shared,right_value,shared_right\nA,West,L-AW,LS-AW,R-AW,RS-AW\nA,East,L-AE,LS-AE,R-AE,RS-AE\nB,East,,,R-BE,RS-BE\nC,West,,,R-CW,RS-CW\n,West,,,R-null,RS-null\n",
            ),
            (
                "full",
                "id,region,left_value,shared,right_value,shared_right\nA,West,L-AW,LS-AW,R-AW,RS-AW\nA,East,L-AE,LS-AE,R-AE,RS-AE\nB,West,L-BW,LS-BW,,\nD,West,L-DW,LS-DW,,\n,West,,,R-null,RS-null\nB,East,,,R-BE,RS-BE\nC,West,,,R-CW,RS-CW\n",
            ),
            (
                "semi",
                "id,region,left_value,shared\nA,West,L-AW,LS-AW\nA,East,L-AE,LS-AE\n",
            ),
            (
                "anti",
                "id,region,left_value,shared\nB,West,L-BW,LS-BW\nD,West,L-DW,LS-DW\n",
            ),
        ] {
            let io = OsDriverIo;
            let prepared = prepare_source_with_io(
                workspace.join(format!("{kind}.pdl")),
                format!(
                    r#"let right_side =
  load "right.csv"

load "left.csv"
  | join right_side on (id, right_id), (region, market) kind {kind}"#
                ),
                &io,
            );
            let options = RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            };
            let row = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options.clone(),
                &io,
                BTreeMap::new(),
                ExecutionEngine::Row,
            );
            let auto = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options.clone(),
                &io,
                BTreeMap::new(),
                ExecutionEngine::Auto,
            );
            let native = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options,
                &io,
                BTreeMap::new(),
                ExecutionEngine::Native,
            );

            assert!(row.diagnostics.is_empty(), "{kind}: {:?}", row.diagnostics);
            assert!(
                auto.diagnostics.is_empty(),
                "{kind}: {:?}",
                auto.diagnostics
            );
            assert!(
                native.diagnostics.is_empty(),
                "{kind}: {:?}",
                native.diagnostics
            );
            assert_eq!(auto.backend, DataBackend::NativePolars, "{kind}");
            assert_eq!(native.backend, DataBackend::NativePolars, "{kind}");
            let row_csv = String::from_utf8(row.stdout.expect("row csv")).expect("utf8");
            assert_eq!(row_csv, expected, "{kind}");
            assert_eq!(
                String::from_utf8(auto.stdout.expect("auto csv")).expect("utf8"),
                row_csv,
                "{kind}"
            );
            assert_eq!(
                String::from_utf8(native.stdout.expect("native csv")).expect("utf8"),
                row_csv,
                "{kind}"
            );
        }

        fs::remove_dir_all(workspace).expect("clean temp workspace");
    }

    #[test]
    fn native_engine_right_and_full_join_match_rows_for_binding_input() {
        let workspace = temp_workspace("native-right-full-join");
        fs::write(
            workspace.join("left.csv"),
            "id,left_value,shared\nB,L-B,LS-B\nD,L-D,LS-D\n",
        )
        .expect("write left");
        fs::write(
            workspace.join("right.csv"),
            "right_id,right_value,shared\nC,R-C,RS-C\nA,R-A,RS-A\nB,R-B,RS-B\n,R-null,RS-null\n",
        )
        .expect("write right");

        for (kind, expected) in [
            (
                "right",
                "id,left_value,shared,right_value,shared_right\nC,,,R-C,RS-C\nA,,,R-A,RS-A\nB,L-B,LS-B,R-B,RS-B\n,,,R-null,RS-null\n",
            ),
            (
                "full",
                "id,left_value,shared,right_value,shared_right\nB,L-B,LS-B,R-B,RS-B\nD,L-D,LS-D,,\n,,,R-null,RS-null\nA,,,R-A,RS-A\nC,,,R-C,RS-C\n",
            ),
        ] {
            let io = OsDriverIo;
            let prepared = prepare_source_with_io(
                workspace.join(format!("{kind}.pdl")),
                format!(
                    r#"let right_side =
  load "right.csv"

load "left.csv"
  | join right_side on (id, right_id) kind {kind}"#
                ),
                &io,
            );
            let options = RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            };
            let row = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options.clone(),
                &io,
                BTreeMap::new(),
                ExecutionEngine::Row,
            );
            let auto = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options.clone(),
                &io,
                BTreeMap::new(),
                ExecutionEngine::Auto,
            );
            let native = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options,
                &io,
                BTreeMap::new(),
                ExecutionEngine::Native,
            );

            assert!(row.diagnostics.is_empty(), "{kind}: {:?}", row.diagnostics);
            assert!(
                auto.diagnostics.is_empty(),
                "{kind}: {:?}",
                auto.diagnostics
            );
            assert!(
                native.diagnostics.is_empty(),
                "{kind}: {:?}",
                native.diagnostics
            );
            assert_eq!(auto.backend, DataBackend::NativePolars, "{kind}");
            assert_eq!(native.backend, DataBackend::NativePolars, "{kind}");
            let row_csv = String::from_utf8(row.stdout.expect("row csv")).expect("utf8");
            assert_eq!(row_csv, expected, "{kind}");
            assert_eq!(
                String::from_utf8(auto.stdout.expect("auto csv")).expect("utf8"),
                row_csv,
                "{kind}"
            );
            assert_eq!(
                String::from_utf8(native.stdout.expect("native csv")).expect("utf8"),
                row_csv,
                "{kind}"
            );
        }

        fs::remove_dir_all(workspace).expect("clean temp workspace");
    }

    #[test]
    fn native_engine_semi_and_anti_join_match_rows_for_binding_input() {
        let workspace = temp_workspace("native-semi-anti-join");
        fs::write(
            workspace.join("sales.csv"),
            "customer_id,amount\nc1,30\nc2,10\nc3,5\n,99\n",
        )
        .expect("write sales");
        fs::write(
            workspace.join("customers.csv"),
            "customer_id,segment\nc1,enterprise\nc2,self-serve\n,unknown\n",
        )
        .expect("write customers");
        for (kind, expected) in [
            ("semi", "customer_id,amount\nc1,30\nc2,10\n"),
            ("anti", "customer_id,amount\nc3,5\n,99\n"),
        ] {
            let io = OsDriverIo;
            let prepared = prepare_source_with_io(
                workspace.join(format!("{kind}.pdl")),
                format!(
                    r#"let customers =
  load "customers.csv"

load "sales.csv"
  | join customers on customer_id kind {kind}
  | sort customer_id, amount"#
                ),
                &io,
            );
            let options = RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            };
            let row = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options.clone(),
                &io,
                BTreeMap::new(),
                ExecutionEngine::Row,
            );
            let auto = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options.clone(),
                &io,
                BTreeMap::new(),
                ExecutionEngine::Auto,
            );
            let native = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options,
                &io,
                BTreeMap::new(),
                ExecutionEngine::Native,
            );

            assert!(row.diagnostics.is_empty(), "{kind}: {:?}", row.diagnostics);
            assert!(
                auto.diagnostics.is_empty(),
                "{kind}: {:?}",
                auto.diagnostics
            );
            assert!(
                native.diagnostics.is_empty(),
                "{kind}: {:?}",
                native.diagnostics
            );
            assert_eq!(auto.backend, DataBackend::NativePolars, "{kind}");
            assert_eq!(native.backend, DataBackend::NativePolars, "{kind}");
            assert_eq!(
                String::from_utf8(row.stdout.expect("row csv")).expect("utf8"),
                expected,
                "{kind}"
            );
            assert_eq!(
                String::from_utf8(auto.stdout.expect("auto csv")).expect("utf8"),
                expected,
                "{kind}"
            );
            assert_eq!(
                String::from_utf8(native.stdout.expect("native csv")).expect("utf8"),
                expected,
                "{kind}"
            );
        }
        fs::remove_dir_all(workspace).expect("clean temp workspace");
    }

    #[test]
    fn native_engine_window_subset_matches_rows_for_path_input() {
        let workspace = temp_workspace("native-window-subset");
        fs::write(
            workspace.join("orders.csv"),
            "order_id,region,amount\nA,West,30\nB,West,\nC,West,10\nD,East,20\nE,East,20\nF,,5\n",
        )
        .expect("write orders");
        let io = OsDriverIo;
        let prepared = prepare_source_with_io(
            workspace.join("main.pdl"),
            r#"load "orders.csv"
  | mutate
      seq = row_number() over (partition_by region order_by amount asc nulls_first),
      amount_rank = rank() over (partition_by region order_by amount asc nulls_first),
      amount_dense = dense_rank() over (partition_by region order_by amount asc nulls_first),
      region_rows = count() over (partition_by region),
      known_amounts = count(amount) over (partition_by region),
      amount_total = sum(amount) over (partition_by region),
      amount_mean = mean(amount) over (partition_by region),
      amount_min = min(amount) over (partition_by region),
      amount_max = max(amount) over (partition_by region)
  | select order_id, region, amount, seq, amount_rank, amount_dense, region_rows, known_amounts, amount_total, amount_mean, amount_min, amount_max
  | sort order_id"#,
            &io,
        );
        let options = RunOptions {
            stdout_format: Some("csv".to_string()),
            dry_run: false,
            allow_binary_stdout: true,
        };
        let row = run_prepared_with_io_and_context_and_engine(
            &prepared,
            options.clone(),
            &io,
            BTreeMap::new(),
            ExecutionEngine::Row,
        );
        let auto = run_prepared_with_io_and_context_and_engine(
            &prepared,
            options.clone(),
            &io,
            BTreeMap::new(),
            ExecutionEngine::Auto,
        );
        let native = run_prepared_with_io_and_context_and_engine(
            &prepared,
            options,
            &io,
            BTreeMap::new(),
            ExecutionEngine::Native,
        );

        assert!(row.diagnostics.is_empty(), "{:?}", row.diagnostics);
        assert!(auto.diagnostics.is_empty(), "{:?}", auto.diagnostics);
        assert!(native.diagnostics.is_empty(), "{:?}", native.diagnostics);
        assert_eq!(auto.backend, DataBackend::NativePolars);
        assert_eq!(native.backend, DataBackend::NativePolars);
        let row_csv = String::from_utf8(row.stdout.expect("row csv")).expect("utf8");
        assert_eq!(
            String::from_utf8(auto.stdout.expect("auto csv")).expect("utf8"),
            row_csv
        );
        assert_eq!(
            String::from_utf8(native.stdout.expect("native csv")).expect("utf8"),
            row_csv
        );
        assert_eq!(
            row_csv,
            "order_id,region,amount,seq,amount_rank,amount_dense,region_rows,known_amounts,amount_total,amount_mean,amount_min,amount_max\nA,West,30,3,3,3,3,2,40,20,10,30\nB,West,,1,1,1,3,2,40,20,10,30\nC,West,10,2,2,2,3,2,40,20,10,30\nD,East,20,1,1,1,2,2,40,20,20,20\nE,East,20,2,1,1,2,2,40,20,20,20\nF,,5,1,1,1,1,1,5,5,5,5\n"
        );

        fs::remove_dir_all(workspace).expect("clean temp workspace");
    }

    #[test]
    fn native_engine_advanced_windows_match_rows_for_path_formats() {
        let workspace = temp_workspace("native-advanced-windows");
        let table = pdl_data::read_table_from_bytes(
            Path::new("orders.csv"),
            DataFormat::Csv,
            b"order_id,customer_id,region,order_date,amount\nA0,C1,North,2026-01-31,\nA1,C1,North,2026-02-01,10\nA2,C1,North,2026-02-03,25\nA3,C2,North,2026-02-02,15\nA4,C2,South,2026-02-01,40\nA5,C1,North,2026-02-04,5\nA6,C3,North,2026-02-05,25\n",
        )
        .expect("orders table");
        for (path, format) in [
            ("orders.csv", DataFormat::Csv),
            ("orders.parquet", DataFormat::Parquet),
            ("orders.arrow", DataFormat::ArrowFile),
            ("orders.arrow-stream", DataFormat::ArrowStream),
        ] {
            pdl_data::write_table_to_path(&workspace.join(path), format, &table)
                .expect("write window fixture");
        }

        for (input, format_clause) in [
            ("orders.csv", ""),
            ("orders.parquet", ""),
            ("orders.arrow", ""),
            ("orders.arrow-stream", r#" format "arrow-stream""#),
        ] {
            let io = OsDriverIo;
            let prepared = prepare_source_with_io(
                workspace.join(format!("{input}.pdl")),
                format!(
                    r#"load "{input}"{format_clause}
  | mutate
      region_count = count() over (partition_by region),
      customer_running_count = count(amount) over (partition_by customer_id order_by order_date frame running),
      customer_running_amount = sum(amount) over (partition_by customer_id order_by order_date frame running),
      customer_running_mean = mean(amount) over (partition_by customer_id order_by order_date frame running),
      customer_running_min = min(amount) over (partition_by customer_id order_by order_date frame running),
      customer_running_max = max(amount) over (partition_by customer_id order_by order_date frame running),
      previous_amount = lag(amount) over (partition_by customer_id order_by order_date),
      next_amount = lead(amount, 1, null) over (partition_by customer_id order_by order_date),
      region_top_order = first_value(order_id) over (partition_by region order_by amount desc),
      region_low_order = last_value(order_id) over (partition_by region order_by amount desc frame whole_partition),
      current_customer_order = last_value(order_id) over (partition_by customer_id order_by order_date frame running),
      region_percent_rank = percent_rank() over (partition_by region order_by amount desc),
      region_cume_dist = cume_dist() over (partition_by region order_by amount desc)
  | select order_id, region_count, customer_running_count, customer_running_amount, customer_running_mean, customer_running_min, customer_running_max, previous_amount, next_amount, region_top_order, region_low_order, current_customer_order, region_percent_rank, region_cume_dist
  | sort order_id"#
                ),
                &io,
            );
            let options = RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            };
            let row = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options.clone(),
                &io,
                BTreeMap::new(),
                ExecutionEngine::Row,
            );
            let auto = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options.clone(),
                &io,
                BTreeMap::new(),
                ExecutionEngine::Auto,
            );
            let native = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options,
                &io,
                BTreeMap::new(),
                ExecutionEngine::Native,
            );

            assert!(row.diagnostics.is_empty(), "{input}: {:?}", row.diagnostics);
            assert!(
                auto.diagnostics.is_empty(),
                "{input}: {:?}",
                auto.diagnostics
            );
            assert!(
                native.diagnostics.is_empty(),
                "{input}: {:?}",
                native.diagnostics
            );
            assert_eq!(auto.backend, DataBackend::NativePolars, "{input}");
            assert_eq!(native.backend, DataBackend::NativePolars, "{input}");
            let row_csv = String::from_utf8(row.stdout.expect("row csv")).expect("utf8");
            assert_eq!(
                String::from_utf8(auto.stdout.expect("auto csv")).expect("utf8"),
                row_csv,
                "{input}"
            );
            assert_eq!(
                String::from_utf8(native.stdout.expect("native csv")).expect("utf8"),
                row_csv,
                "{input}"
            );
        }

        fs::remove_dir_all(workspace).expect("clean temp workspace");
    }

    #[test]
    fn native_engine_multi_key_windows_match_rows_for_path_formats() {
        let workspace = temp_workspace("native-multi-key-windows");
        let table = pdl_data::read_table_from_bytes(
            Path::new("orders.csv"),
            DataFormat::Csv,
            b"order_id,region,amount,tie,note\nA,West,30,b,alpha\nB,West,30,a,beta\nC,West,10,a,gamma\nD,East,20,b,delta\nE,East,20,a,epsilon\nF,East,5,a,zeta\nG,West,30,a,eta\n",
        )
        .expect("orders table");
        for (path, format) in [
            ("orders.csv", DataFormat::Csv),
            ("orders.parquet", DataFormat::Parquet),
            ("orders.arrow", DataFormat::ArrowStream),
        ] {
            pdl_data::write_table_to_path(&workspace.join(path), format, &table)
                .expect("write multi-key window fixture");
        }

        for (input, format_clause) in [
            ("orders.csv", ""),
            ("orders.parquet", ""),
            ("orders.arrow", r#" format "arrow-stream""#),
        ] {
            let io = OsDriverIo;
            let prepared = prepare_source_with_io(
                workspace.join(format!("{input}.pdl")),
                format!(
                    r#"load "{input}"{format_clause}
  | mutate
      seq = row_number() over (partition_by region order_by amount desc, tie asc),
      sparse_rank = rank() over (partition_by region order_by amount desc, tie asc),
      dense = dense_rank() over (partition_by region order_by amount desc, tie asc),
      pct = percent_rank() over (partition_by region order_by amount desc, tie asc),
      cume = cume_dist() over (partition_by region order_by amount desc, tie asc),
      prior_amount = lag(amount, 1, 0) over (partition_by region order_by amount desc, tie asc),
      next_note = lead(note, 1, "end") over (partition_by region order_by amount desc, tie asc),
      running_amount = sum(amount) over (partition_by region order_by amount desc, tie asc frame running),
      tie_seq = row_number() over (partition_by region order_by tie asc, amount desc),
      global_seq = row_number() over (order_by tie asc, region asc)
  | select order_id, seq, sparse_rank, dense, pct, cume, prior_amount, next_note, running_amount, tie_seq, global_seq"#
                ),
                &io,
            );
            let options = RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            };
            let row = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options.clone(),
                &io,
                BTreeMap::new(),
                ExecutionEngine::Row,
            );
            let auto = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options.clone(),
                &io,
                BTreeMap::new(),
                ExecutionEngine::Auto,
            );
            let native = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options,
                &io,
                BTreeMap::new(),
                ExecutionEngine::Native,
            );

            assert!(row.diagnostics.is_empty(), "{input}: {:?}", row.diagnostics);
            assert!(
                auto.diagnostics.is_empty(),
                "{input}: {:?}",
                auto.diagnostics
            );
            assert!(
                native.diagnostics.is_empty(),
                "{input}: {:?}",
                native.diagnostics
            );
            assert_eq!(auto.backend, DataBackend::NativePolars, "{input}");
            assert_eq!(native.backend, DataBackend::NativePolars, "{input}");
            let row_csv = String::from_utf8(row.stdout.expect("row csv")).expect("utf8");
            assert_eq!(
                String::from_utf8(auto.stdout.expect("auto csv")).expect("utf8"),
                row_csv,
                "{input}"
            );
            assert_eq!(
                String::from_utf8(native.stdout.expect("native csv")).expect("utf8"),
                row_csv,
                "{input}"
            );
            assert_eq!(
                row_csv,
                "order_id,seq,sparse_rank,dense,pct,cume,prior_amount,next_note,running_amount,tie_seq,global_seq\nA,3,3,2,0.6666666666666666,0.75,30,gamma,90,4,7\nB,1,1,1,0,0.5,0,eta,30,1,3\nC,4,4,3,1,1,30,end,100,3,4\nD,2,2,2,0.5,0.6666666666666666,20,zeta,40,3,6\nE,1,1,1,0,0.3333333333333333,0,delta,20,1,1\nF,3,3,3,1,1,20,end,45,2,2\nG,2,1,1,0,0.5,30,alpha,60,2,5\n"
            );
        }

        fs::remove_dir_all(workspace).expect("clean temp workspace");
    }

    #[test]
    fn native_engine_accelerates_chained_window_mutates() {
        let workspace = temp_workspace("native-chained-window-mutates");
        fs::write(
            workspace.join("sales.csv"),
            "sale_id,status,region,customer_id,amount\nS1,completed,West,C1,30\nS2,pending,West,C1,100\nS3,completed,West,C1,50\nS4,completed,East,C2,40\nS5,completed,East,C3,20\nS6,completed,North,C4,80\n",
        )
        .expect("write sales");
        let io = OsDriverIo;
        let prepared = prepare_source_with_io(
            workspace.join("main.pdl"),
            r#"load "sales.csv"
  | filter status == "completed"
  | mutate
      customer_sale_number =
        row_number() over (
          partition_by customer_id
          order_by amount desc
        ),
      customer_revenue =
        sum(amount) over (
          partition_by customer_id
        ),
      region_revenue =
        sum(amount) over (
          partition_by region
        )
  | mutate
      region_revenue_rank =
        dense_rank() over (
          order_by region_revenue desc
        )
  | select
      region,
      customer_id,
      amount,
      customer_sale_number,
      customer_revenue,
      region_revenue_rank
  | sort region_revenue_rank, customer_id, amount desc"#,
            &io,
        );
        let options = RunOptions {
            stdout_format: Some("csv".to_string()),
            dry_run: false,
            allow_binary_stdout: true,
        };
        let row = run_prepared_with_io_and_context_and_engine(
            &prepared,
            options.clone(),
            &io,
            BTreeMap::new(),
            ExecutionEngine::Row,
        );
        let auto = run_prepared_with_io_and_context_and_engine(
            &prepared,
            options.clone(),
            &io,
            BTreeMap::new(),
            ExecutionEngine::Auto,
        );
        let native = run_prepared_with_io_and_context_and_engine(
            &prepared,
            options,
            &io,
            BTreeMap::new(),
            ExecutionEngine::Native,
        );

        assert!(row.diagnostics.is_empty(), "{:?}", row.diagnostics);
        assert!(auto.diagnostics.is_empty(), "{:?}", auto.diagnostics);
        assert!(native.diagnostics.is_empty(), "{:?}", native.diagnostics);
        assert_eq!(auto.backend, DataBackend::NativePolars);
        assert_eq!(native.backend, DataBackend::NativePolars);
        let row_csv = String::from_utf8(row.stdout.expect("row csv")).expect("utf8");
        assert_eq!(
            String::from_utf8(auto.stdout.expect("auto csv")).expect("utf8"),
            row_csv
        );
        assert_eq!(
            String::from_utf8(native.stdout.expect("native csv")).expect("utf8"),
            row_csv
        );
        assert_eq!(
            row_csv,
            "region,customer_id,amount,customer_sale_number,customer_revenue,region_revenue_rank\nWest,C1,50,1,80,1\nWest,C1,30,2,80,1\nNorth,C4,80,1,80,1\nEast,C2,40,1,40,2\nEast,C3,20,1,20,2\n"
        );

        fs::remove_dir_all(workspace).expect("clean temp workspace");
    }

    #[test]
    fn native_engine_union_by_name_distinct_matches_rows_for_binding_input() {
        let workspace = temp_workspace("native-union");
        fs::write(
            workspace.join("day1.csv"),
            "order_id,region,amount\n1,West,30\n2,East,10\n",
        )
        .expect("write day1");
        fs::write(
            workspace.join("day2.csv"),
            "amount,order_id,region\n10,2,East\n40,3,North\n",
        )
        .expect("write day2");
        let io = OsDriverIo;
        let prepared = prepare_source_with_io(
            workspace.join("main.pdl"),
            r#"let day2 =
  load "day2.csv"

load "day1.csv"
  | union day2 by_name true distinct true
  | sort order_id"#,
            &io,
        );
        let options = RunOptions {
            stdout_format: Some("csv".to_string()),
            dry_run: false,
            allow_binary_stdout: true,
        };
        let row = run_prepared_with_io_and_context_and_engine(
            &prepared,
            options.clone(),
            &io,
            BTreeMap::new(),
            ExecutionEngine::Row,
        );
        let auto = run_prepared_with_io_and_context_and_engine(
            &prepared,
            options.clone(),
            &io,
            BTreeMap::new(),
            ExecutionEngine::Auto,
        );
        let native = run_prepared_with_io_and_context_and_engine(
            &prepared,
            options,
            &io,
            BTreeMap::new(),
            ExecutionEngine::Native,
        );

        assert!(row.diagnostics.is_empty(), "{:?}", row.diagnostics);
        assert!(auto.diagnostics.is_empty(), "{:?}", auto.diagnostics);
        assert!(native.diagnostics.is_empty(), "{:?}", native.diagnostics);
        assert_eq!(auto.backend, DataBackend::NativePolars);
        assert_eq!(native.backend, DataBackend::NativePolars);
        let row_csv = String::from_utf8(row.stdout.expect("row csv")).expect("utf8");
        assert_eq!(
            String::from_utf8(auto.stdout.expect("auto csv")).expect("utf8"),
            row_csv
        );
        assert_eq!(
            String::from_utf8(native.stdout.expect("native csv")).expect("utf8"),
            row_csv
        );
        assert_eq!(
            row_csv,
            "order_id,region,amount\n1,West,30\n2,East,10\n3,North,40\n"
        );
        fs::remove_dir_all(workspace).expect("clean temp workspace");
    }

    #[test]
    fn native_engine_runs_grouped_aggregate_csv_parquet_and_arrow_stream() {
        let workspace = temp_workspace("native-aggregate");
        let csv = "region,score,latency_ms,category\nWest,30,100,alpha\nEast,10,80,beta\nWest,50,120,alpha\nEast,30,90,\n";
        fs::write(workspace.join("sales.csv"), csv).expect("write csv");
        let table = Table::new(
            vec![
                "region".to_string(),
                "score".to_string(),
                "latency_ms".to_string(),
                "category".to_string(),
            ],
            vec![
                Row {
                    values: vec![
                        Value::String("West".to_string()),
                        Value::Number(30.0),
                        Value::Number(100.0),
                        Value::String("alpha".to_string()),
                    ],
                },
                Row {
                    values: vec![
                        Value::String("East".to_string()),
                        Value::Number(10.0),
                        Value::Number(80.0),
                        Value::String("beta".to_string()),
                    ],
                },
                Row {
                    values: vec![
                        Value::String("West".to_string()),
                        Value::Number(50.0),
                        Value::Number(120.0),
                        Value::String("alpha".to_string()),
                    ],
                },
                Row {
                    values: vec![
                        Value::String("East".to_string()),
                        Value::Number(30.0),
                        Value::Number(90.0),
                        Value::Null,
                    ],
                },
            ],
        );
        pdl_data::write_table_to_path(
            &workspace.join("sales.parquet"),
            DataFormat::Parquet,
            &table,
        )
        .expect("write parquet");
        pdl_data::write_table_to_path(
            &workspace.join("sales.arrow"),
            DataFormat::ArrowStream,
            &table,
        )
        .expect("write arrow stream");

        for (input, format_clause) in [
            ("sales.csv", ""),
            ("sales.parquet", ""),
            ("sales.arrow", r#" format "arrow-stream""#),
        ] {
            let program_path = workspace.join(format!("{input}.pdl"));
            let io = OsDriverIo;
            let prepared = prepare_source_with_io(
                &program_path,
                format!(
                    r#"load "{input}"{format_clause}
  | group_by region
  | agg
      row_count = count(),
      total_score = sum(score),
      total_weighted_score = sum(score * 2),
      avg_score = mean(score),
      avg_score_plus_ten = mean(score + 10),
      min_latency_ms = min(latency_ms),
      min_latency_minus_score = min(latency_ms - score),
      max_latency_ms = max(latency_ms),
      max_latency_plus_score = max(latency_ms + score),
      categories = count_distinct(category),
      latency_buckets = count_distinct(round(latency_ms / 10, 0))
  | sort region"#
                ),
                &io,
            );
            let options = RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            };
            let row = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options.clone(),
                &io,
                BTreeMap::new(),
                ExecutionEngine::Row,
            );
            let auto = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options.clone(),
                &io,
                BTreeMap::new(),
                ExecutionEngine::Auto,
            );
            let native = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options,
                &io,
                BTreeMap::new(),
                ExecutionEngine::Native,
            );

            assert!(row.diagnostics.is_empty(), "{:?}", row.diagnostics);
            assert!(
                native.diagnostics.is_empty(),
                "{input}: {:?}",
                native.diagnostics
            );
            assert!(
                auto.diagnostics.is_empty(),
                "{input}: {:?}",
                auto.diagnostics
            );
            assert_eq!(auto.backend, DataBackend::NativePolars, "{input}");
            assert_eq!(native.backend, DataBackend::NativePolars);
            let row_csv = String::from_utf8(row.stdout.expect("row csv")).expect("utf8");
            assert_eq!(
                String::from_utf8(native.stdout.expect("native csv")).expect("utf8"),
                row_csv,
                "{input}"
            );
            assert_eq!(
                String::from_utf8(auto.stdout.expect("auto csv")).expect("utf8"),
                row_csv,
                "{input}"
            );
        }
        fs::remove_dir_all(workspace).expect("clean temp workspace");
    }

    #[test]
    fn native_engine_writes_readable_arrow_stream_stdout() {
        let workspace = temp_workspace("native-arrow-stream");
        fs::write(
            workspace.join("sales.csv"),
            "region,amount\nWest,30\nEast,10\n",
        )
        .expect("write csv");
        let program_path = workspace.join("main.pdl");
        let io = OsDriverIo;
        let prepared = prepare_source_with_io(
            &program_path,
            r#"load "sales.csv"
  | select region, amount
  | sort amount desc"#,
            &io,
        );

        let result = run_prepared_with_io_and_context_and_engine(
            &prepared,
            RunOptions {
                stdout_format: Some("arrow-stream".to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            },
            &io,
            BTreeMap::new(),
            ExecutionEngine::Native,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(result.backend, DataBackend::NativePolars);
        let stdout = result.stdout.expect("arrow stdout");
        assert!(stdout.starts_with(&[0xff, 0xff, 0xff, 0xff]));
        assert_eq!(
            pdl_data::read_table_from_bytes(
                Path::new("stdout.arrow"),
                DataFormat::ArrowStream,
                &stdout,
            )
            .expect("read arrow stdout"),
            Table::new(
                vec!["region".to_string(), "amount".to_string()],
                vec![
                    Row {
                        values: vec![Value::String("West".to_string()), Value::Number(30.0)],
                    },
                    Row {
                        values: vec![Value::String("East".to_string()), Value::Number(10.0)],
                    },
                ],
            )
        );
        fs::remove_dir_all(workspace).expect("clean temp workspace");
    }

    #[test]
    fn native_engine_supports_terminal_save_stdout_arrow_stream() {
        let workspace = temp_workspace("native-save-stdout");
        fs::write(
            workspace.join("sales.csv"),
            "region,amount\nWest,30\nEast,10\n",
        )
        .expect("write csv");
        let program_path = workspace.join("main.pdl");
        let io = OsDriverIo;
        let prepared = prepare_source_with_io(
            &program_path,
            r#"load "sales.csv"
  | select region, amount
  | sort amount desc
  | save stdout format "arrow-stream""#,
            &io,
        );

        let result = run_prepared_with_io_and_context_and_engine(
            &prepared,
            RunOptions {
                stdout_format: None,
                dry_run: false,
                allow_binary_stdout: true,
            },
            &io,
            BTreeMap::new(),
            ExecutionEngine::Native,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(result.backend, DataBackend::NativePolars);
        let stdout = result.stdout.expect("arrow stdout");
        assert!(stdout.starts_with(&[0xff, 0xff, 0xff, 0xff]));
        assert_eq!(
            pdl_data::read_table_from_bytes(
                Path::new("stdout.arrow"),
                DataFormat::ArrowStream,
                &stdout,
            )
            .expect("read arrow stdout")
            .columns,
            vec!["region", "amount"]
        );
        fs::remove_dir_all(workspace).expect("clean temp workspace");
    }

    #[test]
    fn native_engine_writes_parquet_and_arrow_file_sinks_after_mutate() {
        let workspace = temp_workspace("native-file-sinks");
        fs::write(
            workspace.join("sales.csv"),
            "region,amount\nWest,30\nEast,10\n",
        )
        .expect("write csv");

        for (output_name, format_name, data_format) in [
            ("out.parquet", "parquet", DataFormat::Parquet),
            ("out.arrow", "arrow-file", DataFormat::ArrowFile),
        ] {
            let output_path = workspace.join(output_name);
            let program_path = workspace.join(format!("{output_name}.pdl"));
            let io = OsDriverIo;
            let prepared = prepare_source_with_io(
                &program_path,
                format!(
                    r#"load "sales.csv"
  | mutate doubled = amount * 2
  | select region, doubled
  | sort region
  | save "{}" format "{format_name}""#,
                    output_path.display()
                ),
                &io,
            );

            let result = run_prepared_with_io_and_context_and_engine(
                &prepared,
                RunOptions {
                    stdout_format: None,
                    dry_run: false,
                    allow_binary_stdout: true,
                },
                &io,
                BTreeMap::new(),
                ExecutionEngine::Native,
            );

            assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
            assert_eq!(result.backend, DataBackend::NativePolars);
            let bytes = fs::read(&output_path).expect("read native sink");
            assert_eq!(
                pdl_data::read_table_from_bytes(&output_path, data_format, &bytes)
                    .expect("read native sink table"),
                Table::new(
                    vec!["region".to_string(), "doubled".to_string()],
                    vec![
                        Row {
                            values: vec![Value::String("East".to_string()), Value::Number(20.0)],
                        },
                        Row {
                            values: vec![Value::String("West".to_string()), Value::Number(60.0)],
                        },
                    ],
                ),
                "{format_name}"
            );
        }
        fs::remove_dir_all(workspace).expect("clean temp workspace");
    }

    #[test]
    fn reactive_context_defaults_and_overrides_drive_named_outputs() {
        let io = InMemoryDriverIo::default().with_file_bytes(
            "memory/trips.csv",
            "zone,station,fleet,revenue,duration_min\nDowntown,A,bus,100,12\nDowntown,B,rail,50,30\nRiverfront,C,bus,80,20\nRiverfront,D,rail,120,40\n",
        );
        let source = r#"param active_fleet = "all"
state selected_zone = "Downtown"
param metric_column = "revenue"

let trips =
  load "trips.csv"
  | filter $active_fleet == "all" or fleet == $active_fleet

output zone_summary =
  trips
  | group_by zone
  | agg total_revenue = sum(revenue)
  | save "zone_summary.csv"

output active_rankings =
  trips
  | filter zone == @selected_zone
  | group_by station
  | agg total = sum(col($metric_column))
  | sort total desc
  | save "active_rankings.csv""#;
        let prepared = prepare_source_with_io("memory/main.pdl", source, &io);

        let run_options = RunOptions {
            dry_run: true,
            ..RunOptions::default()
        };
        let defaults = run_prepared_with_io(&prepared, run_options.clone(), &io);
        assert!(
            defaults.diagnostics.is_empty(),
            "{:?}",
            defaults.diagnostics
        );
        assert_eq!(
            named_output_csv(&defaults, "active_rankings"),
            "station,total\nA,100\nB,50\n"
        );

        let mut context = BTreeMap::new();
        context.insert("active_fleet".to_string(), Value::String("bus".to_string()));
        context.insert(
            "selected_zone".to_string(),
            Value::String("Riverfront".to_string()),
        );
        context.insert(
            "metric_column".to_string(),
            Value::String("duration_min".to_string()),
        );
        let overridden = run_prepared_with_io_and_context(&prepared, run_options, &io, context);
        assert!(
            overridden.diagnostics.is_empty(),
            "{:?}",
            overridden.diagnostics
        );
        assert_eq!(
            named_output_csv(&overridden, "active_rankings"),
            "station,total\nC,20\n"
        );
    }

    #[test]
    fn sniffs_arrow_stream_stdin_and_preserves_bytes_for_execution() {
        let input_table = Table::new(
            vec!["region".to_string(), "amount".to_string()],
            vec![
                Row {
                    values: vec![Value::String("West".to_string()), Value::Number(30.0)],
                },
                Row {
                    values: vec![Value::String("East".to_string()), Value::Number(10.0)],
                },
            ],
        );
        let stdin = pdl_data::write_table_to_bytes(DataFormat::ArrowStream, &input_table)
            .expect("arrow stdin");
        let io = InMemoryDriverIo::default().with_stdin_bytes(stdin);
        let prepared = prepare_source_for_run_with_io(
            "memory/main.pdl",
            r#"load stdin
  | sort amount desc"#,
            None,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(result.backend, DataBackend::NativePolars);
        assert_eq!(
            String::from_utf8(result.stdout.expect("csv stdout")).expect("utf8 csv"),
            "region,amount\nWest,30\nEast,10\n"
        );
    }

    #[test]
    fn stdin_format_conflict_reports_e1217_before_reading_stdin() {
        let io = InMemoryDriverIo::default();
        let prepared = prepare_source_for_run_with_io(
            "memory/main.pdl",
            r#"load stdin format "csv""#,
            Some("arrow-stream".to_string()),
            &io,
        );
        let diagnostics = prepared.diagnostics();

        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "E1217"),
            "{diagnostics:?}"
        );
        assert!(
            !diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "E1806"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn emits_deterministic_arrow_stream_stdout() {
        let io = InMemoryDriverIo::default()
            .with_file_bytes("memory/sales.csv", "region,amount\nWest,30\nEast,10\n");
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "sales.csv"
  | sort amount desc"#,
            &io,
        );

        let first = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("arrow-stream".to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            },
            &io,
        );
        let second = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("arrow-stream".to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(first.diagnostics.is_empty(), "{:?}", first.diagnostics);
        assert!(second.diagnostics.is_empty(), "{:?}", second.diagnostics);
        let first_stdout = first.stdout.expect("arrow stdout");
        let second_stdout = second.stdout.expect("arrow stdout");
        assert_eq!(first_stdout, second_stdout);
        assert!(first_stdout.starts_with(&[0xff, 0xff, 0xff, 0xff]));
        assert_eq!(
            pdl_data::read_table_from_bytes(
                Path::new("stdout.arrow"),
                DataFormat::ArrowStream,
                &first_stdout,
            )
            .expect("read arrow stdout"),
            Table::new(
                vec!["region".to_string(), "amount".to_string()],
                vec![
                    Row {
                        values: vec![Value::String("West".to_string()), Value::Number(30.0)],
                    },
                    Row {
                        values: vec![Value::String("East".to_string()), Value::Number(10.0)],
                    },
                ],
            )
        );
    }

    #[test]
    fn save_stdout_writes_arrow_stream_bytes() {
        let io = InMemoryDriverIo::default()
            .with_file_bytes("memory/sales.csv", "region,amount\nWest,30\n");
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "sales.csv"
  | save stdout format "arrow-stream""#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: None,
                dry_run: false,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        let stdout = result.stdout.expect("arrow stdout");
        assert!(stdout.starts_with(&[0xff, 0xff, 0xff, 0xff]));
    }

    #[test]
    fn executes_mutate_distinct_and_scalar_functions() {
        let io = InMemoryDriverIo::default().with_file_bytes(
            "memory/orders.csv",
            "order_id,region,channel,gross,discount,status\nA1,North,web,120,20,completed\nA1,North,web,120,20,completed\nA2,South,store,80,5,pending\nA3,West,Web,200,50,completed\n",
        );
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "orders.csv"
  | filter status == "completed"
  | mutate net_amount = gross - discount, region_channel = concat(upper(region), ":", lower(channel)), priority = if_else(gross >= 150, "high", "standard")
  | distinct order_id
  | select order_id, region_channel, net_amount, priority
  | sort order_id"#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            String::from_utf8(result.stdout.expect("csv stdout")).expect("utf8 csv"),
            "order_id,region_channel,net_amount,priority\nA1,NORTH:web,100,standard\nA3,WEST:web,150,high\n"
        );
    }

    #[test]
    fn executes_decimal_rounding_and_count_distinct() {
        let io = InMemoryDriverIo::default().with_file_bytes(
            "memory/events.csv",
            "group,user,amount\nA,u1,1.234\nA,u1,2.345\nA,u2,-0.004\nA,,4.0\nB,u3,10.005\n",
        );
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "events.csv"
  | group_by `group`
  | agg users = count_distinct(user), total = sum(amount)
  | mutate rounded = round(total, 2), nearest = round(total)
  | sort `group`"#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            String::from_utf8(result.stdout.expect("csv stdout")).expect("utf8 csv"),
            "group,users,total,rounded,nearest\nA,2,7.575,7.58,8\nB,1,10.005,10.01,10\n"
        );
    }

    #[test]
    fn round_propagates_null_and_normalizes_negative_zero() {
        let io = InMemoryDriverIo::default()
            .with_file_bytes("memory/values.csv", "id,value\nnegative,-0.004\nempty,\n");
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "values.csv"
  | mutate rounded = round(value, 2)
  | sort id"#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            String::from_utf8(result.stdout.expect("csv stdout")).expect("utf8 csv"),
            "id,value,rounded\nempty,,\nnegative,-0.004,0\n"
        );
    }

    #[test]
    fn invalid_round_digits_are_semantic_diagnostics() {
        let io = InMemoryDriverIo::default().with_file_bytes("memory/values.csv", "value\n1.234\n");
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "values.csv"
  | mutate rounded = round(value, 13)"#,
            &io,
        );
        let diagnostics = prepared.diagnostics();

        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "E1206"
                    && diagnostic.message.contains("round() digits")),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn executes_pivot_longer_with_stable_order() {
        let io = InMemoryDriverIo::default().with_file_bytes(
            "memory/wide.csv",
            "rider_type,Share of rides,Share of revenue\nmember,65.96,39.01\nvisitor,34.04,60.99\n",
        );
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "wide.csv"
  | pivot_longer `Share of rides`, `Share of revenue` names_to metric values_to share"#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            String::from_utf8(result.stdout.expect("csv stdout")).expect("utf8 csv"),
            "rider_type,metric,share\nmember,Share of rides,65.96\nmember,Share of revenue,39.01\nvisitor,Share of rides,34.04\nvisitor,Share of revenue,60.99\n"
        );
    }

    #[test]
    fn executes_complete_with_deterministic_fill_rows() {
        let io = InMemoryDriverIo::default().with_file_bytes(
            "memory/daily.csv",
            "trip_date,rider_type,trips,revenue\n2026-04-01,member,3,13.85\n2026-04-01,visitor,2,33.13\n2026-04-03,member,2,8.35\n",
        );
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "daily.csv"
  | complete trip_date, rider_type fill trips = 0, revenue = 0"#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            String::from_utf8(result.stdout.expect("csv stdout")).expect("utf8 csv"),
            "trip_date,rider_type,trips,revenue\n2026-04-01,member,3,13.85\n2026-04-01,visitor,2,33.13\n2026-04-03,member,2,8.35\n2026-04-03,visitor,0,0\n"
        );
    }

    #[test]
    fn complete_rejects_duplicate_key_tuples() {
        let io = InMemoryDriverIo::default().with_file_bytes(
            "memory/daily.csv",
            "trip_date,rider_type,trips\n2026-04-01,member,3\n2026-04-01,member,4\n",
        );
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "daily.csv"
  | complete trip_date, rider_type fill trips = 0"#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.stdout.is_none());
        assert!(result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E1208"));
    }

    #[test]
    fn executes_named_outputs_in_source_order() {
        let io = InMemoryDriverIo::default()
            .with_file_bytes("memory/sales.csv", "region,amount\nWest,30\nEast,10\n");
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"let sales =
  load "sales.csv"

output west =
  sales
  | filter region == "West"
  | save "west.csv"

output totals =
  sales
  | agg total = sum(amount)
  | save "totals.csv""#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: None,
                dry_run: true,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            result
                .named_outputs
                .iter()
                .map(|output| output.name.as_str())
                .collect::<Vec<_>>(),
            vec!["west", "totals"]
        );
        assert_eq!(
            result.named_outputs[0].table.columns,
            vec!["region", "amount"]
        );
        assert_eq!(result.named_outputs[0].table.rows.len(), 1);
        assert_eq!(result.named_outputs[1].table.columns, vec!["total"]);
    }

    #[test]
    fn multiple_named_outputs_reject_stdout_format() {
        let io = InMemoryDriverIo::default()
            .with_file_bytes("memory/sales.csv", "region,amount\nWest,30\n");
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"output one =
  load "sales.csv"

output two =
  load "sales.csv""#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.stdout.is_none());
        assert!(result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E1607"));
    }

    #[test]
    fn prepares_bikeshare_story_named_outputs() {
        let io = InMemoryDriverIo::default().with_file_bytes(
            "memory/trips.csv",
            "trip_id,trip_date,rider_type,weather,dock_id,bike_id,fare,tip,trip_status\nT1,2026-04-01,member,clear,D1,B1,10.005,0.5,valid\nT2,2026-04-01,visitor,clear,D2,B2,20.125,1.0,valid\nT3,2026-04-02,visitor,rain,D2,B2,8.1,0,invalid\nT4,2026-04-03,member,rain,D1,B1,5.333,0.25,valid\n",
        );
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"let cleaned =
  load "trips.csv"
  | filter trip_status == "valid"
  | mutate revenue = round(fare + tip, 2)

output daily_rider_trips =
  cleaned
  | group_by trip_date, rider_type
  | agg trips = count(), revenue = sum(revenue)
  | complete trip_date, rider_type fill trips = 0, revenue = 0
  | sort trip_date, rider_type
  | save "daily_rider_trips.csv"

output valid_trips =
  cleaned
  | select trip_id, trip_date, rider_type, weather, dock_id, revenue
  | sort trip_id
  | save "valid_trips.csv"

output revenue_inversion =
  cleaned
  | group_by rider_type
  | agg trips = count(), revenue = sum(revenue)
  | mutate `Share of rides` = round(trips, 2), `Share of revenue` = round(revenue, 2)
  | select rider_type, `Share of rides`, `Share of revenue`
  | pivot_longer `Share of rides`, `Share of revenue` names_to metric values_to value
  | save "revenue_inversion.csv"

output weather_split =
  cleaned
  | group_by weather, rider_type
  | agg trips = count()
  | sort weather, rider_type
  | save "weather_split.csv"

output dock_priority =
  cleaned
  | group_by dock_id
  | agg trips = count(), bikes = count_distinct(bike_id), revenue = sum(revenue)
  | mutate revenue = round(revenue, 2)
  | sort dock_id
  | save "dock_priority.csv""#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: None,
                dry_run: true,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            result
                .named_outputs
                .iter()
                .map(|output| output.name.as_str())
                .collect::<Vec<_>>(),
            vec![
                "daily_rider_trips",
                "valid_trips",
                "revenue_inversion",
                "weather_split",
                "dock_priority"
            ]
        );
        assert_eq!(
            named_output_csv(&result, "daily_rider_trips"),
            "trip_date,rider_type,trips,revenue\n2026-04-01,member,1,10.51\n2026-04-01,visitor,1,21.13\n2026-04-03,member,1,5.58\n2026-04-03,visitor,0,0\n"
        );
        assert_eq!(
            named_output_csv(&result, "valid_trips"),
            "trip_id,trip_date,rider_type,weather,dock_id,revenue\nT1,2026-04-01,member,clear,D1,10.51\nT2,2026-04-01,visitor,clear,D2,21.13\nT4,2026-04-03,member,rain,D1,5.58\n"
        );
        assert_eq!(
            named_output_csv(&result, "revenue_inversion"),
            "rider_type,metric,value\nmember,Share of rides,2\nmember,Share of revenue,16.09\nvisitor,Share of rides,1\nvisitor,Share of revenue,21.13\n"
        );
        assert_eq!(
            named_output_csv(&result, "weather_split"),
            "weather,rider_type,trips\nclear,member,1\nclear,visitor,1\nrain,member,1\n"
        );
        assert_eq!(
            named_output_csv(&result, "dock_priority"),
            "dock_id,trips,bikes,revenue\nD1,2,1,16.09\nD2,1,1,21.13\n"
        );
    }

    fn named_output_csv(result: &RunResult, name: &str) -> String {
        let table = &result
            .named_outputs
            .iter()
            .find(|output| output.name == name)
            .unwrap_or_else(|| panic!("missing named output `{name}`"))
            .table;
        String::from_utf8(
            pdl_data::write_table_to_bytes(DataFormat::Csv, table).expect("csv output"),
        )
        .expect("utf8 csv")
    }

    fn temp_workspace(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("pdl-{name}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&path).expect("create temp workspace");
        path
    }

    #[test]
    fn executes_window_mutations_with_rank_offsets_and_frames() {
        let io = InMemoryDriverIo::default().with_file_bytes(
            "memory/orders.csv",
            "order_id,customer_id,region,order_date,amount\nA1,C1,North,2026-02-01,10\nA2,C1,North,2026-02-03,25\nA3,C2,North,2026-02-02,15\nA4,C2,South,2026-02-01,40\nA5,C1,North,2026-02-04,5\n",
        );
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "orders.csv"
  | mutate customer_row = row_number() over (partition_by customer_id order_by order_date), customer_running_amount = sum(amount) over (partition_by customer_id order_by order_date frame running), previous_amount = lag(amount) over (partition_by customer_id order_by order_date), region_amount_rank = dense_rank() over (partition_by region order_by amount desc)
  | select order_id, customer_id, amount, customer_row, customer_running_amount, previous_amount, region_amount_rank"#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            String::from_utf8(result.stdout.expect("csv stdout")).expect("utf8 csv"),
            "order_id,customer_id,amount,customer_row,customer_running_amount,previous_amount,region_amount_rank\nA1,C1,10,1,10,,3\nA2,C1,25,2,35,10,1\nA3,C2,15,2,55,40,2\nA4,C2,40,1,40,,1\nA5,C1,5,3,40,25,4\n"
        );
    }

    #[test]
    fn native_engine_bounded_named_frames_match_rows() {
        let io = InMemoryDriverIo::default().with_file_bytes(
            "memory/orders.csv",
            "order_id,region,seq,amount\nA1,North,1,1\nA2,North,2,2\nA3,North,3,3\nA4,North,4,4\nA5,North,5,5\n",
        );
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "orders.csv"
  | mutate trailing_sum = sum(amount) over (partition_by region order_by seq frame trailing 2), leading_sum = sum(amount) over (partition_by region order_by seq frame leading 2), centered_sum = sum(amount) over (partition_by region order_by seq frame centered 1), remaining_sum = sum(amount) over (partition_by region order_by seq frame remaining)
  | select order_id, amount, trailing_sum, leading_sum, centered_sum, remaining_sum"#,
            &io,
        );
        let options = RunOptions {
            stdout_format: Some("csv".to_string()),
            dry_run: true,
            allow_binary_stdout: true,
        };
        let row = run_prepared_with_io_and_context_and_engine(
            &prepared,
            options.clone(),
            &io,
            BTreeMap::new(),
            ExecutionEngine::Row,
        );
        let auto = run_prepared_with_io_and_context_and_engine(
            &prepared,
            options.clone(),
            &io,
            BTreeMap::new(),
            ExecutionEngine::Auto,
        );
        let native = run_prepared_with_io_and_context_and_engine(
            &prepared,
            options,
            &io,
            BTreeMap::new(),
            ExecutionEngine::Native,
        );

        assert!(row.diagnostics.is_empty(), "{:?}", row.diagnostics);
        assert!(auto.diagnostics.is_empty(), "{:?}", auto.diagnostics);
        assert!(native.diagnostics.is_empty(), "{:?}", native.diagnostics);
        assert_eq!(auto.backend, DataBackend::NativePolars);
        assert_eq!(native.backend, DataBackend::NativePolars);
        let expected =
            "order_id,amount,trailing_sum,leading_sum,centered_sum,remaining_sum\nA1,1,1,6,3,15\nA2,2,3,9,6,14\nA3,3,6,12,9,12\nA4,4,9,9,12,9\nA5,5,12,5,9,5\n";
        assert_eq!(
            String::from_utf8(row.stdout.expect("row csv stdout")).expect("utf8 csv"),
            expected
        );
        assert_eq!(
            String::from_utf8(auto.stdout.expect("auto csv stdout")).expect("utf8 csv"),
            expected
        );
        assert_eq!(
            String::from_utf8(native.stdout.expect("native csv stdout")).expect("utf8 csv"),
            expected
        );
    }

    #[test]
    fn native_engine_bounded_named_frames_match_rows_for_edge_cases() {
        let assert_native_parity = |source: &str, files: &[(&str, &str)]| {
            let mut io = InMemoryDriverIo::default();
            for (path, bytes) in files {
                io = io.with_file_bytes(*path, *bytes);
            }
            let prepared = prepare_source_with_io("memory/main.pdl", source, &io);
            let options = RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
            };
            let row = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options.clone(),
                &io,
                BTreeMap::new(),
                ExecutionEngine::Row,
            );
            let auto = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options.clone(),
                &io,
                BTreeMap::new(),
                ExecutionEngine::Auto,
            );
            let native = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options,
                &io,
                BTreeMap::new(),
                ExecutionEngine::Native,
            );

            assert!(row.diagnostics.is_empty(), "{:?}", row.diagnostics);
            assert!(auto.diagnostics.is_empty(), "{:?}", auto.diagnostics);
            assert!(native.diagnostics.is_empty(), "{:?}", native.diagnostics);
            assert_eq!(auto.backend, DataBackend::NativePolars);
            assert_eq!(native.backend, DataBackend::NativePolars);
            let row_csv = String::from_utf8(row.stdout.expect("row csv")).expect("utf8");
            assert_eq!(
                String::from_utf8(auto.stdout.expect("auto csv")).expect("utf8"),
                row_csv
            );
            assert_eq!(
                String::from_utf8(native.stdout.expect("native csv")).expect("utf8"),
                row_csv
            );
        };

        assert_native_parity(
            r#"load "empty.csv"
  | mutate trailing_count = count() over (order_by amount frame trailing 0)
  | select amount, trailing_count"#,
            &[("memory/empty.csv", "amount\n")],
        );

        assert_native_parity(
            r#"load "orders.csv"
  | mutate
      current_count = count() over (partition_by region order_by order_key frame trailing 0),
      large_leading_sum = sum(amount) over (partition_by region order_by order_key frame leading 99),
      large_centered_count = count(amount) over (partition_by region order_by order_key frame centered 99),
      remaining_first = first_value(order_id) over (partition_by region order_by order_key frame remaining),
      trailing_first = first_value(order_id) over (partition_by region order_by order_key frame trailing 99),
      leading_last = last_value(order_id) over (partition_by region order_by order_key frame leading 99)
  | select order_id, current_count, large_leading_sum, large_centered_count, remaining_first, trailing_first, leading_last"#,
            &[(
                "memory/orders.csv",
                "order_id,region,order_key,amount\nN1,North,,1\nN2,North,,2\nS1,Solo,,10\nB1,Big,,5\nB2,Big,,7\nB3,Big,,11\n",
            )],
        );
    }

    #[test]
    fn executes_window_distribution_value_and_lead_functions() {
        let io = InMemoryDriverIo::default().with_file_bytes(
            "memory/orders.csv",
            "order_id,customer_id,region,order_date,amount\nA1,C1,North,2026-02-01,10\nA2,C1,North,2026-02-03,25\nA3,C2,North,2026-02-02,15\nA4,C2,South,2026-02-01,40\nA5,C1,North,2026-02-04,5\n",
        );
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "orders.csv"
  | mutate region_count = count() over (partition_by region), region_top_order = first_value(order_id) over (partition_by region order_by amount desc), region_low_order = last_value(order_id) over (partition_by region order_by amount desc frame whole_partition), next_amount = lead(amount, 1, "none") over (partition_by customer_id order_by order_date), region_percent_rank = percent_rank() over (partition_by region order_by amount desc)
  | select order_id, region_count, region_top_order, region_low_order, next_amount, region_percent_rank"#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            String::from_utf8(result.stdout.expect("csv stdout")).expect("utf8 csv"),
            "order_id,region_count,region_top_order,region_low_order,next_amount,region_percent_rank\nA1,4,A2,A5,25,0.6666666666666666\nA2,4,A2,A5,5,0\nA3,4,A2,A5,none,0.3333333333333333\nA4,1,A4,A4,15,0\nA5,4,A2,A5,none,1\n"
        );
    }

    #[test]
    fn executes_left_join_with_binding_and_suffixes() {
        let io = InMemoryDriverIo::default()
            .with_file_bytes(
                "memory/sales.csv",
                "sale_id,customer_id,amount,segment\nS1,C001,120,Direct\nS2,C999,50,Unknown\nS3,C003,200,Direct\n",
            )
            .with_file_bytes(
                "memory/customers.csv",
                "customer_id,segment\nC001,Enterprise\nC003,Consumer\n",
            );
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"let customers =
  load "customers.csv"

load "sales.csv"
  | join customers on customer_id kind left
  | select sale_id, customer_id, segment, segment_right
  | sort sale_id"#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            String::from_utf8(result.stdout.expect("csv stdout")).expect("utf8 csv"),
            "sale_id,customer_id,segment,segment_right\nS1,C001,Direct,Enterprise\nS2,C999,Unknown,\nS3,C003,Direct,Consumer\n"
        );
    }

    #[test]
    fn executes_full_join_with_unmatched_right_rows_sorted_by_key() {
        let io = InMemoryDriverIo::default()
            .with_file_bytes("memory/left.csv", "id,left_value\nB,left-b\n")
            .with_file_bytes(
                "memory/right.csv",
                "id,right_value\nC,right-c\nA,right-a\nB,right-b\n",
            );
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"let right_side =
  load "right.csv"

load "left.csv"
  | join right_side on id kind full"#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            String::from_utf8(result.stdout.expect("csv stdout")).expect("utf8 csv"),
            "id,left_value,right_value\nB,left-b,right-b\nA,,right-a\nC,,right-c\n"
        );
    }

    #[test]
    fn executes_union_by_name_and_distinct() {
        let io = InMemoryDriverIo::default()
            .with_file_bytes(
                "memory/day1.csv",
                "order_id,region,amount\nA1,North,10\nA2,South,20\n",
            )
            .with_file_bytes(
                "memory/day2.csv",
                "amount,region,order_id\n20,South,A2\n30,West,A3\n",
            );
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"let day2 =
  load "day2.csv"

load "day1.csv"
  | union day2 by_name true distinct true
  | sort order_id"#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            String::from_utf8(result.stdout.expect("csv stdout")).expect("utf8 csv"),
            "order_id,region,amount\nA1,North,10\nA2,South,20\nA3,West,30\n"
        );
    }

    #[test]
    fn executes_union_by_name_with_null_padding() {
        let io = InMemoryDriverIo::default()
            .with_file_bytes("memory/day1.csv", "order_id,region,amount\nA1,North,10\n")
            .with_file_bytes("memory/day2.csv", "order_id,amount,channel\nA2,30,web\n");
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"let day2 =
  load "day2.csv"

load "day1.csv"
  | union day2 by_name true
  | sort order_id"#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            String::from_utf8(result.stdout.expect("csv stdout")).expect("utf8 csv"),
            "order_id,region,amount,channel\nA1,North,10,\nA2,,30,web\n"
        );
    }

    #[test]
    fn incompatible_join_key_types_report_e1208() {
        let io = InMemoryDriverIo::default()
            .with_file_bytes("memory/left.csv", "id,value\n1,left\n")
            .with_file_bytes("memory/right.csv", "id,label\nA,right\n");
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"let right_side =
  load "right.csv"

load "left.csv"
  | join right_side on id"#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E1208"));
        assert!(result.stdout.is_none());
    }

    /// v0.46.5 temporal scalar functions: parsing, normalization, calendar
    /// fields, flooring, and formatting on the row runtime. `Z` and
    /// `+00:00` inputs must produce identical calendar fields and month
    /// keys, fractional seconds must parse, and unparseable text must
    /// return null. The automatic engine must demote to rows because the
    /// temporal subset has no native lowering in v0.46.5.
    #[test]
    fn temporal_scalar_functions_row_runtime_end_to_end() {
        let io = InMemoryDriverIo::default().with_stdin_bytes(
            "stamp\n\
             2025-02-17T14:20:59Z\n\
             2025-02-17T14:20:59+00:00\n\
             2024-01-15T10:22:33.123456-05:00\n\
             2024-01-15\n\
             not-a-date\n",
        );
        let prepared = prepare_source_for_run_with_io(
            "memory/main.pdl",
            r#"load stdin format "csv"
  | mutate
      day_key = date(stamp),
      normalized = datetime(stamp),
      y = year(stamp),
      m = month(stamp),
      d = day(stamp),
      month_start = date_floor(stamp, "month"),
      month_key = date_format(stamp, "%Y-%m"),
      week_key = date_format(stamp, "%G-W%V")
  | drop stamp"#,
            None,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(result.backend, DataBackend::NativePolars);
        assert_eq!(
            String::from_utf8(result.stdout.expect("csv stdout")).expect("utf8 csv"),
            "day_key,normalized,y,m,d,month_start,month_key,week_key\n\
             2025-02-17,2025-02-17T14:20:59Z,2025,2,17,2025-02-01T00:00:00Z,2025-02,2025-W08\n\
             2025-02-17,2025-02-17T14:20:59Z,2025,2,17,2025-02-01T00:00:00Z,2025-02,2025-W08\n\
             2024-01-15,2024-01-15T10:22:33-05:00,2024,1,15,2024-01-01T00:00:00-05:00,2024-01,2024-W03\n\
             2024-01-15,,2024,1,15,2024-01-01,2024-01,2024-W03\n\
             ,,,,,,,\n"
        );
    }

    /// v0.46.5 temporal diagnostics: unsupported literal units and pattern
    /// tokens report `E1406`; non-string units and patterns report `E1403`.
    #[test]
    fn temporal_scalar_functions_report_unit_and_pattern_diagnostics() {
        for (expr, code) in [
            (r#"date_floor(stamp, "fortnight")"#, "E1406"),
            (r#"date_floor(stamp, 7)"#, "E1403"),
            (r#"date_format(stamp, "%B")"#, "E1406"),
            (r#"date_format(stamp, null)"#, "E1403"),
        ] {
            let io = InMemoryDriverIo::default().with_stdin_bytes("stamp\n2024-01-15\n");
            let prepared = prepare_source_for_run_with_io(
                "memory/main.pdl",
                format!(
                    r#"load stdin format "csv"
  | mutate out = {expr}"#
                ),
                None,
                &io,
            );

            let result = run_prepared_with_io(
                &prepared,
                RunOptions {
                    stdout_format: Some("csv".to_string()),
                    dry_run: false,
                    allow_binary_stdout: true,
                },
                &io,
            );

            assert!(
                result
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code == code),
                "`{expr}` must report {code}: {:?}",
                result.diagnostics
            );
        }
    }
}
