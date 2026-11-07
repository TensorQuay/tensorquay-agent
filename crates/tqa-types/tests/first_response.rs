//! PROTO-U-030: session.config's route snapshot carries the route's first-response bound, `first_response_secs` (5 to 900
//! seconds, revision 10): how long the relay waits for the model's first byte, which the runtime's own wait follows.
//! It's optional: a gateway without it sends none, and the runtime keeps its default.
use serde_json::{Value, json};
use std::error::Error;
use tqa_types::{PROTOCOL_REVISIONS, RouteSnapshot, protocol_schema};

type TestResult = Result<(), Box<dyn Error>>;

fn route(extra: Option<Value>) -> Value {
    let mut route = json!({
        "name":"local","protocol":"openai_chat","model":"qwen3:4b",
        "relay_path":"/relay/r/local","context_window":4096,"max_output":null,
        "native_tools":true,"parallel_tools":true,"vision":false,"reasoning":null
    });
    if let Some(value) = extra {
        route["first_response_secs"] = value;
    }
    route
}
fn schema_accepts(route: &Value) -> Result<bool, Box<dyn Error>> {
    let mut schema = protocol_schema();
    let frame = json!({"type":"session.update","route":route});
    schema["$ref"] = json!("#/definitions/ClientFrame");
    Ok(jsonschema::validator_for(&schema)?.is_valid(&frame))
}

// TEST: PROTO-U-030
#[test]
fn proto_u_030_the_first_response_bound_is_optional_and_bounded() -> TestResult {
    let absent: RouteSnapshot = serde_json::from_value(route(None))?;
    assert_eq!(absent.first_response_secs, None);
    assert!(
        serde_json::to_value(&absent)?
            .get("first_response_secs")
            .is_none()
    );
    assert!(schema_accepts(&route(None))?);
    for secs in [5_u32, 60, 300, 900] {
        let wire = route(Some(json!(secs)));
        let parsed: RouteSnapshot = serde_json::from_value(wire.clone())?;
        assert_eq!(parsed.first_response_secs, Some(secs));
        assert_eq!(serde_json::to_value(&parsed)?, wire);
        assert!(schema_accepts(&wire)?);
    }
    for secs in [0_u32, 4, 901] {
        let wire = route(Some(json!(secs)));
        assert!(
            serde_json::from_value::<RouteSnapshot>(wire.clone()).is_err(),
            "{secs}"
        );
        assert!(!schema_accepts(&wire)?, "{secs}");
    }
    let mut constructed: RouteSnapshot = serde_json::from_value(route(None))?;
    constructed.first_response_secs = Some(901);
    assert!(constructed.validate().is_err());
    Ok(())
}

// TEST: PROTO-U-030
#[test]
fn proto_u_030_revision_10_adds_the_field() -> TestResult {
    let ten = PROTOCOL_REVISIONS
        .iter()
        .find(|revision| revision.number == 10)
        .ok_or("revision 10")?;
    assert!(
        ten.schema
            .contains(&("RouteSnapshot", "first_response_secs"))
    );
    Ok(())
}
