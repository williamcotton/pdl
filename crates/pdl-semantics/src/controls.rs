use pdl_core::{codes, Diagnostic, Span};
use pdl_data::Value;
use pdl_syntax::{
    ContextDecl, ControlArg, ControlInitializer, ControlKind, ControlLiteral, ControlValue,
};
use std::collections::{BTreeMap, BTreeSet};

use crate::ir::ControlKindIr;

#[derive(Clone, Debug, PartialEq)]
pub struct ControlMetadata {
    pub name: String,
    pub kind: ControlKindIr,
    pub label: String,
    pub default: Value,
    pub span: Span,
    pub name_span: Span,
    pub control_span: Span,
    pub placeholder: Option<String>,
    pub rows: Option<usize>,
    pub min: Option<Value>,
    pub max: Option<Value>,
    pub step: Option<Value>,
    pub choices: Vec<ControlChoice>,
    pub choices_from: Option<ChoiceSource>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ControlChoice {
    pub value: Value,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChoiceSource {
    pub binding: String,
    pub column: String,
    pub span: Span,
}

pub(crate) fn analyze_control_decl(
    context: &ContextDecl,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ControlMetadata> {
    let control = context.control.as_ref()?;
    let args = ArgLookup::new(control, diagnostics);
    let allowed = allowed_args(control.kind);
    for arg in &control.args {
        if !allowed.contains(&arg.name.value.as_str()) {
            diagnostics.push(Diagnostic::error(
                codes::E2009,
                format!(
                    "unknown argument `{}` for `{}`",
                    arg.name.value,
                    control.kind.as_str()
                ),
                arg.name.span,
            ));
        }
    }

    let label = required_string("label", control, &args, diagnostics);
    let metadata = match control.kind {
        ControlKind::Text => text_metadata(context, control, &args, diagnostics, false),
        ControlKind::Textarea => text_metadata(context, control, &args, diagnostics, true),
        ControlKind::Number => number_metadata(context, control, &args, diagnostics, false),
        ControlKind::Range => number_metadata(context, control, &args, diagnostics, true),
        ControlKind::Checkbox => checkbox_metadata(context, control, &args, diagnostics),
        ControlKind::Select | ControlKind::Radio => {
            choice_metadata(context, control, &args, diagnostics)
        }
        ControlKind::Date | ControlKind::Time | ControlKind::Datetime => {
            temporal_metadata(context, control, &args, diagnostics)
        }
        ControlKind::Color => color_metadata(context, control, &args, diagnostics),
    };

    metadata.map(|mut metadata| {
        metadata.label = label.unwrap_or_default();
        metadata
    })
}

pub(crate) fn control_kind_ir(kind: ControlKind) -> ControlKindIr {
    match kind {
        ControlKind::Text => ControlKindIr::Text,
        ControlKind::Textarea => ControlKindIr::Textarea,
        ControlKind::Number => ControlKindIr::Number,
        ControlKind::Range => ControlKindIr::Range,
        ControlKind::Checkbox => ControlKindIr::Checkbox,
        ControlKind::Select => ControlKindIr::Select,
        ControlKind::Radio => ControlKindIr::Radio,
        ControlKind::Date => ControlKindIr::Date,
        ControlKind::Time => ControlKindIr::Time,
        ControlKind::Datetime => ControlKindIr::Datetime,
        ControlKind::Color => ControlKindIr::Color,
    }
}

pub(crate) fn literal_value(literal: &ControlLiteral) -> Option<Value> {
    match literal {
        ControlLiteral::Quoted(value) => Some(Value::String(value.value.clone())),
        ControlLiteral::Number(value) => Some(Value::Number(value.value)),
        ControlLiteral::Bool(value) => Some(Value::Bool(value.value)),
        ControlLiteral::Null(_) => Some(Value::Null),
    }
}

fn text_metadata(
    context: &ContextDecl,
    control: &ControlInitializer,
    args: &ArgLookup<'_>,
    diagnostics: &mut Vec<Diagnostic>,
    textarea: bool,
) -> Option<ControlMetadata> {
    let default = required_string("default", control, args, diagnostics).map(Value::String)?;
    let placeholder = optional_string("placeholder", args, diagnostics);
    let rows = if textarea {
        optional_positive_integer("rows", args, diagnostics)
    } else {
        None
    };
    Some(base_metadata(context, control, default)).map(|metadata| ControlMetadata {
        placeholder,
        rows,
        ..metadata
    })
}

fn number_metadata(
    context: &ContextDecl,
    control: &ControlInitializer,
    args: &ArgLookup<'_>,
    diagnostics: &mut Vec<Diagnostic>,
    range: bool,
) -> Option<ControlMetadata> {
    let default = required_number("default", control, args, diagnostics)?;
    let min = if range {
        required_number("min", control, args, diagnostics)
    } else {
        optional_number("min", args, diagnostics)
    };
    let max = if range {
        required_number("max", control, args, diagnostics)
    } else {
        optional_number("max", args, diagnostics)
    };
    let step = optional_number("step", args, diagnostics);
    validate_numeric_bounds(control, default, min, max, step, diagnostics);
    Some(ControlMetadata {
        min: min.map(Value::Number),
        max: max.map(Value::Number),
        step: step.map(Value::Number),
        ..base_metadata(context, control, Value::Number(default))
    })
}

fn checkbox_metadata(
    context: &ContextDecl,
    control: &ControlInitializer,
    args: &ArgLookup<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ControlMetadata> {
    let default = required_bool("default", control, args, diagnostics)?;
    Some(base_metadata(context, control, Value::Bool(default)))
}

fn choice_metadata(
    context: &ContextDecl,
    control: &ControlInitializer,
    args: &ArgLookup<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ControlMetadata> {
    let default = required_scalar("default", control, args, diagnostics)?;
    if matches!(default, Value::Null) {
        diagnostics.push(Diagnostic::error(
            codes::E2011,
            format!(
                "`{}` default must be a string, number, or boolean",
                control.kind.as_str()
            ),
            args.span("default").unwrap_or(control.span),
        ));
        return None;
    }

    let choices_from = optional_choice_source("choicesFrom", args, diagnostics);
    let choices = optional_static_choices("choices", args, &default, diagnostics);
    if choices.is_empty() && choices_from.is_none() {
        diagnostics.push(Diagnostic::error(
            codes::E2008,
            format!(
                "`{}` requires `choices` or `choicesFrom`",
                control.kind.as_str()
            ),
            control.span,
        ));
    }
    if !choices.is_empty()
        && choices_from.is_none()
        && !choices.iter().any(|choice| choice.value == default)
    {
        diagnostics.push(Diagnostic::error(
            codes::E2011,
            "`default` must be present in static choices",
            args.span("default").unwrap_or(control.span),
        ));
    }

    Some(ControlMetadata {
        choices,
        choices_from,
        ..base_metadata(context, control, default)
    })
}

fn temporal_metadata(
    context: &ContextDecl,
    control: &ControlInitializer,
    args: &ArgLookup<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ControlMetadata> {
    let default = required_string("default", control, args, diagnostics)?;
    validate_temporal_value("default", &default, control, args, diagnostics);
    let min = optional_string("min", args, diagnostics);
    let max = optional_string("max", args, diagnostics);
    if let Some(value) = &min {
        validate_temporal_value("min", value, control, args, diagnostics);
    }
    if let Some(value) = &max {
        validate_temporal_value("max", value, control, args, diagnostics);
    }
    if let (Some(min), Some(max)) = (&min, &max) {
        if min > max {
            diagnostics.push(Diagnostic::error(
                codes::E2011,
                "`min` must be less than or equal to `max`",
                args.span("min").unwrap_or(control.span),
            ));
        }
    }
    let step = optional_temporal_step(control, args, diagnostics);
    Some(ControlMetadata {
        min: min.map(Value::String),
        max: max.map(Value::String),
        step,
        ..base_metadata(context, control, Value::String(default))
    })
}

fn color_metadata(
    context: &ContextDecl,
    control: &ControlInitializer,
    args: &ArgLookup<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ControlMetadata> {
    let default = required_string("default", control, args, diagnostics)?;
    if !is_hex_color(&default) {
        diagnostics.push(Diagnostic::error(
            codes::E2011,
            "`input_color` default must be a `#RRGGBB` string",
            args.span("default").unwrap_or(control.span),
        ));
    }
    Some(base_metadata(context, control, Value::String(default)))
}

fn base_metadata(
    context: &ContextDecl,
    control: &ControlInitializer,
    default: Value,
) -> ControlMetadata {
    ControlMetadata {
        name: context.name.value.clone(),
        kind: control_kind_ir(control.kind),
        label: String::new(),
        default,
        span: context.span,
        name_span: context.name.span,
        control_span: control.span,
        placeholder: None,
        rows: None,
        min: None,
        max: None,
        step: None,
        choices: Vec::new(),
        choices_from: None,
    }
}

fn required_string(
    name: &str,
    control: &ControlInitializer,
    args: &ArgLookup<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<String> {
    match args.get(name) {
        Some(arg) => string_value(arg, diagnostics),
        None => {
            diagnostics.push(Diagnostic::error(
                codes::E2008,
                format!("`{}` requires `{name}`", control.kind.as_str()),
                control.span,
            ));
            None
        }
    }
}

fn optional_string(
    name: &str,
    args: &ArgLookup<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<String> {
    args.get(name)
        .and_then(|arg| string_value(arg, diagnostics))
}

fn string_value(arg: &ControlArg, diagnostics: &mut Vec<Diagnostic>) -> Option<String> {
    match &arg.value {
        ControlValue::Literal(ControlLiteral::Quoted(value)) => Some(value.value.clone()),
        _ => {
            diagnostics.push(Diagnostic::error(
                codes::E2011,
                format!("control argument `{}` must be a string", arg.name.value),
                arg.value.span(),
            ));
            None
        }
    }
}

fn required_number(
    name: &str,
    control: &ControlInitializer,
    args: &ArgLookup<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<f64> {
    match args.get(name) {
        Some(arg) => number_value(arg, diagnostics),
        None => {
            diagnostics.push(Diagnostic::error(
                codes::E2008,
                format!("`{}` requires `{name}`", control.kind.as_str()),
                control.span,
            ));
            None
        }
    }
}

fn optional_number(
    name: &str,
    args: &ArgLookup<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<f64> {
    args.get(name)
        .and_then(|arg| number_value(arg, diagnostics))
}

fn number_value(arg: &ControlArg, diagnostics: &mut Vec<Diagnostic>) -> Option<f64> {
    match &arg.value {
        ControlValue::Literal(ControlLiteral::Number(value)) => Some(value.value),
        _ => {
            diagnostics.push(Diagnostic::error(
                codes::E2011,
                format!("control argument `{}` must be a number", arg.name.value),
                arg.value.span(),
            ));
            None
        }
    }
}

fn required_bool(
    name: &str,
    control: &ControlInitializer,
    args: &ArgLookup<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<bool> {
    match args.get(name) {
        Some(arg) => match &arg.value {
            ControlValue::Literal(ControlLiteral::Bool(value)) => Some(value.value),
            _ => {
                diagnostics.push(Diagnostic::error(
                    codes::E2011,
                    format!("control argument `{}` must be a boolean", arg.name.value),
                    arg.value.span(),
                ));
                None
            }
        },
        None => {
            diagnostics.push(Diagnostic::error(
                codes::E2008,
                format!("`{}` requires `{name}`", control.kind.as_str()),
                control.span,
            ));
            None
        }
    }
}

fn required_scalar(
    name: &str,
    control: &ControlInitializer,
    args: &ArgLookup<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Value> {
    match args.get(name) {
        Some(arg) => match &arg.value {
            ControlValue::Literal(value) => literal_value(value),
            _ => {
                diagnostics.push(Diagnostic::error(
                    codes::E2011,
                    format!(
                        "control argument `{}` must be a scalar literal",
                        arg.name.value
                    ),
                    arg.value.span(),
                ));
                None
            }
        },
        None => {
            diagnostics.push(Diagnostic::error(
                codes::E2008,
                format!("`{}` requires `{name}`", control.kind.as_str()),
                control.span,
            ));
            None
        }
    }
}

fn optional_positive_integer(
    name: &str,
    args: &ArgLookup<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<usize> {
    let arg = args.get(name)?;
    let value = number_value(arg, diagnostics)?;
    if value.fract() == 0.0 && value > 0.0 {
        Some(value as usize)
    } else {
        diagnostics.push(Diagnostic::error(
            codes::E2011,
            format!("control argument `{name}` must be a positive integer"),
            arg.value.span(),
        ));
        None
    }
}

fn optional_static_choices(
    name: &str,
    args: &ArgLookup<'_>,
    default: &Value,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<ControlChoice> {
    let Some(arg) = args.get(name) else {
        return Vec::new();
    };
    let ControlValue::Array { values, span } = &arg.value else {
        diagnostics.push(Diagnostic::error(
            codes::E2011,
            "`choices` must be an array of scalar literals",
            arg.value.span(),
        ));
        return Vec::new();
    };
    if values.is_empty() {
        diagnostics.push(Diagnostic::error(
            codes::E2011,
            "`choices` must not be empty",
            *span,
        ));
    }
    let mut seen = BTreeSet::new();
    let mut choices = Vec::new();
    for value in values {
        let Some(choice) = literal_value(value) else {
            continue;
        };
        if matches!(choice, Value::Null) {
            diagnostics.push(Diagnostic::error(
                codes::E2011,
                "`choices` values must be strings, numbers, or booleans",
                value.span(),
            ));
            continue;
        }
        if !value_class_matches(default, &choice) {
            diagnostics.push(Diagnostic::error(
                codes::E2011,
                "`choices` values must match the default value type",
                value.span(),
            ));
            continue;
        }
        let key = stable_value_key(&choice);
        if !seen.insert(key) {
            diagnostics.push(Diagnostic::error(
                codes::E2011,
                "duplicate static choice",
                value.span(),
            ));
            continue;
        }
        choices.push(ControlChoice {
            value: choice,
            span: value.span(),
        });
    }
    choices
}

fn optional_choice_source(
    name: &str,
    args: &ArgLookup<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ChoiceSource> {
    let arg = args.get(name)?;
    match &arg.value {
        ControlValue::BindingColumn {
            binding,
            column,
            span,
        } => Some(ChoiceSource {
            binding: binding.value.clone(),
            column: column.value.clone(),
            span: *span,
        }),
        _ => {
            diagnostics.push(Diagnostic::error(
                codes::E2011,
                "`choicesFrom` must be a binding-column reference",
                arg.value.span(),
            ));
            None
        }
    }
}

fn optional_temporal_step(
    control: &ControlInitializer,
    args: &ArgLookup<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Value> {
    if !matches!(control.kind, ControlKind::Time | ControlKind::Datetime) {
        return None;
    }
    let arg = args.get("step")?;
    let value = number_value(arg, diagnostics)?;
    if value <= 0.0 {
        diagnostics.push(Diagnostic::error(
            codes::E2011,
            "`step` must be positive",
            arg.value.span(),
        ));
    }
    Some(Value::Number(value))
}

fn validate_numeric_bounds(
    control: &ControlInitializer,
    default: f64,
    min: Option<f64>,
    max: Option<f64>,
    step: Option<f64>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let (Some(min), Some(max)) = (min, max) {
        if min > max {
            diagnostics.push(Diagnostic::error(
                codes::E2011,
                "`min` must be less than or equal to `max`",
                control.span,
            ));
        }
        if default < min || default > max {
            diagnostics.push(Diagnostic::error(
                codes::E2011,
                "`default` must be inside the inclusive range",
                control.span,
            ));
        }
    }
    if step.is_some_and(|value| value <= 0.0) {
        diagnostics.push(Diagnostic::error(
            codes::E2011,
            "`step` must be positive",
            control.span,
        ));
    }
}

fn validate_temporal_value(
    name: &str,
    value: &str,
    control: &ControlInitializer,
    args: &ArgLookup<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let valid = match control.kind {
        ControlKind::Date => is_date(value),
        ControlKind::Time => is_time(value),
        ControlKind::Datetime => is_datetime_local(value),
        _ => true,
    };
    if !valid {
        diagnostics.push(Diagnostic::error(
            codes::E2011,
            format!(
                "`{}` argument `{name}` has an invalid date/time string",
                control.kind.as_str()
            ),
            args.span(name).unwrap_or(control.span),
        ));
    }
}

fn value_class_matches(default: &Value, value: &Value) -> bool {
    matches!(
        (default, value),
        (Value::Bool(_), Value::Bool(_))
            | (Value::Number(_), Value::Number(_))
            | (Value::String(_), Value::String(_))
    )
}

fn stable_value_key(value: &Value) -> String {
    match value {
        Value::Null => "null:".to_string(),
        Value::Bool(value) => format!("bool:{value}"),
        Value::Number(value) => format!("number:{value:?}"),
        Value::String(value) => format!("string:{value}"),
        // Geometry is never a valid control value (PDL_SPEC §10.13).
        Value::Geometry(_) => "geometry:".to_string(),
    }
}

fn allowed_args(kind: ControlKind) -> &'static [&'static str] {
    match kind {
        ControlKind::Text => &["label", "default", "placeholder"],
        ControlKind::Textarea => &["label", "default", "placeholder", "rows"],
        ControlKind::Number => &["label", "default", "min", "max", "step"],
        ControlKind::Range => &["label", "min", "max", "default", "step"],
        ControlKind::Checkbox => &["label", "default"],
        ControlKind::Select | ControlKind::Radio => &["label", "default", "choices", "choicesFrom"],
        ControlKind::Date => &["label", "default", "min", "max"],
        ControlKind::Time | ControlKind::Datetime => &["label", "default", "min", "max", "step"],
        ControlKind::Color => &["label", "default"],
    }
}

fn is_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && in_range(&value[5..7], 1, 12)
        && in_range(&value[8..10], 1, 31)
}

fn is_time(value: &str) -> bool {
    let parts = value.split(':').collect::<Vec<_>>();
    matches!(parts.len(), 2 | 3)
        && parts
            .iter()
            .all(|part| part.len() == 2 && part.bytes().all(|byte| byte.is_ascii_digit()))
        && in_range(parts[0], 0, 23)
        && in_range(parts[1], 0, 59)
        && (parts.len() == 2 || in_range(parts[2], 0, 59))
}

fn is_datetime_local(value: &str) -> bool {
    let Some((date, time)) = value.split_once('T') else {
        return false;
    };
    is_date(date) && is_time(time)
}

fn is_hex_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value.as_bytes()[1..]
            .iter()
            .all(|byte| byte.is_ascii_hexdigit())
}

fn in_range(value: &str, min: u32, max: u32) -> bool {
    value
        .parse::<u32>()
        .is_ok_and(|value| (min..=max).contains(&value))
}

struct ArgLookup<'a> {
    first: BTreeMap<&'a str, &'a ControlArg>,
}

impl<'a> ArgLookup<'a> {
    fn new(control: &'a ControlInitializer, diagnostics: &mut Vec<Diagnostic>) -> Self {
        let mut first = BTreeMap::new();
        for arg in &control.args {
            if first.insert(arg.name.value.as_str(), arg).is_some() {
                diagnostics.push(Diagnostic::error(
                    codes::E2010,
                    format!("duplicate control argument `{}`", arg.name.value),
                    arg.name.span,
                ));
            }
        }
        Self { first }
    }

    fn get(&self, name: &str) -> Option<&'a ControlArg> {
        self.first.get(name).copied()
    }

    fn span(&self, name: &str) -> Option<Span> {
        self.get(name).map(|arg| arg.span)
    }
}
