use clap::Parser;
use pdl_core::has_errors;
use pdl_driver::{prepare_file, prepare_file_for_run};
use pdl_exec::{run_prepared, RunOptions};
use std::io::{self, Write};
use std::process::ExitCode;

use crate::args::{Cli, Command};
use crate::diagnostics::print_diagnostics;

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
                "pdl {} (language draft 0.13.0, data engine {})",
                env!("CARGO_PKG_VERSION"),
                pdl_data::native_engine_name()
            );
            Ok(ExitCode::SUCCESS)
        }
    }
}
