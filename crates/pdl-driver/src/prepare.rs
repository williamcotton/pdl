use pdl_core::{codes, has_errors, Diagnostic};
use pdl_data::DataFormat;
use pdl_semantics::{analyze_program, Analysis, LoadRequest};
use pdl_syntax::{parse, ParseResult, Program, SourceRef};
use std::path::{Path, PathBuf};

use crate::io::{DriverIo, OsDriverIo};
use crate::path::resolve_input_path;
use crate::report::{PreparationReport, ReportPhase};
use crate::source::SourceOrigin;

#[derive(Clone, Debug)]
pub struct PreparedProgram {
    pub path: PathBuf,
    pub origin: SourceOrigin,
    pub source: String,
    pub parse: ParseResult,
    pub analysis: Analysis,
    pub report: PreparationReport,
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
    Ok(prepare_source_with_io(path, source, &io))
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
    let path = path.as_ref().to_path_buf();
    let source = source.into();
    let parse = parse(&source);
    let mut report = PreparationReport::default();
    report.extend(ReportPhase::Parse, parse.diagnostics.clone());

    let base_dir = path
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let analysis = if has_errors(&parse.diagnostics) {
        Analysis::default()
    } else {
        let program = &parse.program;
        analyze_program(program, |request| {
            load_schema_for_request(request, &path, &base_dir, io)
        })
    };
    report.extend(ReportPhase::Semantic, analysis.diagnostics.clone());

    PreparedProgram {
        origin: SourceOrigin::path(path.clone()),
        path,
        source,
        parse,
        analysis,
        report,
    }
}

pub fn program(prepared: &PreparedProgram) -> &Program {
    &prepared.parse.program
}

fn load_schema_for_request(
    request: LoadRequest<'_>,
    program_path: &Path,
    base_dir: &Path,
    io: &dyn DriverIo,
) -> Result<Vec<String>, Diagnostic> {
    match &request.load.source {
        SourceRef::Path(path) => {
            if let Some(format) = &request.load.format {
                if format.value != "csv" {
                    return Err(Diagnostic::error(
                        codes::E1215,
                        format!("format `{}` is not supported in 0.4.0", format.value),
                        format.span,
                    ));
                }
            } else if DataFormat::infer_from_path(&path.value) != Some(DataFormat::Csv) {
                return Err(Diagnostic::error(
                    codes::E1216,
                    format!("could not infer supported format for `{}`", path.value),
                    path.span,
                ));
            }
            let resolved = if base_dir == Path::new(".") {
                resolve_input_path(program_path, &path.value)
            } else {
                base_dir.join(&path.value)
            };
            io.read_csv_schema(&resolved)
        }
        SourceRef::Stdin(span) => Err(Diagnostic::error(
            codes::E1211,
            "stdin loading is deferred in 0.4.0",
            *span,
        )),
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
    }
}
