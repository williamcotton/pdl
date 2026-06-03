use crate::format::format_number;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
}

impl Value {
    pub fn parse_csv_cell(cell: &str) -> Self {
        if cell.is_empty() {
            Self::Null
        } else if cell == "true" {
            Self::Bool(true)
        } else if cell == "false" {
            Self::Bool(false)
        } else if let Ok(number) = cell.parse::<f64>() {
            Self::Number(number)
        } else {
            Self::String(cell.to_string())
        }
    }

    pub fn to_csv_cell(&self) -> String {
        match self {
            Value::Null => String::new(),
            Value::Bool(value) => value.to_string(),
            Value::Number(value) => format_number(*value),
            Value::String(value) => value.clone(),
        }
    }

    pub fn is_truthy_true(&self) -> bool {
        matches!(self, Value::Bool(true))
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Number(value) => Some(*value),
            _ => None,
        }
    }
}
