use clap::Parser;
use pdl_core::has_errors;
use pdl_driver::{
    prepare_file, prepare_file_for_binding_schema, prepare_file_for_run, prepare_file_with_options,
    PrepareOptions,
};
use pdl_exec::{plan_prepared, planning::PlanningOptions, run_prepared, RunOptions};
use serde::Serialize;
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
        } => {
            let prepared = match prepare_file_for_run(&file, stdin_format) {
                Ok(prepared) => prepared,
                Err(diagnostic) => {
                    eprintln!("{}", diagnostic.message);
                    return Ok(ExitCode::from(1));
                }
            };
            let result = run_prepared(
                &prepared,
                RunOptions {
                    stdout_format,
                    dry_run,
                    allow_binary_stdout: true,
                },
            );
            print_diagnostics(
                &prepared.path.display().to_string(),
                &prepared.source,
                &result.diagnostics,
            );
            if has_errors(&result.diagnostics) {
                return Ok(ExitCode::from(1));
            }
            if let Some(stdout) = result.stdout {
                io::stdout()
                    .write_all(&stdout)
                    .map_err(|error| format!("stdout write failed: {error}"))?;
            }
            Ok(ExitCode::SUCCESS)
        }
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
                "pdl {} (language draft 0.25.0, data engine {})",
                env!("CARGO_PKG_VERSION"),
                pdl_data::native_engine_name()
            );
            Ok(ExitCode::SUCCESS)
        }
    }
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
