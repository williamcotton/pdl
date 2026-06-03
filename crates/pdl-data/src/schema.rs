use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LogicalType {
    String,
    Bool,
    Int,
    Number,
    Decimal,
    Date,
    Time,
    DateTime,
    Duration,
    Binary,
    Null,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ColumnSchema {
    pub name: String,
    pub logical_type: LogicalType,
    pub nullable: bool,
}

impl ColumnSchema {
    pub fn unknown(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            logical_type: LogicalType::Unknown,
            nullable: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TableSchema {
    pub columns: Vec<ColumnSchema>,
}

impl TableSchema {
    pub fn from_column_names(columns: impl IntoIterator<Item = String>) -> Self {
        Self {
            columns: columns.into_iter().map(ColumnSchema::unknown).collect(),
        }
    }

    pub fn column_names(&self) -> Vec<String> {
        self.columns
            .iter()
            .map(|column| column.name.clone())
            .collect()
    }
}
