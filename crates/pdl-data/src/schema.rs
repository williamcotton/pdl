#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ColumnSchema {
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableSchema {
    pub columns: Vec<ColumnSchema>,
}

impl TableSchema {
    pub fn from_column_names(columns: impl IntoIterator<Item = String>) -> Self {
        Self {
            columns: columns
                .into_iter()
                .map(|name| ColumnSchema { name })
                .collect(),
        }
    }

    pub fn column_names(&self) -> Vec<String> {
        self.columns
            .iter()
            .map(|column| column.name.clone())
            .collect()
    }
}
