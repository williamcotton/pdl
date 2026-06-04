use pdl_core::{codes, has_errors, Diagnostic, Span};
use pdl_data::{sniff_format_from_bytes, DataFormat};
use pdl_semantics::{analyze_program, Analysis, LoadRequest};
use pdl_syntax::{
    parse, BinaryOp, Binding, Expr, ParseResult, Pipeline, PipelineStart, Program, SourceRef,
    Spanned, Stage,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::io::{DriverIo, OsDriverIo};
use crate::path::resolve_input_path;
use crate::plan::DriverPlan;
use crate::report::{PreparationReport, ReportPhase};
use crate::source::SourceOrigin;

#[derive(Clone, Debug)]
pub struct PreparedProgram {
    pub path: PathBuf,
    pub origin: SourceOrigin,
    pub source: String,
    pub parse: ParseResult,
    pub analysis: Analysis,
    pub driver_plan: DriverPlan,
    pub report: PreparationReport,
    pub stdin_format: Option<String>,
    pub stdin_bytes: Option<Vec<u8>>,
}

impl PreparedProgram {
    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        self.report.diagnostics()
    }

    pub fn has_errors(&self) -> bool {
        has_errors(&self.diagnostics())
    }
}

pub fn prepare_file(path: impl AsRef<Path>) -> Result<PreparedProgram, Diagnostic> {
    let path = path.as_ref().to_path_buf();
    let io = OsDriverIo;
    let source = io.read_source(&path)?;
    Ok(prepare_source_with_options_and_io(
        path,
        source,
        PrepareOptions::default(),
        &io,
    ))
}

pub fn prepare_file_with_options(
    path: impl AsRef<Path>,
    options: PrepareOptions,
) -> Result<PreparedProgram, Diagnostic> {
    let path = path.as_ref().to_path_buf();
    let io = OsDriverIo;
    let source = io.read_source(&path)?;
    Ok(prepare_source_with_options_and_io(
        path, source, options, &io,
    ))
}

pub fn prepare_file_for_run(
    path: impl AsRef<Path>,
    stdin_format: Option<String>,
) -> Result<PreparedProgram, Diagnostic> {
    let path = path.as_ref().to_path_buf();
    let io = OsDriverIo;
    let source = io.read_source(&path)?;
    Ok(prepare_source_with_options_and_io(
        path,
        source,
        PrepareOptions {
            stdin_format,
            read_stdin: true,
            analysis_binding: None,
        },
        &io,
    ))
}

pub fn prepare_file_for_binding_schema(
    path: impl AsRef<Path>,
    binding: impl Into<String>,
) -> Result<PreparedProgram, Diagnostic> {
    let path = path.as_ref().to_path_buf();
    let io = OsDriverIo;
    let source = io.read_source(&path)?;
    Ok(prepare_source_with_options_and_io(
        path,
        source,
        PrepareOptions {
            stdin_format: None,
            read_stdin: false,
            analysis_binding: Some(binding.into()),
        },
        &io,
    ))
}

pub fn prepare_source(path: impl AsRef<Path>, source: impl Into<String>) -> PreparedProgram {
    let io = OsDriverIo;
    prepare_source_with_io(path, source, &io)
}

pub fn prepare_source_with_io(
    path: impl AsRef<Path>,
    source: impl Into<String>,
    io: &dyn DriverIo,
) -> PreparedProgram {
    prepare_source_with_options_and_io(path, source, PrepareOptions::default(), io)
}

pub fn prepare_source_for_run_with_io(
    path: impl AsRef<Path>,
    source: impl Into<String>,
    stdin_format: Option<String>,
    io: &dyn DriverIo,
) -> PreparedProgram {
    prepare_source_with_options_and_io(
        path,
        source,
        PrepareOptions {
            stdin_format,
            read_stdin: true,
            analysis_binding: None,
        },
        io,
    )
}

#[derive(Clone, Debug, Default)]
pub struct PrepareOptions {
    pub stdin_format: Option<String>,
    pub read_stdin: bool,
    pub analysis_binding: Option<String>,
}

