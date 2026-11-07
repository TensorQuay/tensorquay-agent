//! GW-U-074: the decision engine record (ADR-0013 point 6) and its validator, shared by the `[[decision_engines]]` seed
//! and the admin API; every refusal names the field and a documented reason.
use super::{EngineDocument, EngineProtocol, valid_engine_name, validate};
use crate::{InvalidReason, connectors::Origin};
use serde_json::{Value, json};
use tqa_types::DecisionExposure;

fn minimal() -> Value {
    json!({"name":"router","url":"https://engine.example/api"})
}
fn with(field: &str, value: Value) -> Value {
    let mut base = minimal();
    base[field] = value;
    base
}
fn refused(value: Value, origin: Origin) -> Option<(&'static str, InvalidReason)> {
    let document: EngineDocument = serde_json::from_value(value).ok()?;
    validate(&document, origin).err()
}

// TEST: GW-U-074
#[test]
fn gw_u_074_a_minimal_engine_gets_the_documented_defaults() -> Result<(), Box<dyn std::error::Error>>
{
    let parsed: EngineDocument = serde_json::from_value(minimal())?;
    validate(&parsed, Origin::Admin).map_err(|refusal| format!("{refusal:?}"))?;
    assert_eq!(parsed.protocol, EngineProtocol::DecisionV1);
    assert_eq!(parsed.timeout_ms, 2000);
    assert_eq!(parsed.exposure, DecisionExposure::None);
    assert!(parsed.enabled && parsed.key_ref.is_none() && !parsed.insecure_http);
    assert_eq!(
        serde_json::to_value(&parsed)?,
        json!({"name":"router","protocol":"decision_v1","url":"https://engine.example/api",
               "auth":{"scheme":"none","required":false},"timeout_ms":2000,"exposure":"none","enabled":true}),
        "the stored form omits unset and file-only fields"
    );
    Ok(())
}

// TEST: GW-U-074
#[test]
fn gw_u_074_a_full_engine_with_a_key_is_accepted() -> Result<(), Box<dyn std::error::Error>> {
    let full = json!({"name":"typed-router","protocol":"decision_v1","url":"https://engine.example/api",
        "auth":{"scheme":"bearer"},"key_ref":"secrets/decision-engine","timeout_ms":10000,"exposure":"calls",
        "enabled":false});
    let parsed: EngineDocument = serde_json::from_value(full)?;
    validate(&parsed, Origin::Admin).map_err(|refusal| format!("{refusal:?}"))?;
    assert_eq!(parsed.exposure, DecisionExposure::Calls);
    Ok(())
}

// TEST: GW-U-074
#[test]
fn gw_u_074_every_field_rule_names_the_field_and_reason() {
    for (value, origin, expected) in [
        (
            with("name", json!("Router")),
            Origin::Admin,
            ("name", InvalidReason::NameInvalid),
        ),
        (
            with("name", json!("r".repeat(65))),
            Origin::Admin,
            ("name", InvalidReason::NameInvalid),
        ),
        (
            with("name", json!("-router")),
            Origin::Admin,
            ("name", InvalidReason::NameInvalid),
        ),
        (
            with("url", json!("http://engine.example")),
            Origin::Admin,
            ("url", InvalidReason::BaseUrlInvalid),
        ),
        (
            with("url", json!("https://user:pw@engine.example")),
            Origin::Admin,
            ("url", InvalidReason::BaseUrlInvalid),
        ),
        (
            with("url", json!("https://engine.example/?token=x")),
            Origin::Admin,
            ("url", InvalidReason::AuthQueryInvalid),
        ),
        (
            with("insecure_http", json!(true)),
            Origin::Admin,
            ("insecure_http", InvalidReason::FileOnly),
        ),
        (
            with("ca_file", json!("/etc/ca.pem")),
            Origin::Admin,
            ("ca_file", InvalidReason::FileOnly),
        ),
        (
            with("ca_file", json!("relative.pem")),
            Origin::File,
            ("ca_file", InvalidReason::ParameterInvalid),
        ),
        (
            with("key_ref", json!("sk-literal-value")),
            Origin::Admin,
            ("key_ref", InvalidReason::KeyRefNotReference),
        ),
        (
            with("key_ref", json!("secrets/decision-engine")),
            Origin::Admin,
            ("key_ref", InvalidReason::AuthInvalid),
        ),
        (
            with("auth", json!({"scheme":"header","header":"Cookie"})),
            Origin::Admin,
            ("auth", InvalidReason::AuthHeaderInvalid),
        ),
        (
            with("timeout_ms", json!(0)),
            Origin::Admin,
            ("timeout_ms", InvalidReason::LimitOutOfRange),
        ),
        (
            with("timeout_ms", json!(10_001)),
            Origin::Admin,
            ("timeout_ms", InvalidReason::LimitOutOfRange),
        ),
    ] {
        assert_eq!(refused(value.clone(), origin), Some(expected), "{value}");
    }
    // The file may use plain http for tests and on-premises engines.
    let mut local = with("url", json!("http://127.0.0.1:8900"));
    local["insecure_http"] = json!(true);
    assert_eq!(refused(local, Origin::File), None);
}

// TEST: GW-U-074
#[test]
fn gw_u_074_unknown_fields_and_protocols_are_refused_when_parsing() {
    assert!(serde_json::from_value::<EngineDocument>(with("extra", json!(1))).is_err());
    assert!(
        serde_json::from_value::<EngineDocument>(with("protocol", json!("openai_chat"))).is_err()
    );
}

// TEST: GW-U-074
#[test]
fn gw_u_074_engine_names() {
    for name in ["router", "typed-questions", "a", "r_1"] {
        assert!(valid_engine_name(name), "{name}");
    }
    for name in ["", "Router", "1router", "router/x", "rou ter", "décision"] {
        assert!(!valid_engine_name(name), "{name}");
    }
}

// TEST: GW-U-074
#[test]
fn gw_u_074_a_display_name_is_what_people_read_and_is_checked()
-> Result<(), Box<dyn std::error::Error>> {
    let named: EngineDocument =
        serde_json::from_value(with("display_name", json!("Policy engine")))?;
    validate(&named, Origin::Admin).map_err(|refusal| format!("{refusal:?}"))?;
    assert_eq!(named.display_name.as_deref(), Some("Policy engine"));
    assert_eq!(
        serde_json::to_value(&named)?["display_name"],
        "Policy engine"
    );
    for bad in [
        json!(""),
        json!(" padded"),
        json!("two\nlines"),
        json!("x".repeat(81)),
    ] {
        assert_eq!(
            refused(with("display_name", bad.clone()), Origin::Admin),
            Some(("display_name", InvalidReason::DisplayNameInvalid)),
            "{bad}"
        );
    }
    Ok(())
}
