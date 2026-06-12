use clap::Parser;
use pdl_core::has_errors;
use pdl_data::Value;
use pdl_driver::{
    prepare_file, prepare_file_for_binding_schema, prepare_file_for_run, prepare_file_with_options,
    OsDriverIo, PrepareOptions,
};
use pdl_exec::{
    plan_prepared, planning::PlanningOptions, run_prepared_with_engine,
    run_prepared_with_io_and_context_and_engine, ExecutionEngine, PlannedEngine, RunOptions,
};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

use crate::args::{Cli, Command};
use crate::diagnostics::print_diagnostics;
use crate::render::{
    ast_json, final_schema_columns, ir_json, manifest_json, plan_json, render_plan_text,
    render_schema_text, schema_json,
};

pub fn run_cli() -> Result<ExitCode, String> {
    let cli = Cli::parse();
    match cli.command {
        Command::Run {
            file,
            stdin_format,
            stdout_format,
            dry_run,
            context,
            engine,
        } => {
            let context = parse_context_overrides(&context)?;
            let prepared = match prepare_file_for_run(&file, stdin_format) {
                Ok(prepared) => prepared,
                Err(diagnostic) => {
                    eprintln!("{}", diagnostic.message);
                    return Ok(ExitCode::from(1));
                }
            };
            let io = OsDriverIo;
            let result = if context.is_empty() {
                run_prepared_with_engine(
                    &prepared,
                    RunOptions {
                        stdout_format,
                        dry_run,
                        allow_binary_stdout: true,
                    },
                    engine.into(),
                )
            } else {
                run_prepared_with_io_and_context_and_engine(
                    &prepared,
                    RunOptions {
                        stdout_format,
                        dry_run,
                        allow_binary_stdout: true,
                    },
                    &io,
                    context,
                    engine.into(),
                )
            };
            print_diagnostics(
                &prepared.path.display().to_string(),
                &prepared.source,
                &result.diagnostics,
            );
            if has_errors(&result.diagnostics) {
                return Ok(ExitCode::from(1));
            }
            if engine == crate::args::EngineArg::RowStrict
                && result.backend != pdl_data::DataBackend::PortableRows
            {
                eprintln!(
                    "--engine row-strict requires portable row execution, but the run \
                     reported backend `{:?}`",
                    result.backend
                );
                return Ok(ExitCode::from(1));
            }
            if let Some(stdout) = result.stdout {
                io::stdout()
                    .write_all(&stdout)
                    .map_err(|error| format!("stdout write failed: {error}"))?;
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Controls {
            file,
            json,
            context,
        } => {
            if !json {
                eprintln!("`pdl controls` currently requires `--json`");
                return Ok(ExitCode::from(1));
            }
            let context = parse_context_overrides(&context)?;
            let prepared = match prepare_file(&file) {
                Ok(prepared) => prepared,
                Err(diagnostic) => {
                    eprintln!("{}", diagnostic.message);
                    return Ok(ExitCode::from(1));
                }
            };
            let output = crate::render::controls_json(&prepared, context);
            let diagnostics = output.diagnostics().to_vec();
            print_json(&output)?;
            if has_errors(&diagnostics) {
                Ok(ExitCode::from(1))
            } else {
                Ok(ExitCode::SUCCESS)
            }
        }
        Command::Serve { file, host, port } => crate::serve::serve(file, host, port),
        Command::Check { file } => {
            let prepared = match prepare_file(&file) {
                Ok(prepared) => prepared,
                Err(diagnostic) => {
                    eprintln!("{}", diagnostic.message);
                    return Ok(ExitCode::from(1));
                }
            };
            let diagnostics = prepared.diagnostics();
            print_diagnostics(
                &prepared.path.display().to_string(),
                &prepared.source,
                &diagnostics,
            );
            if has_errors(&diagnostics) {
                Ok(ExitCode::from(1))
            } else {
                Ok(ExitCode::SUCCESS)
            }
        }
        Command::Fmt { file, check } => handle_fmt(&file, check),
        Command::Schema {
            file,
            binding,
            json,
        } => {
            let prepared = match &binding {
                Some(binding) => match prepare_file_for_binding_schema(&file, binding) {
                    Ok(prepared) => prepared,
                    Err(diagnostic) => {
                        eprintln!("{}", diagnostic.message);
                        return Ok(ExitCode::from(1));
                    }
                },
                None => match prepare_file(&file) {
                    Ok(prepared) => prepared,
                    Err(diagnostic) => {
                        eprintln!("{}", diagnostic.message);
                        return Ok(ExitCode::from(1));
                    }
                },
            };
            let diagnostics = prepared.diagnostics();
            print_diagnostics(
                &prepared.path.display().to_string(),
                &prepared.source,
                &diagnostics,
            );
            if has_errors(&diagnostics) {
                return Ok(ExitCode::from(1));
            }
            let Some(columns) = final_schema_columns(&prepared) else {
                eprintln!("schema is unavailable");
                return Ok(ExitCode::from(1));
            };
            if json {
                print_json(&schema_json(&prepared, binding.as_deref()))?;
            } else {
                print!("{}", render_schema_text(&columns));
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Plan {
            file,
            stdin_format,
            stdout_format,
            engine,
            json,
        } => {
            let prepared = match prepare_for_planning(&file, stdin_format) {
                Ok(prepared) => prepared,
                Err(diagnostic) => {
                    eprintln!("{}", diagnostic.message);
                    return Ok(ExitCode::from(1));
                }
            };
            let plan = match plan_prepared(
                &prepared,
                PlanningOptions {
                    stdout_format,
                    dry_run: true,
                    allow_binary_stdout: true,
                    engine: engine.into(),
                },
            ) {
                Ok(plan) => plan,
                Err(diagnostics) => {
                    print_diagnostics(
                        &prepared.path.display().to_string(),
                        &prepared.source,
                        &diagnostics,
                    );
                    return Ok(ExitCode::from(1));
                }
            };
            let diagnostics = prepared.diagnostics();
            print_diagnostics(
                &prepared.path.display().to_string(),
                &prepared.source,
                &diagnostics,
            );
            if json {
                print_json(&plan_json(&prepared, &plan))?;
            } else {
                print!("{}", render_plan_text(&prepared, &plan));
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Ast { file } => {
            let source = read_source(&file)?;
            let parse = pdl_syntax::parse(&source);
            print_diagnostics(&file.display().to_string(), &source, &parse.diagnostics);
            if has_errors(&parse.diagnostics) {
                return Ok(ExitCode::from(1));
            }
            print_json(&ast_json(
                file.display().to_string(),
                &parse.program,
                parse.diagnostics,
            ))?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Ir { file } => {
            let prepared = match prepare_file(&file) {
                Ok(prepared) => prepared,
                Err(diagnostic) => {
                    eprintln!("{}", diagnostic.message);
                    return Ok(ExitCode::from(1));
                }
            };
            let diagnostics = prepared.diagnostics();
            print_diagnostics(
                &prepared.path.display().to_string(),
                &prepared.source,
                &diagnostics,
            );
            if has_errors(&diagnostics) {
                return Ok(ExitCode::from(1));
            }
            let Some(ir) = prepared.analysis.ir.as_ref() else {
                eprintln!("semantic IR is unavailable");
                return Ok(ExitCode::from(1));
            };
            print_json(&ir_json(
                prepared.path.display().to_string(),
                ir,
                diagnostics,
            ))?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Manifest {
            file,
            stdin_format,
            stdout_format,
            engine,
        } => {
            let prepared = match prepare_for_planning(&file, stdin_format) {
                Ok(prepared) => prepared,
                Err(diagnostic) => {
                    eprintln!("{}", diagnostic.message);
                    return Ok(ExitCode::from(1));
                }
            };
            let plan = match plan_prepared(
                &prepared,
                PlanningOptions {
                    stdout_format,
                    dry_run: true,
                    allow_binary_stdout: true,
                    engine: engine.into(),
                },
            ) {
                Ok(plan) => plan,
                Err(diagnostics) => {
                    print_diagnostics(
                        &prepared.path.display().to_string(),
                        &prepared.source,
                        &diagnostics,
                    );
                    return Ok(ExitCode::from(1));
                }
            };
            let diagnostics = prepared.diagnostics();
            print_diagnostics(
                &prepared.path.display().to_string(),
                &prepared.source,
                &diagnostics,
            );
            print_json(&manifest_json(&prepared, &plan))?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Init {
            dir,
            codex,
            claude,
            agy,
        } => {
            for action in crate::init::init_agent_files(&dir, codex, claude, agy)? {
                println!("{action}");
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Lsp => {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| format!("failed to start Tokio runtime: {error}"))?;
            runtime.block_on(pdl_lsp::run_stdio());
            Ok(ExitCode::SUCCESS)
        }
        Command::Version => {
            println!(
                "pdl {} (language draft 0.52.0, data engine {})",
                env!("CARGO_PKG_VERSION"),
                pdl_data::native_engine_name()
            );
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn parse_context_overrides(items: &[String]) -> Result<BTreeMap<String, Value>, String> {
    let mut values = BTreeMap::new();
    for item in items {
        let Some((name, raw_value)) = item.split_once('=') else {
            return Err(format!("context override `{item}` must be `name=value`"));
        };
        if name.is_empty() {
            return Err(format!("context override `{item}` has an empty name"));
        }
        values.insert(name.to_string(), parse_context_value(raw_value)?);
    }
    Ok(values)
}

fn parse_context_value(raw: &str) -> Result<Value, String> {
    if raw == "null" {
        return Ok(Value::Null);
    }
    if raw == "true" {
        return Ok(Value::Bool(true));
    }
    if raw == "false" {
        return Ok(Value::Bool(false));
    }
    if raw.starts_with('"') {
        return serde_json::from_str::<String>(raw)
            .map(Value::String)
            .map_err(|error| format!("invalid quoted context string `{raw}`: {error}"));
    }
    if let Ok(number) = raw.parse::<f64>() {
        return Ok(Value::Number(number));
    }
    Ok(Value::String(raw.to_string()))
}

fn handle_fmt(file: &Path, check: bool) -> Result<ExitCode, String> {
    let source = read_source(file)?;
    let formatted = match pdl_syntax::format_source(&source) {
        Some(formatted) => formatted,
        None => {
            let parse = pdl_syntax::parse(&source);
            print_diagnostics(&file.display().to_string(), &source, &parse.diagnostics);
            if has_errors(&parse.diagnostics) {
                return Ok(ExitCode::from(1));
            }
            eprintln!(
                "formatter cannot safely rewrite `{}` because comments are present",
                file.display()
            );
            return Ok(ExitCode::from(1));
        }
    };
    let formatted = format!("{formatted}\n");
    if check {
        if source == formatted {
            Ok(ExitCode::SUCCESS)
        } else {
            eprintln!("{} is not formatted", file.display());
            Ok(ExitCode::from(1))
        }
    } else {
        fs::write(file, formatted)
            .map_err(|error| format!("could not write `{}`: {error}", file.display()))?;
        Ok(ExitCode::SUCCESS)
    }
}

fn prepare_for_planning(
    file: &Path,
    stdin_format: Option<String>,
) -> Result<pdl_driver::PreparedProgram, pdl_core::Diagnostic> {
    prepare_file_with_options(
        file,
        PrepareOptions {
            stdin_format,
            read_stdin: false,
            analysis_binding: None,
        },
    )
}

fn read_source(file: &Path) -> Result<String, String> {
    fs::read_to_string(file)
        .map_err(|error| format!("could not read `{}`: {error}", file.display()))
}

fn print_json(value: &impl Serialize) -> Result<(), String> {
    let stdout = io::stdout();
    let mut lock = stdout.lock();
    serde_json::to_writer_pretty(&mut lock, value)
        .map_err(|error| format!("json serialization failed: {error}"))?;
    writeln!(lock).map_err(|error| format!("stdout write failed: {error}"))
}

impl From<crate::args::EngineArg> for ExecutionEngine {
    fn from(value: crate::args::EngineArg) -> Self {
        match value {
            crate::args::EngineArg::Auto => ExecutionEngine::Auto,
            crate::args::EngineArg::Row => ExecutionEngine::Row,
            crate::args::EngineArg::RowStrict => ExecutionEngine::RowStrict,
            crate::args::EngineArg::Native => ExecutionEngine::Native,
            crate::args::EngineArg::NativeStrict => ExecutionEngine::NativeStrict,
        }
    }
}

impl From<crate::args::EngineArg> for PlannedEngine {
    fn from(value: crate::args::EngineArg) -> Self {
        match value {
            crate::args::EngineArg::Auto => PlannedEngine::Auto,
            crate::args::EngineArg::Row => PlannedEngine::Row,
            crate::args::EngineArg::RowStrict => PlannedEngine::RowStrict,
            crate::args::EngineArg::Native => PlannedEngine::Native,
            crate::args::EngineArg::NativeStrict => PlannedEngine::NativeStrict,
        }
    }
}