pub fn prepare_source_with_options_and_io(
    path: impl AsRef<Path>,
    source: impl Into<String>,
    options: PrepareOptions,
    io: &dyn DriverIo,
) -> PreparedProgram {
    let path = path.as_ref().to_path_buf();
    let source = source.into();
    let parse = parse(&source);
    let mut report = PreparationReport::default();
    report.extend(ReportPhase::Parse, parse.diagnostics.clone());
    let origin = SourceOrigin::path(path.clone());
    let driver_plan = DriverPlan::build(&path, origin.clone(), &parse.program);

    let base_dir = path
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let parse_has_errors = has_errors(&parse.diagnostics);
    let pre_read_diagnostics = stdin_pre_read_diagnostics(&parse.program, &options.stdin_format);
    report.extend(ReportPhase::SourceResolution, pre_read_diagnostics.clone());

    let mut stdin_read_diagnostic = None;
    let stdin_bytes = if options.read_stdin
        && !driver_plan.stdin_reads().is_empty()
        && !has_errors(&pre_read_diagnostics)
    {
        match io.read_stdin_bytes() {
            Ok(bytes) => Some(bytes),
            Err(diagnostic) => {
                stdin_read_diagnostic = Some(diagnostic.clone());
                report.extend(ReportPhase::SchemaFacts, [diagnostic]);
                None
            }
        }
    } else {
        None
    };

    let analysis_blocked = has_errors(&pre_read_diagnostics) || stdin_read_diagnostic.is_some();
    let analysis = if analysis_blocked
        || (parse_has_errors && !parse_errors_allow_semantic_followup(&parse.diagnostics))
    {
        Analysis::default()
    } else {
        let analysis_program = options.analysis_binding.as_deref().map_or_else(
            || parse.program.clone(),
            |binding| analysis_target_program(&parse.program, binding),
        );
        let program = &analysis_program;
        let stdin_schema_hints = stdin_static_schema_hints(program);
        let mut schema_diagnostics = Vec::new();
        let mut analysis = analyze_program(program, |request| {
            let stdin_schema_hint = stdin_schema_hints
                .get(&span_key(request.load.span))
                .map(Vec::as_slice);
            match load_schema_for_request(
                request,
                &path,
                &base_dir,
                options.stdin_format.as_deref(),
                stdin_bytes.as_deref(),
                stdin_schema_hint,
                io,
            ) {
                Ok(schema) => Ok(schema),
                Err(diagnostic) => {
                    schema_diagnostics.push(diagnostic.clone());
                    Err(diagnostic)
                }
            }
        });
        report.extend(ReportPhase::SchemaFacts, schema_diagnostics.clone());
        report.extend(
            ReportPhase::Semantic,
            analysis
                .diagnostics
                .iter()
                .filter(|diagnostic| !schema_diagnostics.contains(diagnostic))
                .cloned(),
        );
        if parse_has_errors {
            analysis.ir = None;
        }
        analysis
    };
    PreparedProgram {
        origin,
        path,
        source,
        parse,
        analysis,
        driver_plan,
        report,
        stdin_format: options.stdin_format,
        stdin_bytes,
    }
}

fn parse_errors_allow_semantic_followup(diagnostics: &[Diagnostic]) -> bool {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == pdl_core::Severity::Error)
        .all(is_recoverable_parse_error_for_semantic_followup)
}

fn is_recoverable_parse_error_for_semantic_followup(diagnostic: &Diagnostic) -> bool {
    matches!(
        (diagnostic.code, diagnostic.message.as_str()),
        ("E0001", "expected operator in filter expression")
            | ("E0001", "expected `|` before stage")
            | ("E1213", "aggregate items require `as`")
    )
}

pub fn program(prepared: &PreparedProgram) -> &Program {
    &prepared.parse.program
}

fn load_schema_for_request(
    request: LoadRequest<'_>,
    program_path: &Path,
    base_dir: &Path,
    stdin_format: Option<&str>,
    stdin_bytes: Option<&[u8]>,
    stdin_schema_hint: Option<&[String]>,
    io: &dyn DriverIo,
) -> Result<Vec<String>, Diagnostic> {
    match &request.load.source {
        SourceRef::Path(path) => {
            let resolved = if base_dir == Path::new(".") {
                resolve_input_path(program_path, &path.value)
            } else {
                base_dir.join(&path.value)
            };
            let explicit = request
                .load
                .format
                .as_ref()
                .map(|format| (format.value.as_str(), format.span));
            if explicit.is_none()
                && DataFormat::infer_from_path(&path.value) == Some(DataFormat::Csv)
            {
                return io.read_csv_schema(&resolved);
            }
            let bytes = io.read_path_bytes(&resolved)?;
            let format = resolve_input_format(
                explicit,
                None,
                DataFormat::infer_from_path(&path.value),
                Some(&bytes),
                path.span,
            )?;
            pdl_data::read_schema_from_bytes(&resolved, format, &bytes)
        }
        SourceRef::Stdin(span) => {
            let Some(bytes) = stdin_bytes else {
                return Ok(stdin_schema_hint.unwrap_or_default().to_vec());
            };
            let explicit = request
                .load
                .format
                .as_ref()
                .map(|format| (format.value.as_str(), format.span));
            let format = resolve_input_format(explicit, stdin_format, None, Some(bytes), *span)?;
            pdl_data::read_schema_from_bytes(Path::new("stdin"), format, bytes)
        }
    }
}

