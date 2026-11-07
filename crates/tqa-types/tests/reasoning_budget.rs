//! PROTO-U-010: the reasoning budget table of a route snapshot (protocol revision 6).
use serde_json::{Value, json};
use std::error::Error;
use tqa_types::{ClientFrame, PROTOCOL_REVISIONS, protocol_schema};

fn config(reasoning: &Value, max_output: &Value) -> Value {
    json!({
        "type":"session.config","conversation_id":"conversation","last_stored_seq":0,
        "route":{
            "name":"thinking","protocol":"anthropic_messages","model":"example-model",
            "relay_path":"/relay/r/thinking","context_window":200_000,"max_output":max_output,
            "native_tools":true,"parallel_tools":true,"vision":false,"reasoning":reasoning
        },
        "network":{"allowed":[],"more":0},
        "limits":{"max_steps":10,"wall_clock_secs":300,"tool_result_bytes":65_536,"max_output_tokens":131_072},
        "workspace":{"display":"/workspace"},"protocol":1
    })
}

fn validator() -> Result<jsonschema::Validator, Box<dyn Error>> {
    let mut schema = protocol_schema();
    schema["$ref"] = json!("#/definitions/ClientFrame");
    Ok(jsonschema::validator_for(&schema)?)
}

fn refusal(value: &Value) -> Result<String, Box<dyn Error>> {
    match ClientFrame::parse(&serde_json::to_vec(value)?) {
        Ok(_) => Err("the frame was accepted".into()),
        Err(error) => Ok(error.detail),
    }
}

#[test]
fn proto_u_010_a_matching_budget_table_round_trips() -> Result<(), Box<dyn Error>> {
    let reasoning = json!({"allowed":["low","high"],"default":"low",
        "budget_tokens":{"high":16_000,"low":1024}});
    let value = config(&reasoning, &json!(32_000));
    let frame = ClientFrame::parse(&serde_json::to_vec(&value)?)?;
    assert_eq!(serde_json::to_value(&frame)?, value);
    assert!(validator()?.is_valid(&value));
    Ok(())
}

#[test]
fn proto_u_010_an_absent_table_is_omitted_and_unchanged() -> Result<(), Box<dyn Error>> {
    let value = config(&json!({"allowed":["low"],"default":"low"}), &json!(4096));
    let frame = ClientFrame::parse(&serde_json::to_vec(&value)?)?;
    let encoded = serde_json::to_value(&frame)?;
    assert_eq!(encoded, value);
    assert_eq!(encoded["route"]["reasoning"].get("budget_tokens"), None);
    assert!(validator()?.is_valid(&value));
    Ok(())
}

#[test]
fn proto_u_010_a_table_that_does_not_match_allowed_is_refused() -> Result<(), Box<dyn Error>> {
    for table in [
        json!({"low":2048}),
        json!({"low":2048,"high":4096,"extra":4096}),
        json!({"low":2048,"medium":4096}),
        json!({"low":1023,"high":4096}),
        json!({"low":2048,"high":1_048_577}),
        json!({}),
    ] {
        let reasoning = json!({"allowed":["low","high"],"default":"low","budget_tokens":table});
        let detail = refusal(&config(&reasoning, &json!(150_000)))?;
        assert!(detail.contains("budget table"), "{table}: {detail}");
    }
    Ok(())
}

#[test]
fn proto_u_010_every_budget_must_be_below_a_declared_max_output() -> Result<(), Box<dyn Error>> {
    let reasoning = json!({"allowed":["low","high"],"default":"low",
        "budget_tokens":{"low":2048,"high":8192}});
    for max_output in [json!(8192), json!(4096), Value::Null] {
        let value = config(&reasoning, &max_output);
        let detail = refusal(&value)?;
        assert!(
            detail.contains("below max_output"),
            "{max_output}: {detail}"
        );
    }
    assert!(ClientFrame::parse(&serde_json::to_vec(&config(&reasoning, &json!(8193)))?).is_ok());
    Ok(())
}

#[test]
fn proto_u_010_revision_6_names_the_budget_table() {
    let sixth = PROTOCOL_REVISIONS
        .iter()
        .find(|revision| revision.number == 6)
        .map(|revision| revision.schema);
    assert_eq!(sixth, Some(&[("ReasoningSnapshot", "budget_tokens")][..]));
}
