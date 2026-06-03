use clap::{Parser, Subcommand};
use pdl_core::{has_errors, render_diagnostic};
use pdl_driver::prepare_file;
use pdl_exec::{run_prepared, RunOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(name = "pdl", version, about = "Pipeline Data Language")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Run {
        file: PathBuf,
        #[arg(long)]
        stdout_format: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
    Check {
        file: PathBuf,
    },
    Lsp,
    Version,
}

fn main() -> ExitCode {
    match run_cli() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

fn run_cli() -> Result<ExitCode, String> {
    let cli = Cli::parse();
    match cli.command {
        Command::Run {
            file,
            stdout_format,
            dry_run,
        } => {
            let prepared = match prepare_file(&file) {
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
                "pdl {} (language draft 0.3.0, data engine {})",
                env!("CARGO_PKG_VERSION"),
                pdl_data::native_engine_name()
            );
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn print_diagnostics(source_name: &str, source: &str, diagnostics: &[pdl_core::Diagnostic]) {
    for diagnostic in diagnostics {
        eprintln!("{}", render_diagnostic(source_name, source, diagnostic));
    }
}