fn resolve_input_format(
    explicit: Option<(&str, Span)>,
    cli_override: Option<&str>,
    inferred_from_path: Option<DataFormat>,
    bytes: Option<&[u8]>,
    span: Span,
) -> Result<DataFormat, Diagnostic> {
    if let Some((format, format_span)) = explicit {
        return DataFormat::from_name(format).ok_or_else(|| {
            Diagnostic::error(
                codes::E1215,
                format!("format `{format}` is not supported in 0.23.0"),
                format_span,
            )
        });
    }
    if let Some(format) = cli_override {
        return DataFormat::from_name(format).ok_or_else(|| {
            Diagnostic::error(
                codes::E1215,
                format!("format `{format}` is not supported in 0.23.0"),
                span,
            )
        });
    }
    if let Some(format) = inferred_from_path {
        return Ok(format);
    }
    if let Some(bytes) = bytes {
        return sniff_format_from_bytes(bytes);
    }
    Ok(DataFormat::Csv)
}

fn stdin_pre_read_diagnostics(program: &Program, stdin_format: &Option<String>) -> Vec<Diagnostic> {
    let cli_format = match stdin_format {
        Some(format) => match DataFormat::from_name(format) {
            Some(data_format) => Some((format.as_str(), data_format)),
            None => {
                return vec![Diagnostic::error(
                    codes::E1215,
                    format!("stdin format `{format}` is not supported in 0.23.0"),
                    Span::zero(),
                )];
            }
        },
        None => None,
    };
    let mut diagnostics = Vec::new();
    for pipeline in program_pipelines(program) {
        if let PipelineStart::Load(load) = &pipeline.start {
            if !matches!(&load.source, SourceRef::Stdin(_)) {
                continue;
            }
            if let Some(explicit) = &load.format {
                let Some(explicit_format) = DataFormat::from_name(&explicit.value) else {
                    diagnostics.push(Diagnostic::error(
                        codes::E1215,
                        format!("format `{}` is not supported in 0.23.0", explicit.value),
                        explicit.span,
                    ));
                    continue;
                };
                if let Some((cli_name, cli_format)) = cli_format {
                    if explicit_format != cli_format {
                        diagnostics.push(Diagnostic::error(
                            codes::E1217,
                            format!(
                                "load stdin format `{}` conflicts with --stdin-format `{cli_name}`",
                                explicit.value
                            ),
                            explicit.span,
                        ));
                    }
                }
            }
        }
    }
    diagnostics
}

fn stdin_static_schema_hints(program: &Program) -> BTreeMap<(usize, usize), Vec<String>> {
    let mut hints = BTreeMap::new();
    for pipeline in program_pipelines(program) {
        if let PipelineStart::Load(load) = &pipeline.start {
            if matches!(&load.source, SourceRef::Stdin(_)) {
                hints.insert(span_key(load.span), pipeline_schema_hint(pipeline));
            }
        }
    }
    hints
}

