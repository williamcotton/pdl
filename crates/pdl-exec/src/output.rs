use pdl_core::Diagnostic;
use pdl_data::{write_csv, write_csv_to_vec, Table};
use std::path::Path;

pub fn emit_csv_stdout(table: &Table) -> Result<Vec<u8>, Diagnostic> {
    write_csv_to_vec(table)
}

pub fn write_csv_output(path: &Path, table: &Table) -> Result<(), Diagnostic> {
    write_csv(path, table)
}
