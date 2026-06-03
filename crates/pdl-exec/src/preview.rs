use pdl_data::Table;

#[derive(Clone, Debug, PartialEq)]
pub struct TablePreview {
    pub columns: Vec<String>,
    pub rows: usize,
}

impl From<&Table> for TablePreview {
    fn from(table: &Table) -> Self {
        Self {
            columns: table.columns.clone(),
            rows: table.rows.len(),
        }
    }
}
