use pdl_core::Diagnostic;
use pdl_data::{write_table_to_bytes, write_table_to_path, DataFormat, Table};
use std::path::Path;

pub fn emit_stdout(format: DataFormat, table: &Table) -> Result<Vec<u8>, Diagnostic> {
    write_table_to_bytes(format, table)
}

pub fn write_output(path: &Path, format: DataFormat, table: &Table) -> Result<(), Diagnostic> {
    write_table_to_path(path, format, table)
}
