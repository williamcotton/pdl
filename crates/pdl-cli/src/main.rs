mod args;
mod diagnostics;
mod handlers;

use std::process::ExitCode;

fn main() -> ExitCode {
    match handlers::run_cli() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}
