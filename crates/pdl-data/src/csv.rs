use pdl_core::{codes, Diagnostic, Span};
use std::fs::File;
use std::io::{BufReader, BufWriter, Cursor, Read, Write};
use std::path::Path;

use crate::{Row, Table, Value};

pub fn read_csv_schema(path: &Path) -> Result<Vec<String>, Diagnostic> {
    let file = File::open(path).map_err(|error| {
        Diagnostic::error(
            codes::E1801,
            format!(
                "source file `{}` could not be opened: {error}",
                path.display()
            ),
            Span::zero(),
        )
    })?;
    read_csv_schema_from_reader(path, BufReader::new(file))
}

pub fn read_csv_schema_from_bytes(path: &Path, bytes: &[u8]) -> Result<Vec<String>, Diagnostic> {
    read_csv_schema_from_reader(path, Cursor::new(bytes))
}

fn read_csv_schema_from_reader<R: Read>(path: &Path, reader: R) -> Result<Vec<String>, Diagnostic> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(reader);
    let headers = reader.headers().map_err(|error| {
        Diagnostic::error(
            codes::E1804,
            format!("CSV header parse failed for `{}`: {error}", path.display()),
            Span::zero(),
        )
    })?;
    Ok(headers.iter().map(str::to_string).collect())
}

pub fn read_csv(path: &Path) -> Result<Table, Diagnostic> {
    let file = File::open(path).map_err(|error| {
        Diagnostic::error(
            codes::E1801,
            format!(
                "source file `{}` could not be opened: {error}",
                path.display()
            ),
            Span::zero(),
        )
    })?;
    read_csv_from_reader(path, BufReader::new(file))
}

pub fn read_csv_from_bytes(path: &Path, bytes: &[u8]) -> Result<Table, Diagnostic> {
    read_csv_from_reader(path, Cursor::new(bytes))
}

fn read_csv_from_reader<R: Read>(path: &Path, reader: R) -> Result<Table, Diagnostic> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(reader);
    let headers = reader.headers().map_err(|error| {
        Diagnostic::error(
            codes::E1804,
            format!("CSV header parse failed for `{}`: {error}", path.display()),
            Span::zero(),
        )
    })?;
    let columns: Vec<String> = headers.iter().map(str::to_string).collect();
    let mut rows = Vec::new();

    for record in reader.records() {
        let record = record.map_err(|error| {
            Diagnostic::error(
                codes::E1804,
                format!("CSV row parse failed for `{}`: {error}", path.display()),
                Span::zero(),
            )
        })?;
        rows.push(Row {
            values: record.iter().map(Value::parse_csv_cell).collect(),
        });
    }

    Ok(Table { columns, rows })
}

pub fn write_csv(path: &Path, table: &Table) -> Result<(), Diagnostic> {
    let file = File::create(path).map_err(|error| {
        Diagnostic::error(
            codes::E1704,
            format!(
                "output file `{}` could not be created: {error}",
                path.display()
            ),
            Span::zero(),
        )
    })?;
    write_csv_to_writer(BufWriter::new(file), table)
}

pub fn write_csv_to_vec(table: &Table) -> Result<Vec<u8>, Diagnostic> {
    let mut bytes = Vec::new();
    write_csv_to_writer(&mut bytes, table)?;
    Ok(bytes)
}

fn write_csv_to_writer<W: Write>(writer: W, table: &Table) -> Result<(), Diagnostic> {
    let mut writer = csv::WriterBuilder::new()
        .has_headers(true)
        .terminator(csv::Terminator::Any(b'\n'))
        .from_writer(writer);
    writer.write_record(&table.columns).map_err(|error| {
        Diagnostic::error(
            codes::E1704,
            format!("CSV header write failed: {error}"),
            Span::zero(),
        )
    })?;
    for row in &table.rows {
        let record: Vec<String> = row.values.iter().map(Value::to_csv_cell).collect();
        writer.write_record(record).map_err(|error| {
            Diagnostic::error(
                codes::E1704,
                format!("CSV row write failed: {error}"),
                Span::zero(),
            )
        })?;
    }
    writer.flush().map_err(|error| {
        Diagnostic::error(
            codes::E1704,
            format!("CSV flush failed: {error}"),
            Span::zero(),
        )
    })
}
