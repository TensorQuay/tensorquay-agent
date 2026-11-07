//! GW-I-060 (unit): the routes API's request inputs, at and past each bound, without a database.
use super::{body, display_name, name, version};
use crate::{GatewayError, InvalidReason};
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// The field and reason of a refusal; anything else fails the test.
fn refused<T: std::fmt::Debug>(
    result: Result<T, GatewayError>,
) -> Result<(&'static str, InvalidReason), Box<dyn std::error::Error>> {
    match result {
        Err(GatewayError::Invalid { field, reason }) => Ok((field, reason)),
        other => Err(format!("expected a field refusal, got {other:?}").into()),
    }
}

fn route() -> Value {
    json!({"protocol": "openai_chat", "base_url": "https://models.example/v1", "model": "m"})
}

// TEST: GW-I-015
#[test]
fn gw_i_060_names_are_1_to_128_bytes_without_control_characters() -> TestResult {
    assert!(name("a").is_ok());
    assert!(name(&"n".repeat(128)).is_ok());
    for bad in [
        String::new(),
        "n".repeat(129),
        "bad\u{1}name".into(),
        "tab\tname".into(),
    ] {
        assert_eq!(
            refused(name(&bad))?,
            ("name", InvalidReason::RouteNameInvalid),
            "{bad:?}"
        );
    }
    Ok(())
}

// TEST: GW-I-015
#[test]
fn gw_i_060_display_names_are_1_to_80_clean_characters_or_absent() -> TestResult {
    assert_eq!(display_name(None)?, None);
    assert_eq!(display_name(Some(Value::Null))?, None);
    assert_eq!(display_name(Some(json!("x")))?, Some("x".into()));
    assert_eq!(
        display_name(Some(json!("é".repeat(80))))?,
        Some("é".repeat(80)),
        "characters, not bytes"
    );
    for bad in [
        json!(""),
        json!("é".repeat(81)),
        json!(" padded"),
        json!("padded "),
        json!("a\u{7}b"),
        json!(5),
        json!([]),
    ] {
        assert_eq!(
            refused(display_name(Some(bad.clone())))?,
            ("display_name", InvalidReason::DisplayNameInvalid),
            "{bad}"
        );
    }
    Ok(())
}

// TEST: GW-I-015
#[test]
fn gw_i_060_versions_are_positive_integers_or_absent() -> TestResult {
    assert_eq!(version(None)?, None);
    assert_eq!(version(Some(Value::Null))?, None);
    assert_eq!(version(Some(json!(1)))?, Some(1));
    assert_eq!(version(Some(json!(i64::MAX)))?, Some(i64::MAX));
    for bad in [json!(0), json!(-1), json!(1.5), json!("1"), json!(true)] {
        assert_eq!(
            refused(version(Some(bad.clone())))?,
            ("version", InvalidReason::VersionInvalid),
            "{bad}"
        );
    }
    Ok(())
}

// TEST: GW-I-015
#[test]
fn gw_i_060_a_body_is_a_route_record_without_its_name() -> TestResult {
    let mut full = route();
    full["display_name"] = json!("Label");
    full["description"] = json!("For quick lookups");
    full["version"] = json!(3);
    let parsed = body("chosen", &serde_json::to_vec(&full)?)?;
    assert_eq!(
        (parsed.config.name.as_str(), parsed.config.model.as_str()),
        ("chosen", "m"),
        "the path names it"
    );
    assert_eq!(
        (parsed.display_name, parsed.description, parsed.expected),
        (
            Some("Label".into()),
            Some("For quick lookups".into()),
            Some(3)
        )
    );
    let bare = body("chosen", &serde_json::to_vec(&route())?)?;
    assert_eq!(
        (
            bare.config.name.as_str(),
            bare.display_name,
            bare.description,
            bare.expected
        ),
        ("chosen", None, None, None)
    );
    let with = |key: &str, value: Value| {
        let mut body = route();
        body[key] = value;
        serde_json::to_vec(&body)
    };
    for bytes in [
        b"[1]".to_vec(),
        b"not json".to_vec(),
        with("name", json!("other"))?,
        with("unknown", json!(1))?,
        with("model", json!(5))?,
    ] {
        assert_eq!(
            refused(body("chosen", &bytes))?,
            ("body", InvalidReason::BodyInvalid),
            "{}",
            String::from_utf8_lossy(&bytes)
        );
    }
    assert_eq!(
        refused(body("chosen", &with("display_name", json!(""))?))?,
        ("display_name", InvalidReason::DisplayNameInvalid)
    );
    assert_eq!(
        refused(body("chosen", &with("version", json!(0))?))?,
        ("version", InvalidReason::VersionInvalid)
    );
    Ok(())
}
