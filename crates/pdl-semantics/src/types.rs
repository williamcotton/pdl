#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogicalType {
    Unknown,
    Null,
    Bool,
    Number,
    String,
}