fn pipeline_schema_hint(pipeline: &Pipeline) -> Vec<String> {
    let mut columns = Vec::new();
    for stage in &pipeline.stages {
        match stage {
            Stage::Filter { expr, .. } => {
                collect_expr_columns(expr, ExprHintRole::PredicateRoot, &mut columns)
            }
            Stage::Select { items, .. } => {
                for item in items {
                    push_unique(&mut columns, &item.column.value);
                }
            }
            Stage::Drop {
                columns: stage_columns,
                ..
            }
            | Stage::GroupBy {
                columns: stage_columns,
                ..
            }
            | Stage::Distinct {
                columns: stage_columns,
                ..
            }
            | Stage::PivotLonger {
                columns: stage_columns,
                ..
            } => {
                for column in stage_columns {
                    push_unique(&mut columns, &column.value);
                }
            }
            Stage::Rename { items, .. } => {
                for item in items {
                    push_unique(&mut columns, &item.old.value);
                }
            }
            Stage::Mutate { items, .. } => {
                for item in items {
                    collect_expr_columns(&item.expr, ExprHintRole::Default, &mut columns);
                    push_unique(&mut columns, &item.column.value);
                }
            }
            Stage::Agg { items, .. } => {
                for item in items {
                    for arg in &item.args {
                        collect_expr_columns(arg, ExprHintRole::Default, &mut columns);
                    }
                }
            }
            Stage::Sort { items, .. } => {
                for item in items {
                    push_unique(&mut columns, &item.column.value);
                }
            }
            Stage::Join { on, .. } => {
                push_unique(&mut columns, &on.left().value);
            }
            Stage::Complete { keys, fills, .. } => {
                for key in keys {
                    push_unique(&mut columns, &key.value);
                }
                for fill in fills {
                    collect_expr_columns(&fill.expr, ExprHintRole::Default, &mut columns);
                    push_unique(&mut columns, &fill.column.value);
                }
            }
            Stage::Limit { .. }
            | Stage::Union { .. }
            | Stage::Save(_)
            | Stage::Unsupported { .. } => {}
        }
    }
    columns
}

#[derive(Clone, Copy)]
enum ExprHintRole {
    Default,
    PredicateRoot,
    ComparisonLeft,
    ComparisonRight,
}

fn collect_expr_columns(expr: &Expr, role: ExprHintRole, columns: &mut Vec<String>) {
    match expr {
        Expr::Quoted(value) => {
            if matches!(
                role,
                ExprHintRole::Default | ExprHintRole::PredicateRoot | ExprHintRole::ComparisonLeft
            ) {
                push_unique(columns, &value.value);
            }
        }
        Expr::Call { name, .. } if name.value == "lit" => {}
        Expr::Call { name, args, .. } if name.value == "col" => {
            if let Some(Expr::Quoted(value)) = args.first() {
                push_unique(columns, &value.value);
            }
        }
        Expr::Call { args, .. } => {
            for arg in args {
                collect_expr_columns(arg, ExprHintRole::Default, columns);
            }
        }
        Expr::Window { args, spec, .. } => {
            for arg in args {
                collect_expr_columns(arg, ExprHintRole::Default, columns);
            }
            for column in &spec.partition_by {
                push_unique(columns, &column.value);
            }
            for item in &spec.order_by {
                push_unique(columns, &item.column.value);
            }
        }
        Expr::Unary { expr, .. } => collect_expr_columns(expr, ExprHintRole::Default, columns),
        Expr::Binary {
            left, op, right, ..
        } if is_comparison_op(*op) => {
            collect_expr_columns(left, ExprHintRole::ComparisonLeft, columns);
            collect_expr_columns(right, ExprHintRole::ComparisonRight, columns);
        }
        Expr::Binary { left, right, .. } => {
            collect_expr_columns(left, ExprHintRole::Default, columns);
            collect_expr_columns(right, ExprHintRole::Default, columns);
        }
        Expr::Number(_) | Expr::Bool(_) | Expr::Null(_) | Expr::Ident(_) => {}
    }
}

fn is_comparison_op(op: BinaryOp) -> bool {
    matches!(
        op,
        BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Lte | BinaryOp::Gt | BinaryOp::Gte
    )
}

fn push_unique(columns: &mut Vec<String>, column: &str) {
    if !columns.iter().any(|existing| existing == column) {
        columns.push(column.to_string());
    }
}

fn span_key(span: Span) -> (usize, usize) {
    (span.start, span.end)
}

fn program_pipelines(program: &Program) -> Vec<&Pipeline> {
    let mut pipelines: Vec<&Pipeline> = program
        .bindings
        .iter()
        .map(|binding: &Binding| &binding.pipeline)
        .collect();
    pipelines.extend(program.outputs.iter().map(|output| &output.pipeline));
    if let Some(main) = &program.main {
        pipelines.push(main);
    }
    pipelines
}

