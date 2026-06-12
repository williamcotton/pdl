use clap::{Parser, Subcommand, ValueEnum};
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
        stdin_format: Option<String>,
        #[arg(long)]
        stdout_format: Option<String>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long = "context")]
        context: Vec<String>,
        #[arg(long, value_enum, default_value_t = EngineArg::Auto)]
        engine: EngineArg,
    },
    Controls {
        file: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long = "context")]
        context: Vec<String>,
    },
    Serve {
        file: PathBuf,
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 0)]
        port: u16,
    },
    Check {
        file: PathBuf,
    },
    Fmt {
        file: PathBuf,
        #[arg(long)]
        check: bool,
    },
    Schema {
        file: PathBuf,
        #[arg(long)]
        binding: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Plan {
        file: PathBuf,
        #[arg(long)]
        stdin_format: Option<String>,
        #[arg(long)]
        stdout_format: Option<String>,
        #[arg(long, value_enum, default_value_t = EngineArg::Auto)]
        engine: EngineArg,
        #[arg(long)]
        json: bool,
    },
    Ast {
        file: PathBuf,
    },
    Ir {
        file: PathBuf,
    },
    Manifest {
        file: PathBuf,
        #[arg(long)]
        stdin_format: Option<String>,
        #[arg(long)]
        stdout_format: Option<String>,
        #[arg(long, value_enum, default_value_t = EngineArg::Auto)]
        engine: EngineArg,
    },
    Init {
        #[arg(default_value = ".")]
        dir: PathBuf,
        #[arg(long)]
        codex: bool,
        #[arg(long)]
        claude: bool,
        #[arg(long)]
        agy: bool,
    },
    Lsp,
    Version,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum EngineArg {
    Auto,
    Row,
    RowStrict,
    Native,
    NativeStrict,
}
