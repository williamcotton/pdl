use pdl_core::{codes, Diagnostic, Span};
use pdl_data::{
    compare_values, read_table_from_bytes, sniff_format_from_bytes, DataFormat,
    NullsOrder as DataNullsOrder, Row, SortDirection as DataSortDirection, SortSpec, Table, Value,
};
use pdl_driver::{
    DriverIo, FormatDecision, OsDriverIo, PlanInputSource, PlanOutputSink, PreparedProgram,
    SinkDescriptor, SourceDescriptor,
};
use pdl_semantics::{
    AggItemIr, BinaryOpIr, ExprIr, JoinKindIr, MutateItemIr, NullsOrderIr, PipelineIr,
    PipelineStartIr, SortDirectionIr, StageIr, UnaryOpIr,
};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use crate::output::{emit_stdout, write_output};
use crate::planning::{plan_prepared, PlanningOptions};

#[derive(Clone, Debug)]
pub struct RunOptions {
    pub stdout_format: Option<String>,
    pub dry_run: bool,
    pub allow_binary_stdout: bool,
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
    pub diagnostics: Vec<Diagnostic>,
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
    let plan = match plan_prepared(
        prepared,
        PlanningOptions {
            stdout_format: options.stdout_format.clone(),
            dry_run: options.dry_run,
            allow_binary_stdout: options.allow_binary_stdout,
        },
    ) {
        Ok(plan) => plan,
        Err(diagnostics) => {
            return RunResult {
                stdout: None,
                diagnostics,
            };
        }
    };

    let mut runtime = Runtime {
        prepared,
        diagnostics: prepared.diagnostics(),
        cache: BTreeMap::new(),
        active_bindings: Vec::new(),
        dry_run: plan.dry_run,
        stdout: None,
        io,
    };

    let Some(ir) = prepared.analysis.ir.as_ref() else {
        runtime.diagnostics.push(Diagnostic::error(
            codes::E1505,
            "semantic IR is unavailable for execution",
            Span::zero(),
        ));
        return RunResult {
            stdout: None,
            diagnostics: runtime.diagnostics,
        };
    };
    let Some(main) = &ir.main else {
        runtime.diagnostics.push(Diagnostic::error(
            codes::E1502,
            "no runnable main pipeline",
            Span::zero(),
        ));
        return RunResult {
            stdout: None,
            diagnostics: runtime.diagnostics,
        };
    };

    let table = match runtime.execute_pipeline(main) {
        Ok(table) => table,
        Err(diagnostic) => {
            runtime.diagnostics.push(diagnostic);
            return RunResult {
                stdout: None,
                diagnostics: runtime.diagnostics,
            };
        }
    };

    let stdout = if let Some(format) = plan.stdout_format {
        match emit_stdout(format, &table) {
            Ok(bytes) => Some(bytes),
            Err(diagnostic) => {
                runtime.diagnostics.push(diagnostic);
                None
            }
        }
    } else {
        runtime.stdout.take()
    };

    RunResult {
        stdout,
        diagnostics: runtime.diagnostics,
    }
}

