use pdl_core::{has_errors, Diagnostic};
use pdl_data::read_csv_schema;
use pdl_semantics::{analyze_program, Analysis, LoadRequest};
use pdl_syntax::{parse, ParseResult, Program, SourceRef};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct PreparedProgram {
    pub path: PathBuf,
    pub source: String,
    pub parse: ParseResult,
    pub analysis: Analysis,
}

impl PreparedProgram {
    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        let mut diagnostics = self.parse.diagnostics.clone();
        diagnostics.extend(self.analysis.diagnostics.clone());
        diagnostics
    }

    pub fn has_errors(&self) -> bool {
        has_errors(&self.diagnostics())
    }
}

pub fn prepare_file(path: impl AsRef<Path>) -> Result<PreparedProgram, Diagnostic> {
    let path = path.as_ref().to_path_buf();
    let source = fs::read_to_string(&path).map_err(|error| {
        Diagnostic::error(
            "P1802",
            format!("could not read PDL file `{}`: {error}", path.display()),
            pdl_core::Span::zero(),
        )
    })?;
    Ok(prepare_source(path, source))
}

pub fn prepare_source(path: impl AsRef<Path>, source: impl Into<String>) -> PreparedProgram {
    let path = path.as_ref().to_path_buf();
    let source = source.into();
    let parse = parse(&source);
    let base_dir = path
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let analysis = if has_errors(&parse.diagnostics) {
        Analysis::default()
    } else {
        let program = &parse.program;
        analyze_program(program, |request| {
            load_schema_for_request(request, &base_dir)
        })
    };

    PreparedProgram {
        path,
        source,
        parse,
        analysis,
    }
}

pub fn resolve_input_path(program_path: &Path, source: &str) -> PathBuf {
    let path = PathBuf::from(source);
    if path.is_absolute() {
        path
    } else {
        program_path
            .parent()
            .map_or_else(|| PathBuf::from(source), |parent| parent.join(source))
    }
}

pub fn resolve_output_path(source: &str) -> PathBuf {
    PathBuf::from(source)
}

pub fn program(prepared: &PreparedProgram) -> &Program {
    &prepared.parse.program
}

fn load_schema_for_request(
    request: LoadRequest<'_>,
    base_dir: &Path,
) -> Result<Vec<String>, Diagnostic> {
    match &request.load.source {
        SourceRef::Path(path) => {
            if let Some(format) = &request.load.format {
                if format.value != "csv" {
                    return Err(Diagnostic::error(
                        "P1215",
                        format!("format `{}` is not supported in 0.2.0", format.value),
                        format.span,
                    ));
                }
            } else if !path.value.ends_with(".csv") {
                return Err(Diagnostic::error(
                    "P1216",
                    format!("could not infer supported format for `{}`", path.value),
                    path.span,
                ));
            }
            read_csv_schema(&base_dir.join(&path.value))
        }
        SourceRef::Stdin(span) => Err(Diagnostic::error(
            "P1211",
            "stdin loading is deferred in 0.2.0",
            *span,
        )),
    }
}
