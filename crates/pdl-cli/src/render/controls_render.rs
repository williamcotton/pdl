use pdl_core::{codes, Diagnostic, Span};
use pdl_data::Value;
use pdl_driver::{OsDriverIo, PreparedProgram};
use pdl_exec::{collect_binding_column_choices, resolve_context_values};
use pdl_semantics::{ChoiceSource, ControlChoice, ControlMetadata};
use serde::Serialize;
use serde_json::{Number, Value as JsonValue};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Serialize)]
pub struct ControlsOutputJson {
    source_path: String,
    contexts: Vec<ContextDeclJson>,
    controls: Vec<ControlJson>,
    diagnostics: Vec<Diagnostic>,
}

impl ControlsOutputJson {
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
}

#[derive(Clone, Serialize)]
struct ContextDeclJson {
    name: String,
    context_kind: &'static str,
    renderable: bool,
    default: JsonValue,
    current_value: JsonValue,
    span: Span,
}

#[derive(Clone, Serialize)]
struct ControlJson {
    name: String,
    kind: &'static str,
    label: String,
    default: JsonValue,
    current_value: JsonValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    placeholder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rows: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    min: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    step: Option<JsonValue>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    choices: Vec<ChoiceJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    choices_from: Option<ChoiceSourceJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_choices: Option<DynamicChoicesJson>,
    span: Span,
    name_span: Span,
    control_span: Span,
}

#[derive(Clone, Serialize)]
struct ChoiceJson {
    value: JsonValue,
    span: Span,
}

#[derive(Clone, Serialize)]
struct ChoiceSourceJson {
    binding: String,
    column: String,
    span: Span,
}

#[derive(Clone, Serialize)]
struct DynamicChoicesJson {
    status: &'static str,
    choices: Vec<JsonValue>,
    current_value_present: bool,
}

pub fn controls_json(
    prepared: &PreparedProgram,
    context_overrides: BTreeMap<String, Value>,
) -> ControlsOutputJson {
    let mut diagnostics = prepared.diagnostics();
    let context_values = prepared
        .analysis
        .ir
        .as_ref()
        .map(|ir| resolve_context_values(ir, context_overrides.clone(), &mut diagnostics))
        .unwrap_or_default();
    let controls = prepared
        .analysis
        .controls
        .iter()
        .map(|control| {
            control_json(
                prepared,
                control,
                &context_values,
                &context_overrides,
                &mut diagnostics,
            )
        })
        .collect();
    ControlsOutputJson {
        source_path: prepared.path.display().to_string(),
        contexts: context_decl_json(prepared, &context_values),
        controls,
        diagnostics,
    }
}