struct Runtime<'a> {
    prepared: &'a PreparedProgram,
    diagnostics: Vec<Diagnostic>,
    cache: BTreeMap<String, Table>,
    active_bindings: Vec<String>,
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
                        .map(|item| (item.source.clone(), item.output.clone()))
                        .collect();
                    table = table.select(&selection);
                    grouping = None;
                }
                StageIr::Drop { columns, .. } => {
                    table = table.drop_columns(columns);
                    grouping = None;
                }
                StageIr::Rename { items, .. } => {
                    let renames: Vec<(String, String)> = items
                        .iter()
                        .map(|item| (item.old.clone(), item.new.clone()))
                        .collect();
                    table = table.rename_columns(&renames);
                    grouping = None;
                }
                StageIr::Mutate { items, .. } => {
                    table = self.mutate(table, items)?;
                    grouping = None;
                }
                StageIr::GroupBy { columns, .. } => {
                    grouping = Some(columns.clone());
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
                            SortSpec {
                                column: item.column.clone(),
                                direction,
                                nulls,
                            }
                        })
                        .collect::<Vec<_>>();
                    table.stable_sort(&specs);
                }
                StageIr::Limit { n, .. } => {
                    table = table.limit(*n);
                }
                StageIr::Join {
                    source,
                    source_span,
                    left_key,
                    right_key,
                    kind,
                    span,
                } => {
                    let right = self.execute_binding(source, *source_span)?;
                    table = self.join(table, right, left_key, right_key, *kind, *span)?;
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
                StageIr::Distinct { columns, .. } => {
                    table = table.distinct(columns);
                    grouping = None;
                }
                StageIr::Save { format, span, .. } => {
                    self.execute_save(*span, format.as_deref(), &table)?;
                }
                StageIr::Unsupported { name, span } => {
                    return Err(Diagnostic::error(
                        codes::E1211,
                        format!("stage `{name}` is deferred in 0.15.0"),
                        *span,
                    ));
                }
            }
        }

        Ok(table)
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
            .filter_map(
                |row| match eval_row_expr(expr, &table, row, ExprRole::PredicateRoot) {
                    Ok(value) if value.is_truthy_true() => Some(Ok(row.clone())),
                    Ok(_) => None,
                    Err(diagnostic) => Some(Err(diagnostic)),
                },
            )
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
                values.push(eval_aggregate(item, table, &group_rows)?);
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
            .map(|row| {
                let mut values = row.values.clone();
                for item in items {
                    let value = eval_row_expr(&item.expr, &table, row, ExprRole::Default)?;
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
        left_key: &str,
        right_key: &str,
        kind: JoinKindIr,
        span: Span,
    ) -> Result<Table, Diagnostic> {
        ensure_key_types_compatible(&left, left_key, &right, right_key, span)?;
        let output_columns = join_columns(&left.columns, &right.columns, right_key, kind, span)?;
        if matches!(kind, JoinKindIr::Semi | JoinKindIr::Anti) {
            return Ok(join_semi_anti(left, &right, left_key, right_key, kind));
        }

        let left_key_index = left.column_index(left_key).ok_or_else(|| {
            Diagnostic::error(codes::E1005, format!("unknown column `{left_key}`"), span)
        })?;
        let right_key_index = right.column_index(right_key).ok_or_else(|| {
            Diagnostic::error(codes::E1005, format!("unknown column `{right_key}`"), span)
        })?;
        let left_matches = join_index(&left, left_key);
        let right_matches = join_index(&right, right_key);
        let right_value_indices = right_non_key_indices(&right.columns, right_key);
        let mut rows = Vec::new();

        match kind {
            JoinKindIr::Inner | JoinKindIr::Left | JoinKindIr::Full => {
                let mut matched_right = vec![false; right.rows.len()];
                for left_row in &left.rows {
                    let key = row_key(left_row, left_key_index);
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
                        row_key(left_row, right_key_index).cmp(&row_key(right_row, right_key_index))
                    });
                    for (_, right_row) in unmatched_right {
                        rows.push(right_only_row(
                            right_row,
                            right_key_index,
                            left_key_index,
                            left.columns.len(),
                            &right_value_indices,
                        ));
                    }
                }
            }
            JoinKindIr::Right => {
                for right_row in &right.rows {
                    let key = row_key(right_row, right_key_index);
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
                            right_key_index,
                            left_key_index,
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
        let columns = left.columns.clone();
        let mut rows = left.rows.clone();
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

fn resolve_input_format(
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
                format!("format `{format}` is not supported in 0.15.0"),
                span,
            )
        });
    }
    if matches!(&input.source, SourceDescriptor::Stdin) {
        if let Some(format) = stdin_format {
            return DataFormat::from_name(format).ok_or_else(|| {
                Diagnostic::error(
                    codes::E1215,
                    format!("stdin format `{format}` is not supported in 0.15.0"),
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

fn resolve_output_format(
    sink: &PlanOutputSink,
    explicit_format: Option<&str>,
    span: Span,
) -> Result<DataFormat, Diagnostic> {
    if let Some(format) = explicit_format {
        return DataFormat::from_name(format).ok_or_else(|| {
            Diagnostic::error(
                codes::E1705,
                format!("output format `{format}` is not supported in 0.15.0"),
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

fn join_columns(
    left_columns: &[String],
    right_columns: &[String],
    right_key: &str,
    kind: JoinKindIr,
    span: Span,
) -> Result<Vec<String>, Diagnostic> {
    if matches!(kind, JoinKindIr::Semi | JoinKindIr::Anti) {
        return Ok(left_columns.to_vec());
    }

    let mut columns = left_columns.to_vec();
    for column in right_columns {
        if column == right_key {
            continue;
        }
        let mut output = column.clone();
        if columns.iter().any(|existing| existing == &output) {
            output.push_str("_right");
            if columns.iter().any(|existing| existing == &output) {
                return Err(Diagnostic::error(
                    codes::E1207,
                    format!("output column collision `{output}`"),
                    span,
                ));
            }
        }
        columns.push(output);
    }
    Ok(columns)
}

fn right_non_key_indices(columns: &[String], right_key: &str) -> Vec<usize> {
    columns
        .iter()
        .enumerate()
        .filter_map(|(index, column)| (column != right_key).then_some(index))
        .collect()
}

fn join_index(table: &Table, key: &str) -> BTreeMap<String, Vec<usize>> {
    let Some(index) = table.column_index(key) else {
        return BTreeMap::new();
    };
    let mut matches = BTreeMap::new();
    for (row_index, row) in table.rows.iter().enumerate() {
        if let Some(key) = row_key(row, index) {
            matches.entry(key).or_insert_with(Vec::new).push(row_index);
        }
    }
    matches
}

fn row_key(row: &Row, index: usize) -> Option<String> {
    match row.values.get(index).unwrap_or(&Value::Null) {
        Value::Null => None,
        value => Some(value.to_csv_cell()),
    }
}

fn combine_rows(
    left_row: &Row,
    right_row: Option<&Row>,
    right_value_indices: &[usize],
    left_width: usize,
) -> Row {
    let mut values = (0..left_width)
        .map(|index| left_row.values.get(index).cloned().unwrap_or(Value::Null))
        .collect::<Vec<_>>();
    match right_row {
        Some(right_row) => {
            values.extend(
                right_value_indices
                    .iter()
                    .map(|index| right_row.values.get(*index).cloned().unwrap_or(Value::Null)),
            );
        }
        None => values.extend((0..right_value_indices.len()).map(|_| Value::Null)),
    }
    Row { values }
}

fn right_only_row(
    right_row: &Row,
    right_key_index: usize,
    left_key_index: usize,
    left_width: usize,
    right_value_indices: &[usize],
) -> Row {
    let mut values = vec![Value::Null; left_width];
    if let Some(value) = right_row.values.get(right_key_index) {
        if let Some(left_key) = values.get_mut(left_key_index) {
            *left_key = value.clone();
        }
    }
    values.extend(
        right_value_indices
            .iter()
            .map(|index| right_row.values.get(*index).cloned().unwrap_or(Value::Null)),
    );
    Row { values }
}

fn join_semi_anti(
    left: Table,
    right: &Table,
    left_key: &str,
    right_key: &str,
    kind: JoinKindIr,
) -> Table {
    let Some(left_index) = left.column_index(left_key) else {
        return left;
    };
    let right_matches = join_index(right, right_key);
    let rows = left
        .rows
        .iter()
        .filter(|row| {
            let matched = row_key(row, left_index)
                .as_ref()
                .is_some_and(|key| right_matches.contains_key(key));
            match kind {
                JoinKindIr::Semi => matched,
                JoinKindIr::Anti => !matched,
                _ => unreachable!("semi/anti helper called for non-semi join"),
            }
        })
        .cloned()
        .collect();
    Table {
        columns: left.columns,
        rows,
    }
}

fn ensure_key_types_compatible(
    left: &Table,
    left_key: &str,
    right: &Table,
    right_key: &str,
    span: Span,
) -> Result<(), Diagnostic> {
    let left_classes = column_value_classes(left, left_key);
    let right_classes = column_value_classes(right, right_key);
    if left_classes.is_empty() || right_classes.is_empty() || left_classes == right_classes {
        return Ok(());
    }

    Err(Diagnostic::error(
        codes::E1208,
        format!("join keys `{left_key}` and `{right_key}` have incompatible observed types"),
        span,
    ))
}

fn ensure_union_compatible(
    left: &Table,
    right: &Table,
    by_name: bool,
    span: Span,
) -> Result<(), Diagnostic> {
    if by_name {
        let left_names: BTreeSet<&String> = left.columns.iter().collect();
        let right_names: BTreeSet<&String> = right.columns.iter().collect();
        if left_names != right_names {
            return Err(Diagnostic::error(
                codes::E1209,
                "union schemas have different column names",
                span,
            ));
        }
        for column in &left.columns {
            ensure_union_column_compatible(left, column, right, column, span)?;
        }
    } else {
        if left.columns.len() != right.columns.len() {
            return Err(Diagnostic::error(
                codes::E1209,
                "union schemas have different column counts",
                span,
            ));
        }
        for (left_column, right_column) in left.columns.iter().zip(&right.columns) {
            ensure_union_column_compatible(left, left_column, right, right_column, span)?;
        }
    }
    Ok(())
}

fn ensure_union_column_compatible(
    left: &Table,
    left_column: &str,
    right: &Table,
    right_column: &str,
    span: Span,
) -> Result<(), Diagnostic> {
    let left_classes = column_value_classes(left, left_column);
    let right_classes = column_value_classes(right, right_column);
    if left_classes.is_empty() || right_classes.is_empty() || left_classes == right_classes {
        return Ok(());
    }

    Err(Diagnostic::error(
        codes::E1209,
        format!(
            "union columns `{left_column}` and `{right_column}` have incompatible observed types"
        ),
        span,
    ))
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ValueClass {
    Bool,
    Number,
    String,
}

fn column_value_classes(table: &Table, column: &str) -> BTreeSet<ValueClass> {
    let Some(index) = table.column_index(column) else {
        return BTreeSet::new();
    };
    table
        .rows
        .iter()
        .filter_map(|row| match row.values.get(index).unwrap_or(&Value::Null) {
            Value::Null => None,
            Value::Bool(_) => Some(ValueClass::Bool),
            Value::Number(_) => Some(ValueClass::Number),
            Value::String(_) => Some(ValueClass::String),
        })
        .collect()
}

#[derive(Clone, Copy)]
enum ExprRole {
    PredicateRoot,
    Default,
    ComparisonLeft,
    ComparisonRight,
}

fn eval_row_expr(
    expr: &ExprIr,
    table: &Table,
    row: &Row,
    role: ExprRole,
) -> Result<Value, Diagnostic> {
    match expr {
        ExprIr::Quoted { value, span } => match role {
            ExprRole::ComparisonLeft => column_value(table, row, value, *span),
            ExprRole::PredicateRoot | ExprRole::Default => {
                if table.column_index(value).is_some() {
                    column_value(table, row, value, *span)
                } else {
                    Ok(Value::String(value.clone()))
                }
            }
            ExprRole::ComparisonRight => Ok(Value::String(value.clone())),
        },
        ExprIr::Number { value, .. } => Ok(Value::Number(*value)),
        ExprIr::Bool { value, .. } => Ok(Value::Bool(*value)),
        ExprIr::Null { .. } => Ok(Value::Null),
        ExprIr::Ident { value, span } => Err(Diagnostic::error(
            codes::E0008,
            format!("unexpected bare identifier `{value}` in expression"),
            *span,
        )),
        ExprIr::Call { name, args, span } => eval_call(name, args, table, row, *span),
        ExprIr::Unary { op, expr, span } => {
            let value = eval_row_expr(expr, table, row, ExprRole::Default)?;
            match op {
                UnaryOpIr::Not => match value {
                    Value::Bool(value) => Ok(Value::Bool(!value)),
                    Value::Null => Ok(Value::Null),
                    _ => Err(Diagnostic::error(
                        codes::E1302,
                        "`not` requires a boolean",
                        *span,
                    )),
                },
                UnaryOpIr::Neg => match value {
                    Value::Number(value) => Ok(Value::Number(-value)),
                    _ => Err(Diagnostic::error(
                        codes::E1302,
                        "`-` requires a number",
                        *span,
                    )),
                },
            }
        }
        ExprIr::Binary {
            left,
            op,
            right,
            span,
        } => eval_binary(*op, left, right, table, row, *span),
    }
}

fn eval_call(
    name: &str,
    args: &[ExprIr],
    table: &Table,
    row: &Row,
    span: Span,
) -> Result<Value, Diagnostic> {
    match name {
        "col" => match args {
            [ExprIr::Quoted {
                value: column,
                span,
            }] => column_value(table, row, column, *span),
            _ => Err(Diagnostic::error(
                codes::E1402,
                "col() expects one quoted column name",
                span,
            )),
        },
        "lit" => match args {
            [ExprIr::Quoted { value, .. }] => Ok(Value::String(value.clone())),
            [expr] => eval_row_expr(expr, table, row, ExprRole::Default),
            _ => Err(Diagnostic::error(
                codes::E1402,
                "lit() expects one argument",
                span,
            )),
        },
        "is_null" => match args {
            [expr] => Ok(Value::Bool(matches!(
                eval_row_expr(expr, table, row, ExprRole::Default)?,
                Value::Null
            ))),
            _ => Err(Diagnostic::error(
                codes::E1402,
                "is_null() expects one argument",
                span,
            )),
        },
        "not_null" => match args {
            [expr] => Ok(Value::Bool(!matches!(
                eval_row_expr(expr, table, row, ExprRole::Default)?,
                Value::Null
            ))),
            _ => Err(Diagnostic::error(
                codes::E1402,
                "not_null() expects one argument",
                span,
            )),
        },
        "coalesce" => {
            for arg in args {
                let value = eval_row_expr(arg, table, row, ExprRole::Default)?;
                if !matches!(value, Value::Null) {
                    return Ok(value);
                }
            }
            Ok(Value::Null)
        }
        "concat" => {
            let mut text = String::new();
            for arg in args {
                let value = eval_row_expr(arg, table, row, ExprRole::Default)?;
                if !matches!(value, Value::Null) {
                    text.push_str(&value.to_csv_cell());
                }
            }
            Ok(Value::String(text))
        }
        "lower" => eval_single_arg(args, table, row, span, |value| {
            Ok(map_text(value, |text| text.to_ascii_lowercase()))
        }),
        "upper" => eval_single_arg(args, table, row, span, |value| {
            Ok(map_text(value, |text| text.to_ascii_uppercase()))
        }),
        "trim" => eval_single_arg(args, table, row, span, |value| {
            Ok(map_text(value, |text| text.trim().to_string()))
        }),
        "to_number" => eval_single_arg(args, table, row, span, |value| {
            Ok(match value {
                Value::Null => Value::Null,
                Value::Number(_) => value,
                _ => value
                    .to_csv_cell()
                    .trim()
                    .parse::<f64>()
                    .map(Value::Number)
                    .unwrap_or(Value::Null),
            })
        }),
        "abs" => eval_single_arg(args, table, row, span, |value| match value {
            Value::Null => Ok(Value::Null),
            Value::Number(value) => Ok(Value::Number(value.abs())),
            _ => Err(Diagnostic::error(
                codes::E1302,
                "abs() requires a number",
                span,
            )),
        }),
        "round" => eval_single_arg(args, table, row, span, |value| match value {
            Value::Null => Ok(Value::Null),
            Value::Number(value) => Ok(Value::Number(value.round())),
            _ => Err(Diagnostic::error(
                codes::E1302,
                "round() requires a number",
                span,
            )),
        }),
        "if_else" => match args {
            [condition, when_true, when_false] => {
                let condition = eval_row_expr(condition, table, row, ExprRole::Default)?;
                match condition {
                    Value::Bool(true) => eval_row_expr(when_true, table, row, ExprRole::Default),
                    Value::Bool(false) => eval_row_expr(when_false, table, row, ExprRole::Default),
                    Value::Null => Ok(Value::Null),
                    _ => Err(Diagnostic::error(
                        codes::E1302,
                        "if_else() condition requires a boolean",
                        span,
                    )),
                }
            }
            _ => Err(Diagnostic::error(
                codes::E1402,
                "if_else() expects three arguments",
                span,
            )),
        },
        _ => Err(Diagnostic::error(
            codes::E1401,
            format!("unknown function `{name}`"),
            span,
        )),
    }
}

fn eval_single_arg(
    args: &[ExprIr],
    table: &Table,
    row: &Row,
    span: Span,
    apply: impl FnOnce(Value) -> Result<Value, Diagnostic>,
) -> Result<Value, Diagnostic> {
    match args {
        [expr] => {
            let value = eval_row_expr(expr, table, row, ExprRole::Default)?;
            apply(value)
        }
        _ => Err(Diagnostic::error(
            codes::E1402,
            "function expects one argument",
            span,
        )),
    }
}

fn map_text(value: Value, apply: impl FnOnce(String) -> String) -> Value {
    match value {
        Value::Null => Value::Null,
        Value::String(value) => Value::String(apply(value)),
        _ => Value::String(apply(value.to_csv_cell())),
    }
}

fn eval_binary(
    op: BinaryOpIr,
    left: &ExprIr,
    right: &ExprIr,
    table: &Table,
    row: &Row,
    span: Span,
) -> Result<Value, Diagnostic> {
    if is_comparison_op(op) {
        let left = eval_row_expr(left, table, row, ExprRole::ComparisonLeft)?;
        let right = eval_row_expr(right, table, row, ExprRole::ComparisonRight)?;
        return Ok(compare_for_op(&left, op, &right));
    }

    match op {
        BinaryOpIr::And => {
            let left = eval_row_expr(left, table, row, ExprRole::Default)?;
            let right = eval_row_expr(right, table, row, ExprRole::Default)?;
            Ok(nullable_and(left, right))
        }
        BinaryOpIr::Or => {
            let left = eval_row_expr(left, table, row, ExprRole::Default)?;
            let right = eval_row_expr(right, table, row, ExprRole::Default)?;
            Ok(nullable_or(left, right))
        }
        BinaryOpIr::Add | BinaryOpIr::Sub | BinaryOpIr::Mul | BinaryOpIr::Div | BinaryOpIr::Rem => {
            let left = eval_row_expr(left, table, row, ExprRole::Default)?;
            let right = eval_row_expr(right, table, row, ExprRole::Default)?;
            let (Some(left), Some(right)) = (left.as_number(), right.as_number()) else {
                return Err(Diagnostic::error(
                    codes::E1302,
                    "arithmetic requires numeric operands",
                    span,
                ));
            };
            match op {
                BinaryOpIr::Add => Ok(Value::Number(left + right)),
                BinaryOpIr::Sub => Ok(Value::Number(left - right)),
                BinaryOpIr::Mul => Ok(Value::Number(left * right)),
                BinaryOpIr::Div if right == 0.0 => {
                    Err(Diagnostic::error(codes::E1407, "division by zero", span))
                }
                BinaryOpIr::Div => Ok(Value::Number(left / right)),
                BinaryOpIr::Rem => Ok(Value::Number(left % right)),
                _ => unreachable!(),
            }
        }
        _ => unreachable!("comparison operators returned earlier"),
    }
}

fn compare_for_op(left: &Value, op: BinaryOpIr, right: &Value) -> Value {
    let Some(ordering) = compare_values(left, right) else {
        return Value::Null;
    };
    let result = match op {
        BinaryOpIr::Eq => ordering == Ordering::Equal,
        BinaryOpIr::Ne => ordering != Ordering::Equal,
        BinaryOpIr::Lt => ordering == Ordering::Less,
        BinaryOpIr::Lte => matches!(ordering, Ordering::Less | Ordering::Equal),
        BinaryOpIr::Gt => ordering == Ordering::Greater,
        BinaryOpIr::Gte => matches!(ordering, Ordering::Greater | Ordering::Equal),
        _ => unreachable!(),
    };
    Value::Bool(result)
}

fn nullable_and(left: Value, right: Value) -> Value {
    match (left, right) {
        (Value::Bool(false), _) | (_, Value::Bool(false)) => Value::Bool(false),
        (Value::Bool(true), Value::Bool(true)) => Value::Bool(true),
        (Value::Null, _) | (_, Value::Null) => Value::Null,
        _ => Value::Null,
    }
}

fn nullable_or(left: Value, right: Value) -> Value {
    match (left, right) {
        (Value::Bool(true), _) | (_, Value::Bool(true)) => Value::Bool(true),
        (Value::Bool(false), Value::Bool(false)) => Value::Bool(false),
        (Value::Null, _) | (_, Value::Null) => Value::Null,
        _ => Value::Null,
    }
}

fn column_value(table: &Table, row: &Row, column: &str, span: Span) -> Result<Value, Diagnostic> {
    table
        .value(row, column)
        .cloned()
        .ok_or_else(|| Diagnostic::error(codes::E1005, format!("unknown column `{column}`"), span))
}

fn eval_aggregate(item: &AggItemIr, table: &Table, rows: &[&Row]) -> Result<Value, Diagnostic> {
    match item.function.as_str() {
        "count" if item.args.is_empty() => Ok(Value::Number(rows.len() as f64)),
        "count" => {
            let values = aggregate_arg_values(&item.args[0], table, rows)?;
            Ok(Value::Number(
                values
                    .iter()
                    .filter(|value| !matches!(value, Value::Null))
                    .count() as f64,
            ))
        }
        "sum" => {
            let values = aggregate_arg_values(&item.args[0], table, rows)?;
            let mut found = false;
            let mut sum = 0.0;
            for value in values {
                if let Value::Number(number) = value {
                    found = true;
                    sum += number;
                }
            }
            Ok(if found {
                Value::Number(sum)
            } else {
                Value::Null
            })
        }
        "mean" => {
            let values = aggregate_arg_values(&item.args[0], table, rows)?;
            let numbers: Vec<f64> = values
                .into_iter()
                .filter_map(|value| value.as_number())
                .collect();
            if numbers.is_empty() {
                Ok(Value::Null)
            } else {
                Ok(Value::Number(
                    numbers.iter().sum::<f64>() / numbers.len() as f64,
                ))
            }
        }
        "min" => aggregate_arg_values(&item.args[0], table, rows).map(|values| {
            values
                .into_iter()
                .filter(|value| !matches!(value, Value::Null))
                .min_by(|left, right| compare_values(left, right).unwrap_or(Ordering::Equal))
                .unwrap_or(Value::Null)
        }),
        "max" => aggregate_arg_values(&item.args[0], table, rows).map(|values| {
            values
                .into_iter()
                .filter(|value| !matches!(value, Value::Null))
                .max_by(|left, right| compare_values(left, right).unwrap_or(Ordering::Equal))
                .unwrap_or(Value::Null)
        }),
        function => Err(Diagnostic::error(
            codes::E1401,
            format!("unknown aggregate function `{function}`"),
            item.span,
        )),
    }
}

fn aggregate_arg_values(
    expr: &ExprIr,
    table: &Table,
    rows: &[&Row],
) -> Result<Vec<Value>, Diagnostic> {
    rows.iter()
        .map(|row| eval_aggregate_expr(expr, table, row))
        .collect()
}

fn eval_aggregate_expr(expr: &ExprIr, table: &Table, row: &Row) -> Result<Value, Diagnostic> {
    match expr {
        ExprIr::Quoted { value, span } => column_value(table, row, value, *span),
        ExprIr::Call { name, args, span } if name == "lit" => match args.as_slice() {
            [ExprIr::Quoted { value, .. }] => Ok(Value::String(value.clone())),
            _ => Err(Diagnostic::error(
                codes::E1402,
                "lit() expects one quoted value",
                *span,
            )),
        },
        ExprIr::Call { name, args, span } if name == "col" => match args.as_slice() {
            [ExprIr::Quoted {
                value: column,
                span,
            }] => column_value(table, row, column, *span),
            _ => Err(Diagnostic::error(
                codes::E1402,
                "col() expects one quoted column name",
                *span,
            )),
        },
        _ => eval_row_expr(expr, table, row, ExprRole::Default),
    }
}

fn is_comparison_op(op: BinaryOpIr) -> bool {
    matches!(
        op,
        BinaryOpIr::Eq
            | BinaryOpIr::Ne
            | BinaryOpIr::Lt
            | BinaryOpIr::Lte
            | BinaryOpIr::Gt
            | BinaryOpIr::Gte
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdl_driver::{prepare_source_for_run_with_io, prepare_source_with_io, InMemoryDriverIo};
    use std::path::Path;

    #[test]
    fn runs_csv_stdin_with_explicit_format() {
        let io = InMemoryDriverIo::default()
            .with_stdin_bytes("status,amount\ncompleted,10\npending,20\n");
        let prepared = prepare_source_for_run_with_io(
            "memory/main.pdl",
            r#"load stdin format "csv"
  | filter "status" == "completed"
  | select "amount""#,
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
  | sort "amount" desc"#,
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
  | sort "amount" desc"#,
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
  | filter "status" == "completed"
  | mutate "net_amount" = "gross" - "discount", "region_channel" = concat(upper("region"), lit(":"), lower("channel")), "priority" = if_else("gross" >= 150, lit("high"), lit("standard"))
  | distinct "order_id"
  | select "order_id", "region_channel", "net_amount", "priority"
  | sort "order_id""#,
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
  | join customers on "customer_id" kind left
  | select "sale_id", "customer_id", "segment", "segment_right"
  | sort "sale_id""#,
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
  | join right_side on "id" kind full"#,
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
  | sort "order_id""#,
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
    fn incompatible_join_key_types_report_e1208() {
        let io = InMemoryDriverIo::default()
            .with_file_bytes("memory/left.csv", "id,value\n1,left\n")
            .with_file_bytes("memory/right.csv", "id,label\nA,right\n");
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"let right_side =
  load "right.csv"

load "left.csv"
  | join right_side on "id""#,
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
}
