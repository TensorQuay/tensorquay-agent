//! PROTO-U-013: the route's cache hint on the link (revision 7). The runtime learns whether a route preserves stable
//! prefixes or takes explicit cache-control blocks; the default `none` is left out, so older frames read unchanged.
use serde_json::{Value, json};
use std::error::Error;
use tqa_types::{CacheHint, ClientFrame, RouteSnapshot, protocol_schema};

type TestResult = Result<(), Box<dyn Error>>;

fn route(name: &str, hint: Option<&str>) -> Value {
    let mut route = json!({"name":name,"protocol":"anthropic_messages","model":"m",
        "relay_path":format!("/relay/r/{name}"),"context_window":8192,"max_output":1024,
        "native_tools":true,"parallel_tools":true,"vision":false,"reasoning":null});
    if let Some(hint) = hint {
        route["cache_hint"] = json!(hint);
    }
    route
}
fn schema_accepts(definition: &str, value: &Value) -> Result<bool, Box<dyn Error>> {
    let mut schema = protocol_schema();
    schema["$ref"] = json!(format!("#/definitions/{definition}"));
    Ok(jsonschema::validator_for(&schema)?.is_valid(value))
}

#[test]
fn proto_u_013_absent_is_none_and_left_out() -> TestResult {
    let absent = route("local", None);
    let parsed: RouteSnapshot = serde_json::from_value(absent.clone())?;
    assert_eq!(parsed.cache_hint, CacheHint::None);
    assert_eq!(serde_json::to_value(&parsed)?, absent);
    assert_eq!(CacheHint::default(), CacheHint::None);
    // An explicit none reads the same and is written without the field.
    let explicit: RouteSnapshot = serde_json::from_value(route("local", Some("none")))?;
    assert_eq!(explicit, parsed);
    assert_eq!(serde_json::to_value(&explicit)?, absent);
    Ok(())
}

#[test]
fn proto_u_013_declared_hints_round_trip_on_every_route_the_link_carries() -> TestResult {
    for hint in ["prefix", "anthropic_cache_control"] {
        let value = route("local", Some(hint));
        let parsed: RouteSnapshot = serde_json::from_value(value.clone())?;
        assert_eq!(serde_json::to_value(&parsed)?, value, "{hint}");
        assert!(schema_accepts("RouteSnapshot", &value)?, "{hint}");
        let update = json!({"type":"session.update","route":value,
            "fallback_routes":[route("backup", Some(hint))]});
        let frame = ClientFrame::parse(&serde_json::to_vec(&update)?)?;
        assert_eq!(serde_json::to_value(&frame)?, update, "{hint}");
        assert!(schema_accepts("ClientFrame", &update)?, "{hint}");
    }
    assert_eq!(
        serde_json::to_value([CacheHint::Prefix, CacheHint::AnthropicCacheControl])?,
        json!(["prefix", "anthropic_cache_control"])
    );
    Ok(())
}

#[test]
fn proto_u_013_unknown_hints_are_refused() -> TestResult {
    for hint in [
        json!("Prefix"),
        json!("auto"),
        json!(""),
        json!(null),
        json!(1),
    ] {
        let mut value = route("local", None);
        value["cache_hint"] = hint.clone();
        assert!(
            serde_json::from_value::<RouteSnapshot>(value.clone()).is_err(),
            "{hint}"
        );
        assert!(!schema_accepts("RouteSnapshot", &value)?, "{hint}");
    }
    Ok(())
}