fn context_decl_json(
    prepared: &PreparedProgram,
    context_values: &BTreeMap<String, Value>,
) -> Vec<ContextDeclJson> {
    prepared
        .analysis
        .ir
        .as_ref()
        .map(|ir| {
            ir.contexts
                .iter()
                .map(|context| {
                    let default = expr_default_value(&context.default).unwrap_or(Value::Null);
                    let current = context_values
                        .get(&context.name)
                        .cloned()
                        .unwrap_or_else(|| default.clone());
                    ContextDeclJson {
                        name: context.name.clone(),
                        context_kind: match context.kind {
                            pdl_semantics::ContextKindIr::Param => "param",
                            pdl_semantics::ContextKindIr::State => "state",
                        },
                        renderable: context.control.is_some(),
                        default: value_json(&default),
                        current_value: value_json(&current),
                        span: context.span,
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn control_json(
    prepared: &PreparedProgram,
    control: &ControlMetadata,
    context_values: &BTreeMap<String, Value>,
    context_overrides: &BTreeMap<String, Value>,
    diagnostics: &mut Vec<Diagnostic>,
) -> ControlJson {
    let current = context_values
        .get(&control.name)
        .cloned()
        .unwrap_or_else(|| control.default.clone());
    let (dynamic_choices, dynamic_status) =
        dynamic_choices(prepared, control, &current, context_overrides, diagnostics);
    ControlJson {
        name: control.name.clone(),
        kind: control.kind.as_str(),
        label: control.label.clone(),
        default: value_json(&control.default),
        current_value: value_json(&current),
        placeholder: control.placeholder.clone(),
        rows: control.rows,
        min: control.min.as_ref().map(value_json),
        max: control.max.as_ref().map(value_json),
        step: control.step.as_ref().map(value_json),
        choices: control.choices.iter().map(choice_json).collect(),
        choices_from: control.choices_from.as_ref().map(choice_source_json),
        dynamic_choices: dynamic_status.map(|status| DynamicChoicesJson {
            status,
            current_value_present: dynamic_choices
                .iter()
                .any(|value| stable_value_key(value) == stable_value_key(&current)),
            choices: dynamic_choices.iter().map(value_json).collect(),
        }),
        span: control.span,
        name_span: control.name_span,
        control_span: control.control_span,
    }
}

fn dynamic_choices(
    prepared: &PreparedProgram,
    control: &ControlMetadata,
    current: &Value,
    context_overrides: &BTreeMap<String, Value>,
    diagnostics: &mut Vec<Diagnostic>,
) -> (Vec<Value>, Option<&'static str>) {
    let Some(source) = &control.choices_from else {
        return (Vec::new(), None);
    };
    let io = OsDriverIo;
    let raw_values = match collect_binding_column_choices(
        prepared,
        &source.binding,
        &source.column,
        context_overrides.clone(),
        &io,
    ) {
        Ok(values) => values,
        Err(extraction_diagnostics) => {
            extend_new_diagnostics(diagnostics, extraction_diagnostics);
            return (Vec::new(), Some("error"));
        }
    };
    let mut seen = BTreeSet::new();
    let mut values = Vec::new();
    for raw in raw_values {
        match coerce_choice_value(raw, &control.default, source) {
            Ok(value) => {
                if seen.insert(stable_value_key(&value)) {
                    values.push(value);
                }
            }
            Err(diagnostic) => diagnostics.push(diagnostic),
        }
    }
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "E2013" && diagnostic.span == source.span)
    {
        (values, Some("error"))
    } else {
        let _ = current;
        (values, Some("ready"))
    }
}

fn coerce_choice_value(
    raw: Value,
    default: &Value,
    source: &ChoiceSource,
) -> Result<Value, Diagnostic> {
    match (default, raw) {
        (Value::String(_), Value::String(value)) => Ok(Value::String(value)),
        (Value::String(_), value) => Ok(Value::String(value.to_csv_cell())),
        (Value::Number(_), Value::Number(value)) => Ok(Value::Number(value)),
        (Value::Number(_), Value::String(value)) => {
            value.parse::<f64>().map(Value::Number).map_err(|_| {
                Diagnostic::error(
                    codes::E2013,
                    format!(
                        "choicesFrom value in `{}.{}` cannot be coerced to a number",
                        source.binding, source.column
                    ),
                    source.span,
                )
            })
        }
        (Value::Bool(_), Value::Bool(value)) => Ok(Value::Bool(value)),
        (Value::Bool(_), Value::String(value)) if value == "true" => Ok(Value::Bool(true)),
        (Value::Bool(_), Value::String(value)) if value == "false" => Ok(Value::Bool(false)),
        (Value::Bool(_), _) => Err(Diagnostic::error(
            codes::E2013,
            format!(
                "choicesFrom value in `{}.{}` cannot be coerced to a boolean",
                source.binding, source.column
            ),
            source.span,
        )),
        (Value::Null, value) => Ok(value),
        (Value::Number(_), _) => Err(Diagnostic::error(
            codes::E2013,
            format!(
                "choicesFrom value in `{}.{}` cannot be coerced to a number",
                source.binding, source.column
            ),
            source.span,
        )),
        // Geometry is opaque and cannot be a control value (PDL_SPEC §10.13).
        (Value::Geometry(_), _) => Err(Diagnostic::error(
            codes::E2013,
            format!(
                "choicesFrom value in `{}.{}` cannot be a geometry",
                source.binding, source.column
            ),
            source.span,
        )),
    }
}

fn choice_json(choice: &ControlChoice) -> ChoiceJson {
    ChoiceJson {
        value: value_json(&choice.value),
        span: choice.span,
    }
}

fn choice_source_json(source: &ChoiceSource) -> ChoiceSourceJson {
    ChoiceSourceJson {
        binding: source.binding.clone(),
        column: source.column.clone(),
        span: source.span,
    }
}

fn expr_default_value(expr: &pdl_semantics::ExprIr) -> Option<Value> {
    match expr {
        pdl_semantics::ExprIr::Quoted { value, .. } => Some(Value::String(value.clone())),
        pdl_semantics::ExprIr::Number { value, .. } => Some(Value::Number(*value)),
        pdl_semantics::ExprIr::Bool { value, .. } => Some(Value::Bool(*value)),
        pdl_semantics::ExprIr::Null { .. } => Some(Value::Null),
        _ => None,
    }
}

fn value_json(value: &Value) -> JsonValue {
    match value {
        Value::Null => JsonValue::Null,
        Value::Bool(value) => JsonValue::Bool(*value),
        Value::Number(value) => Number::from_f64(*value)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        Value::String(value) => JsonValue::String(value.clone()),
        Value::Geometry(_) => JsonValue::Null,
    }
}

fn stable_value_key(value: &Value) -> String {
    match value {
        Value::Null => "null:".to_string(),
        Value::Bool(value) => format!("bool:{value}"),
        Value::Number(value) => format!("number:{value:?}"),
        Value::String(value) => format!("string:{value}"),
        Value::Geometry(_) => "geometry:".to_string(),
    }
}

fn extend_new_diagnostics(target: &mut Vec<Diagnostic>, incoming: Vec<Diagnostic>) {
    for diagnostic in incoming {
        if !target.contains(&diagnostic) {
            target.push(diagnostic);
        }
    }
}
