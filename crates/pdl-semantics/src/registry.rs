#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StageInfo {
    pub name: &'static str,
    pub documentation: &'static str,
    pub can_start_pipeline: bool,
    pub implemented: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FunctionKind {
    Scalar,
    Aggregate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FunctionInfo {
    pub name: &'static str,
    pub documentation: &'static str,
    pub kind: FunctionKind,
    pub min_args: usize,
    pub max_args: Option<usize>,
    pub expected_arity: &'static str,
}

pub type AggregateFunctionInfo = FunctionInfo;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FormatInfo {
    pub name: &'static str,
    pub documentation: &'static str,
    pub load_supported: bool,
    pub save_supported: bool,
    pub stream_supported: bool,
}

pub const LOAD_STAGE: StageInfo = StageInfo {
    name: "load",
    documentation: "Start a pipeline by loading a table from a path or stdin.",
    can_start_pipeline: true,
    implemented: true,
};

pub const STAGES: &[StageInfo] = &[
    StageInfo {
        name: "filter",
        documentation: "Keep rows whose expression evaluates to true.",
        can_start_pipeline: false,
        implemented: true,
    },
    StageInfo {
        name: "select",
        documentation: "Keep and order columns, optionally assigning aliases.",
        can_start_pipeline: false,
        implemented: true,
    },
    StageInfo {
        name: "drop",
        documentation: "Remove columns from the current table.",
        can_start_pipeline: false,
        implemented: true,
    },
    StageInfo {
        name: "rename",
        documentation: "Rename one or more columns with `as`.",
        can_start_pipeline: false,
        implemented: true,
    },
    StageInfo {
        name: "group_by",
        documentation: "Set grouping keys for a following `agg` stage.",
        can_start_pipeline: false,
        implemented: true,
    },
    StageInfo {
        name: "agg",
        documentation: "Aggregate rows with functions such as `sum` and `mean`.",
        can_start_pipeline: false,
        implemented: true,
    },
    StageInfo {
        name: "sort",
        documentation: "Sort rows by one or more columns.",
        can_start_pipeline: false,
        implemented: true,
    },
    StageInfo {
        name: "limit",
        documentation: "Keep the first N rows.",
        can_start_pipeline: false,
        implemented: true,
    },
    StageInfo {
        name: "save",
        documentation: "Write the current table to a file or stdout.",
        can_start_pipeline: false,
        implemented: true,
    },
    StageInfo {
        name: "mutate",
        documentation: "Add or replace columns from row expressions.",
        can_start_pipeline: false,
        implemented: true,
    },
    StageInfo {
        name: "join",
        documentation: "Join the current table with a named binding.",
        can_start_pipeline: false,
        implemented: true,
    },
    StageInfo {
        name: "union",
        documentation:
            "Append rows from a named binding, optionally aligning by name or de-duplicating.",
        can_start_pipeline: false,
        implemented: true,
    },
    StageInfo {
        name: "distinct",
        documentation: "Keep the first row for each unique full row or key column tuple.",
        can_start_pipeline: false,
        implemented: true,
    },
];

pub const SCALAR_FUNCTIONS: &[FunctionInfo] = &[
    FunctionInfo {
        name: "col",
        documentation: "`col(\"name\")`: force a quoted value to resolve as a column.",
        kind: FunctionKind::Scalar,
        min_args: 1,
        max_args: Some(1),
        expected_arity: "one quoted column name",
    },
    FunctionInfo {
        name: "lit",
        documentation: "`lit(value)`: force a value to be interpreted as a literal.",
        kind: FunctionKind::Scalar,
        min_args: 1,
        max_args: Some(1),
        expected_arity: "one argument",
    },
    FunctionInfo {
        name: "is_null",
        documentation: "`is_null(value)`: true when the value is null.",
        kind: FunctionKind::Scalar,
        min_args: 1,
        max_args: Some(1),
        expected_arity: "one argument",
    },
    FunctionInfo {
        name: "not_null",
        documentation: "`not_null(value)`: true when the value is not null.",
        kind: FunctionKind::Scalar,
        min_args: 1,
        max_args: Some(1),
        expected_arity: "one argument",
    },
    FunctionInfo {
        name: "coalesce",
        documentation: "`coalesce(a, b, ...)`: return the first non-null value.",
        kind: FunctionKind::Scalar,
        min_args: 1,
        max_args: None,
        expected_arity: "one or more arguments",
    },
    FunctionInfo {
        name: "concat",
        documentation: "`concat(a, b, ...)`: concatenate non-null values as strings.",
        kind: FunctionKind::Scalar,
        min_args: 1,
        max_args: None,
        expected_arity: "one or more arguments",
    },
    FunctionInfo {
        name: "lower",
        documentation: "`lower(value)`: lowercase a value rendered as text.",
        kind: FunctionKind::Scalar,
        min_args: 1,
        max_args: Some(1),
        expected_arity: "one argument",
    },
    FunctionInfo {
        name: "upper",
        documentation: "`upper(value)`: uppercase a value rendered as text.",
        kind: FunctionKind::Scalar,
        min_args: 1,
        max_args: Some(1),
        expected_arity: "one argument",
    },
    FunctionInfo {
        name: "trim",
        documentation: "`trim(value)`: trim leading and trailing whitespace.",
        kind: FunctionKind::Scalar,
        min_args: 1,
        max_args: Some(1),
        expected_arity: "one argument",
    },
    FunctionInfo {
        name: "to_number",
        documentation: "`to_number(value)`: parse text or pass through a numeric value.",
        kind: FunctionKind::Scalar,
        min_args: 1,
        max_args: Some(1),
        expected_arity: "one argument",
    },
    FunctionInfo {
        name: "abs",
        documentation: "`abs(value)`: absolute value for numeric input.",
        kind: FunctionKind::Scalar,
        min_args: 1,
        max_args: Some(1),
        expected_arity: "one argument",
    },
    FunctionInfo {
        name: "round",
        documentation: "`round(value)`: round numeric input to the nearest integer.",
        kind: FunctionKind::Scalar,
        min_args: 1,
        max_args: Some(1),
        expected_arity: "one argument",
    },
    FunctionInfo {
        name: "if_else",
        documentation: "`if_else(condition, when_true, when_false)`: choose between two values.",
        kind: FunctionKind::Scalar,
        min_args: 3,
        max_args: Some(3),
        expected_arity: "three arguments",
    },
];

pub const AGGREGATE_FUNCTIONS: &[AggregateFunctionInfo] = &[
    FunctionInfo {
        name: "count",
        documentation: "`count()` or `count(\"column\")`: count rows or non-null column values.",
        kind: FunctionKind::Aggregate,
        min_args: 0,
        max_args: Some(1),
        expected_arity: "zero or one argument",
    },
    FunctionInfo {
        name: "sum",
        documentation: "`sum(\"column\")`: sum numeric values.",
        kind: FunctionKind::Aggregate,
        min_args: 1,
        max_args: Some(1),
        expected_arity: "one argument",
    },
    FunctionInfo {
        name: "mean",
        documentation: "`mean(\"column\")`: average numeric values.",
        kind: FunctionKind::Aggregate,
        min_args: 1,
        max_args: Some(1),
        expected_arity: "one argument",
    },
    FunctionInfo {
        name: "min",
        documentation: "`min(\"column\")`: minimum value.",
        kind: FunctionKind::Aggregate,
        min_args: 1,
        max_args: Some(1),
        expected_arity: "one argument",
    },
    FunctionInfo {
        name: "max",
        documentation: "`max(\"column\")`: maximum value.",
        kind: FunctionKind::Aggregate,
        min_args: 1,
        max_args: Some(1),
        expected_arity: "one argument",
    },
];

pub const FORMATS: &[FormatInfo] = &[
    FormatInfo {
        name: "csv",
        documentation: "CSV with a header row; supported for load, save, and stdout.",
        load_supported: true,
        save_supported: true,
        stream_supported: true,
    },
    FormatInfo {
        name: "parquet",
        documentation: "Parquet support is deferred past 0.13.",
        load_supported: false,
        save_supported: false,
        stream_supported: false,
    },
    FormatInfo {
        name: "arrow-file",
        documentation: "Arrow IPC file support is deferred past 0.13.",
        load_supported: false,
        save_supported: false,
        stream_supported: false,
    },
    FormatInfo {
        name: "arrow-stream",
        documentation: "Arrow IPC stream; supported for native stdin, stdout, explicit file load, and explicit file save.",
        load_supported: true,
        save_supported: true,
        stream_supported: true,
    },
    FormatInfo {
        name: "jsonl",
        documentation: "JSON Lines support is deferred past 0.13.",
        load_supported: false,
        save_supported: false,
        stream_supported: false,
    },
];

pub const KEYWORDS: &[&str] = &[
    "load",
    "save",
    "filter",
    "select",
    "drop",
    "rename",
    "mutate",
    "group_by",
    "agg",
    "sort",
    "limit",
    "join",
    "union",
    "distinct",
    "let",
    "as",
    "on",
    "kind",
    "by_name",
    "format",
    "stdin",
    "stdout",
    "true",
    "false",
    "null",
    "and",
    "or",
    "not",
    "asc",
    "desc",
    "inner",
    "left",
    "right",
    "full",
    "semi",
    "anti",
    "nulls_first",
    "nulls_last",
];

pub fn stage_info(name: &str) -> Option<&'static StageInfo> {
    if name == LOAD_STAGE.name {
        return Some(&LOAD_STAGE);
    }
    STAGES.iter().find(|info| info.name == name)
}

pub fn scalar_function(name: &str) -> Option<&'static FunctionInfo> {
    SCALAR_FUNCTIONS.iter().find(|info| info.name == name)
}

pub fn aggregate_function(name: &str) -> Option<&'static AggregateFunctionInfo> {
    AGGREGATE_FUNCTIONS.iter().find(|info| info.name == name)
}

pub fn format_info(name: &str) -> Option<&'static FormatInfo> {
    FORMATS.iter().find(|info| info.name == name)
}

pub fn accepts_arity(info: FunctionInfo, actual: usize) -> bool {
    actual >= info.min_args
        && match info.max_args {
            Some(max) => actual <= max,
            None => true,
        }
}
