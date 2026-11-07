//! PROTO-C-001/PROTO-U-003: link-only route updates use the exact configuration snapshot.
use serde_json::{Value, json};
use std::error::Error;
use tqa_types::{ClientFrame, ErrorCode, protocol_schema};

type TestResult = Result<(), Box<dyn Error>>;

fn update() -> Value {
    json!({"type":"session.update","route":{
        "name":"second","protocol":"openai_chat","model":"second-model",
        "relay_path":"/relay/r/second","context_window":32768,"max_output":4096,
        "native_tools":true,"parallel_tools":true,"vision":false,
        "reasoning":{"allowed":["low","high"],"default":"low"}
    }})
}

#[test]
fn proto_c_001_session_update_round_trips_with_the_configuration_route_schema() -> TestResult {
    let mut schema = protocol_schema();
    let variants = schema["definitions"]["ClientFrame"]["oneOf"]
        .as_array()
        .ok_or("client variants")?;
    let route_schema = |kind| {
        variants.iter().find_map(|variant| {
            (variant["properties"]["type"]["const"] == kind)
                .then_some(&variant["properties"]["route"])
        })
    };
    let config_route = route_schema("session.config").ok_or("configuration schema")?;
    let update_route = route_schema("session.update").ok_or("update schema")?;
    assert_eq!(update_route, config_route);
    schema["$ref"] = json!("#/definitions/ClientFrame");
    let validator = jsonschema::validator_for(&schema)?;
    for protocol in ["openai_chat", "openai_responses", "anthropic_messages"] {
        let mut value = update();
        value["route"]["protocol"] = json!(protocol);
        let frame = ClientFrame::parse(&serde_json::to_vec(&value)?)?;
        assert_eq!(serde_json::to_value(frame)?, value);
        assert!(validator.is_valid(&value));
    }
    Ok(())
}

#[test]
fn proto_u_003_session_update_rejects_missing_extra_and_invalid_route_fields() -> TestResult {
    let mut cases = vec![json!({"type":"session.update"})];
    let mut extra = update();
    extra["policy"] = json!({"policy":"unrestricted"});
    cases.push(extra);
    for (key, value) in [
        ("unexpected", json!(true)),
        ("name", json!("")),
        ("model", json!("")),
        ("context_window", json!(0)),
        ("max_output", json!(40_000)),
        ("relay_path", json!("//upstream.invalid/chat")),
        ("reasoning", json!({"allowed":["low"],"default":"high"})),
    ] {
        let mut invalid = update();
        invalid["route"][key] = value;
        cases.push(invalid);
    }
    for value in cases {
        let error = ClientFrame::parse(&serde_json::to_vec(&value)?)
            .err()
            .ok_or("accepted invalid route update")?;
        assert_eq!(error.code, ErrorCode::InvalidFrame, "{value}");
    }
    Ok(())
}