fn analysis_target_program(program: &Program, binding: &str) -> Program {
    let span = program
        .bindings
        .iter()
        .find(|candidate| candidate.name.value == binding)
        .map_or_else(Span::zero, |candidate| candidate.name.span);
    Program {
        bindings: program.bindings.clone(),
        outputs: Vec::new(),
        main: Some(Pipeline {
            start: PipelineStart::Binding(Spanned::new(binding.to_string(), span)),
            stages: Vec::new(),
            span,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InMemoryDriverIo;

    #[test]
    fn in_memory_io_prepares_report_and_analysis() {
        let io = InMemoryDriverIo::default().with_schema("memory/sales.csv", ["amount", "region"]);
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "sales.csv" | select "region""#,
            &io,
        );

        assert!(
            prepared.diagnostics().is_empty(),
            "{:?}",
            prepared.diagnostics()
        );
        assert!(prepared.analysis.ir.is_some());
        assert_eq!(prepared.report.diagnostics.len(), 0);
    }

    #[test]
    fn preparation_keeps_phase_order_for_parse_diagnostics() {
        let io = InMemoryDriverIo::default();
        let prepared = prepare_source_with_io("memory/bad.pdl", r#"load "sales.csv" |"#, &io);

        assert_eq!(prepared.report.diagnostics[0].phase, ReportPhase::Parse);
        assert_eq!(prepared.report.diagnostics[0].diagnostic.code, "E0006");
        assert_eq!(
            prepared.report.phase_order(),
            &[
                ReportPhase::Parse,
                ReportPhase::SourceResolution,
                ReportPhase::SchemaFacts,
                ReportPhase::Semantic,
                ReportPhase::Planning,
                ReportPhase::Execution,
                ReportPhase::Output,
            ]
        );
    }

    #[test]
    fn recoverable_filter_operator_errors_keep_column_diagnostics() {
        let io = InMemoryDriverIo::default().with_schema(
            "memory/sales.csv",
            ["region", "status", "amount", "customer_age"],
        );
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "sales.csv"
  | filter "staus" = "completed"
  | group_by "region"
  | agg sum("amount") as "total_revenue", mean("customer_age") as "avg_age", count() as "orders"
  | sort "total_revenue" desc
  | limit 3"#,
            &io,
        );
        let diagnostics = prepared.diagnostics();

        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E0001"));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E1005" && diagnostic.message == "unknown column `staus`"
        }));
        assert!(prepared.analysis.ir.is_none());
    }

    #[test]
    fn multiple_recoverable_parse_errors_keep_column_diagnostics_without_alias_noise() {
        let io = InMemoryDriverIo::default().with_schema(
            "memory/sales.csv",
            ["region", "status", "amount", "customer_age"],
        );
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "sales.csv"
  | filter "staus" = "completed"
  | group_by "region"
  | agg sum("amount") a "total_revenue", mean("customer_age") as "avg_age", count() as "orders"
  | sort "total_revenue" desc
  | limit 3"#,
            &io,
        );
        let diagnostics = prepared.diagnostics();

        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E0001"));
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E1213"));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E1005" && diagnostic.message == "unknown column `staus`"
        }));
        assert!(!diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E0009"));
        assert!(prepared.analysis.ir.is_none());
    }

    #[test]
    fn missing_pipe_before_stage_keeps_column_diagnostics() {
        let io = InMemoryDriverIo::default().with_schema(
            "memory/sales.csv",
            ["region", "status", "amount", "customer_age"],
        );
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "sales.csv"
  filter "staus" == "completed"
  | group_by "region"
  | agg sum("amount") as "total_revenue", mean("customer_age") as "avg_age", count() as "orders"
  | sort "total_revenue" desc
  | limit 3"#,
            &io,
        );
        let diagnostics = prepared.diagnostics();

        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E0001" && diagnostic.message == "expected `|` before stage"
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E1005" && diagnostic.message == "unknown column `staus`"
        }));
        assert!(!diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E0021"));
        assert!(prepared.analysis.ir.is_none());
    }

    #[test]
    fn preparation_records_driver_plan_without_reading_stdin_bytes() {
        let io = InMemoryDriverIo::default().with_stdin_bytes("status\ncompleted\n");
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load stdin format "csv" | save stdout format "csv""#,
            &io,
        );

        assert_eq!(prepared.driver_plan.stdin_reads().len(), 1);
        assert_eq!(prepared.driver_plan.stdout_writes().len(), 1);
        assert!(!prepared
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == "E1211"));
    }

    #[test]
    fn static_preparation_accepts_stdin_pipeline_without_stdin_bytes() {
        let io = InMemoryDriverIo::default();
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load stdin format "csv"
  | filter "status" == "completed"
  | select "order_id", "region", "amount"
  | sort "order_id""#,
            &io,
        );

        assert!(
            prepared.diagnostics().is_empty(),
            "{:?}",
            prepared.diagnostics()
        );
        assert!(prepared.analysis.ir.is_some());
    }
}
