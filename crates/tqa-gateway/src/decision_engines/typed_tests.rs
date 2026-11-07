//! GW-U-076: the typed-question engine kind's record (ADR-0013, the adapter amendment): `model` required and bounded,
//! `min_confidence` 0 to 1, bounded `route_hints` keyed by route names, and the optional budget (calls or tokens, a
//! period) that every kind may carry, a `decision_v1` engine counting calls only. None of the kind's own fields is
//! accepted on a `decision_v1` engine. Every refusal names the field and a documented reason.
use super::{EngineDocument, EngineProtocol, validate};
use crate::{InvalidReason, connectors::Origin};
use serde_json::{Value, json};
use tqa_types::DecisionPoint;

fn typed() -> Value {
    json!({"name":"router","protocol":"typed_questions_v1","url":"https://questions.example/v1/evaluate",
        "auth":{"scheme":"bearer"},"key_ref":"secrets/questions","model":"pinned-model"})
}
fn plain() -> Value {
    json!({"name":"router","url":"https://engine.example"})
}
fn with(mut base: Value, field: &str, value: Value) -> Value {
    base[field] = value;
    base
}
fn without(mut base: Value, field: &str) -> Value {
    if let Some(fields) = base.as_object_mut() {
        fields.remove(field);
    }
    base
}
fn refused(value: Value) -> Option<(&'static str, InvalidReason)> {
    let document: EngineDocument = serde_json::from_value(value).ok()?;
    validate(&document, Origin::Admin).err()
}

// TEST: GW-U-076
#[test]
fn gw_u_076_a_typed_question_engine_is_accepted_with_its_defaults()
-> Result<(), Box<dyn std::error::Error>> {
    let parsed: EngineDocument = serde_json::from_value(typed())?;
    validate(&parsed, Origin::Admin).map_err(|refusal| format!("{refusal:?}"))?;
    assert_eq!(parsed.protocol, EngineProtocol::TypedQuestionsV1);
    assert!((parsed.min_confidence() - 0.6).abs() < f64::EPSILON);
    assert!(parsed.route_hints.is_empty() && parsed.budget.is_none());
    assert_eq!(
        parsed.protocol.points(),
        Some(vec![DecisionPoint::BeforeStep]),
        "it answers before a step only, so the runtime never asks it after a response"
    );
    assert_eq!(
        serde_json::to_value(&parsed)?,
        json!({"name":"router","protocol":"typed_questions_v1","url":"https://questions.example/v1/evaluate",
            "auth":{"scheme":"bearer","required":false},"key_ref":"secrets/questions","timeout_ms":2000,
            "exposure":"none","enabled":true,"model":"pinned-model"}),
        "unset optional fields aren't stored"
    );
    let full = with(
        with(
            with(typed(), "min_confidence", json!(0.75)),
            "route_hints",
            json!({"local-text": "routine steps: reading, looking something up"}),
        ),
        "budget",
        json!({"calls": 3000, "tokens": 30_000_000, "period": "month"}),
    );
    let parsed: EngineDocument = serde_json::from_value(full)?;
    validate(&parsed, Origin::Admin).map_err(|refusal| format!("{refusal:?}"))?;
    Ok(())
}

// TEST: GW-U-076
#[test]
fn gw_u_076_every_typed_field_rule_names_the_field_and_reason() {
    let long_hint = "h".repeat(201);
    let many_hints: serde_json::Map<String, Value> = (0..17)
        .map(|index| (format!("r{index}"), json!("a hint")))
        .collect();
    for (value, expected) in [
        (
            without(typed(), "model"),
            ("model", InvalidReason::ModelEmpty),
        ),
        (
            with(typed(), "model", json!("")),
            ("model", InvalidReason::ModelEmpty),
        ),
        (
            with(typed(), "model", json!("m".repeat(129))),
            ("model", InvalidReason::ParameterInvalid),
        ),
        (
            with(typed(), "model", json!("two words")),
            ("model", InvalidReason::ParameterInvalid),
        ),
        (
            with(typed(), "min_confidence", json!(1.5)),
            ("min_confidence", InvalidReason::LimitOutOfRange),
        ),
        (
            with(typed(), "min_confidence", json!(-0.1)),
            ("min_confidence", InvalidReason::LimitOutOfRange),
        ),
        (
            with(typed(), "route_hints", json!({"local-text": ""})),
            ("route_hints", InvalidReason::ParameterInvalid),
        ),
        (
            with(typed(), "route_hints", json!({"local-text": long_hint})),
            ("route_hints", InvalidReason::ParameterInvalid),
        ),
        (
            with(typed(), "route_hints", json!({"local-text": "a\nb"})),
            ("route_hints", InvalidReason::ParameterInvalid),
        ),
        (
            with(typed(), "route_hints", Value::Object(many_hints)),
            ("route_hints", InvalidReason::ParameterInvalid),
        ),
        (
            with(typed(), "route_hints", json!({"": "hint"})),
            ("route_hints", InvalidReason::RouteNameInvalid),
        ),
        (
            with(typed(), "budget", json!({"period": "month"})),
            ("budget", InvalidReason::BudgetInvalid),
        ),
        (
            with(typed(), "budget", json!({"calls": 0})),
            ("budget", InvalidReason::BudgetInvalid),
        ),
        (
            with(typed(), "budget", json!({"tokens": 0})),
            ("budget", InvalidReason::BudgetInvalid),
        ),
    ] {
        assert_eq!(refused(value.clone()), Some(expected), "{value}");
    }
}

// TEST: GW-U-076
#[test]
fn gw_u_076_a_decision_v1_engine_takes_none_of_the_kinds_fields_and_counts_calls_only() {
    for (value, expected) in [
        // The kind's own fields have no meaning on a decision/v1 engine, which also counts calls only.
        (
            with(plain(), "model", json!("pinned-model")),
            ("model", InvalidReason::ParameterInvalid),
        ),
        (
            with(plain(), "min_confidence", json!(0.5)),
            ("min_confidence", InvalidReason::ParameterInvalid),
        ),
        (
            with(plain(), "route_hints", json!({"local": "hint"})),
            ("route_hints", InvalidReason::ParameterInvalid),
        ),
        (
            with(plain(), "budget", json!({"tokens": 1000})),
            ("budget", InvalidReason::BudgetInvalid),
        ),
    ] {
        assert_eq!(refused(value.clone()), Some(expected), "{value}");
    }
    // A decision/v1 engine may carry a budget of calls, over the whole time or a calendar month.
    assert_eq!(
        refused(with(plain(), "budget", json!({"calls": 500}))),
        None
    );
    assert_eq!(
        refused(with(
            plain(),
            "budget",
            json!({"calls": 500, "period": "month"})
        )),
        None
    );
    // An unknown period or budget field is refused when parsing.
    assert!(
        serde_json::from_value::<EngineDocument>(with(
            typed(),
            "budget",
            json!({"calls": 1, "period": "week"})
        ))
        .is_err()
    );
    assert!(
        serde_json::from_value::<EngineDocument>(with(
            typed(),
            "budget",
            json!({"calls": 1, "euros": 5})
        ))
        .is_err()
    );
}
