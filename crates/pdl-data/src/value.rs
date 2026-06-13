use crate::format::format_number;
use geo_types::Geometry;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    /// An opaque geospatial geometry carried through tabular preparation
    /// (PDL_SPEC §10.13). Geometry is its own value class: it is not a string,
    /// number, boolean, or null, and it cannot be used in scalar expressions,
    /// join keys, or control values. It exists so geospatial loaders can carry
    /// feature geometry through ordinary table stages and write it back to
    /// GeoJSON.
    Geometry(Box<Geometry<f64>>),
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

    pub fn geometry(geometry: Geometry<f64>) -> Self {
        Self::Geometry(Box::new(geometry))
    }

    pub fn is_geometry(&self) -> bool {
        matches!(self, Value::Geometry(_))
    }

    pub fn to_csv_cell(&self) -> String {
        match self {
            Value::Null => String::new(),
            Value::Bool(value) => value.to_string(),
            Value::Number(value) => format_number(*value),
            Value::String(value) => value.clone(),
            // Geometry has no scalar text encoding; callers that may encounter
            // geometry reject it before stringifying (PDL_SPEC §10.13).
            Value::Geometry(_) => String::new(),
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
