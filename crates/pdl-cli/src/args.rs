use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "pdl", version, about = "Pipeline Data Language")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
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
