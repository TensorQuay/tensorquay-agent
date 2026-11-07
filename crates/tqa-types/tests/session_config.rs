//! PROTO-C-001/PROTO-U-003: link configuration round trips and enforces run budgets.
use serde_json::{Value, json};
use std::error::Error;
use tqa_types::{ClientFrame, protocol_schema};

fn config() -> Value {
    json!({
        "type":"session.config","conversation_id":"conversation","last_stored_seq":42,
        "route":{
            "name":"local","protocol":"openai_chat","model":"example-model",
            "relay_path":"/relay/r/local","context_window":32_768,"max_output":4096,
            "native_tools":true,"parallel_tools":true,"vision":false,
            "reasoning":{"allowed":["low","high"],"default":"low"}
        },
        "network":{"allowed":[{"host":"api.example.com","ports":[443]}],"more":0},
        "limits":{"max_steps":10,"wall_clock_secs":300,"tool_result_bytes":65_536,"max_output_tokens":131_072},
        "workspace":{"display":"/workspace"},"protocol":1
    })
}

fn validator() -> Result<jsonschema::Validator, Box<dyn Error>> {
    let mut schema = protocol_schema();
    schema["$ref"] = json!("#/definitions/ClientFrame");
    Ok(jsonschema::validator_for(&schema)?)
}

#[test]
fn proto_c_001_session_config_round_trip_and_schema() -> Result<(), Box<dyn Error>> {
    let validator = validator()?;
    for protocol in ["openai_chat", "openai_responses", "anthropic_messages"] {
        let mut value = config();
        value["route"]["protocol"] = json!(protocol);
        let frame = ClientFrame::parse(&serde_json::to_vec(&value)?)?;
        assert_eq!(serde_json::to_value(&frame)?, value);
        assert!(validator.is_valid(&value));
    }
    Ok(())
}

#[test]
fn proto_c_001_session_config_accepts_optional_workspace_clone() -> Result<(), Box<dyn Error>> {
    let mut value = config();
    value["workspace"]["clone"] = json!({"url":"https://git.example/repository.git", "ref":"main"});
    let frame = ClientFrame::parse(&serde_json::to_vec(&value)?)?;
    assert_eq!(serde_json::to_value(frame)?, value);
    assert!(validator()?.is_valid(&value));
    Ok(())
}

#[test]
fn proto_u_003_session_config_numeric_boundaries() -> Result<(), Box<dyn Error>> {
    let validator = validator()?;
    for (field, minimum, maximum) in [
        ("max_steps", 1, 500),
        ("wall_clock_secs", 1, 86_400),
        ("tool_result_bytes", 1024, 262_144),
    ] {
        for number in [minimum - 1, minimum, maximum, maximum + 1] {
            let valid = (minimum..=maximum).contains(&number);
            let mut value = config();
            value["limits"][field] = json!(number);
            assert_eq!(
                ClientFrame::parse(&serde_json::to_vec(&value)?).is_ok(),
                valid,
                "{field}={number}"
            );
            assert_eq!(validator.is_valid(&value), valid, "{field}={number}");
        }
    }
    Ok(())
}

#[test]
fn proto_u_003_session_config_remains_exact() -> Result<(), Box<dyn Error>> {
    for field in ["route", "network", "limits", "workspace"] {
        let mut value = config();
        value[field]["unexpected"] = json!(true);
        assert!(ClientFrame::parse(&serde_json::to_vec(&value)?).is_err());
    }
    let mut value = config();
    value["unexpected"] = json!(true);
    assert!(ClientFrame::parse(&serde_json::to_vec(&value)?).is_err());
    Ok(())
}

#[test]
fn proto_u_003_run_limits_direct_deserialization() {
    for (field, minimum, maximum) in [
        ("max_steps", 1, 500),
        ("wall_clock_secs", 1, 86_400),
        ("tool_result_bytes", 1024, 262_144),
    ] {
        for number in [minimum - 1, minimum, maximum, maximum + 1] {
            let mut value = config()["limits"].clone();
            value[field] = json!(number);
            assert_eq!(
                serde_json::from_value::<tqa_types::RunLimits>(value).is_ok(),
                (minimum..=maximum).contains(&number),
                "{field}={number}"
            );
        }
    }
}

#[test]
fn proto_u_003_session_snapshot_constraints() -> Result<(), Box<dyn Error>> {
    for (pointer, value) in [
        ("/route/name", json!("")),
        ("/route/model", json!("x".repeat(257))),
        ("/route/model", json!("")),
        ("/route/context_window", json!(0)),
        ("/route/max_output", json!(0)),
        ("/route/max_output", json!(40_000)),
        ("/route/reasoning/allowed", json!([])),
        ("/route/reasoning/allowed", json!(["low", "low"])),
        ("/route/reasoning/allowed", json!(["x".repeat(65)])),
        ("/route/reasoning/default", json!("unknown")),
        ("/workspace/display", json!("")),
        ("/workspace/display", json!("x".repeat(4097))),
        ("/workspace/display", json!("line\nbreak")),
    ] {
        let mut frame = config();
        *frame.pointer_mut(pointer).ok_or("missing fixture field")? = value;
        assert!(
            ClientFrame::parse(&serde_json::to_vec(&frame)?).is_err(),
            "{pointer}"
        );
    }
    Ok(())
}

#[test]
fn proto_u_003_relay_path_is_an_absolute_local_route() -> Result<(), Box<dyn Error>> {
    for path in [
        "relay/r/local",
        "//relay/r/local",
        "/relay/r/",
        "/relay/r//local",
        "/other/local",
        "/relay/r/../local",
        "/relay/r/local?key=value",
        "/relay/r/local#fragment",
        "/relay/r/非ascii",
        "/relay/r/line\nbreak",
        "/relay/r/zero\0byte",
    ] {
        let mut frame = config();
        frame["route"]["relay_path"] = json!(path);
        assert!(
            ClientFrame::parse(&serde_json::to_vec(&frame)?).is_err(),
            "{path:?}"
        );
    }
    Ok(())
}

#[test]
fn proto_c_001_budget_retry_and_step_duration_events() -> Result<(), Box<dyn Error>> {
    for value in [
        json!({"type":"budget.warning","budget":"steps","used":8,"limit":10}),
        json!({"type":"budget.warning","budget":"wall_clock","used":80,"limit":100}),
        json!({"type":"provider.retry","attempt":2,"delay_ms":750,"reason":"http 503"}),
        json!({"type":"step.completed","step":1,"duration_ms":123}),
    ] {
        let event: tqa_types::Event = serde_json::from_value(value.clone())?;
        assert_eq!(serde_json::to_value(event)?, value);
        let mut schema = protocol_schema();
        schema["$ref"] = json!("#/definitions/Event");
        assert!(jsonschema::validator_for(&schema)?.is_valid(&value));
    }
    let event: tqa_types::Event =
        serde_json::from_value(json!({"type":"step.completed","step":1}))?;
    assert_eq!(serde_json::to_value(event)?["duration_ms"], json!(0));
    assert!(
        serde_json::from_value::<tqa_types::Event>(json!({
            "type":"provider.retry","attempt":1,"delay_ms":1,"reason":"x".repeat(129)
        }))
        .is_err()
    );
    Ok(())
}
