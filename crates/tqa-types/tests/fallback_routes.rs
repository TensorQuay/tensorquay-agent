//! CORE-U-041 / PROTO-C-001: fallback route snapshots on the link and the route.fallback event.
use serde_json::{Value, json};
use std::error::Error;
use tqa_types::{ClientFrame, Event, MAX_FALLBACK_ROUTES, protocol_schema};

type TestResult = Result<(), Box<dyn Error>>;

fn route(name: &str) -> Value {
    json!({"name":name,"protocol":"openai_chat","model":"m","relay_path":format!("/relay/r/{name}"),
        "context_window":8192,"max_output":1024,"native_tools":false,"parallel_tools":false,"vision":false,
        "reasoning":null})
}
fn config(fallbacks: Option<Value>) -> Value {
    let mut frame = json!({
        "type":"session.config","conversation_id":"conversation","last_stored_seq":0,
        "route":route("local"),"network":{"allowed":[],"more":0},
        "limits":{"max_steps":10,"wall_clock_secs":300,"tool_result_bytes":65_536,"max_output_tokens":131_072},
        "workspace":{"display":"/workspace"},"protocol":1
    });
    if let Some(value) = fallbacks {
        frame["fallback_routes"] = value;
    }
    frame
}
fn schema_accepts(definition: &str, value: &Value) -> Result<bool, Box<dyn Error>> {
    let mut schema = protocol_schema();
    schema["$ref"] = json!(format!("#/definitions/{definition}"));
    Ok(jsonschema::validator_for(&schema)?.is_valid(value))
}

#[test]
fn core_u_041_session_frames_carry_up_to_three_fallback_routes() -> TestResult {
    let absent = config(None);
    let parsed = ClientFrame::parse(&serde_json::to_vec(&absent)?)?;
    assert_eq!(
        serde_json::to_value(&parsed)?,
        absent,
        "an empty list is omitted"
    );
    let three = json!([route("a"), route("b"), route("c")]);
    let wire = config(Some(three.clone()));
    let parsed = ClientFrame::parse(&serde_json::to_vec(&wire)?)?;
    assert_eq!(serde_json::to_value(&parsed)?, wire);
    assert!(schema_accepts("ClientFrame", &wire)?);
    let update = json!({"type":"session.update","route":route("local"),"fallback_routes":three});
    let parsed = ClientFrame::parse(&serde_json::to_vec(&update)?)?;
    assert_eq!(serde_json::to_value(&parsed)?, update);
    assert_eq!(MAX_FALLBACK_ROUTES, 3);
    let mut bad_path = route("x");
    bad_path["relay_path"] = json!("/other/x");
    for invalid in [
        config(Some(json!([
            route("a"),
            route("b"),
            route("c"),
            route("d")
        ]))),
        config(Some(json!([route("a"), route("a")]))),
        config(Some(json!([route("local")]))),
        config(Some(json!([bad_path]))),
    ] {
        assert!(
            ClientFrame::parse(&serde_json::to_vec(&invalid)?).is_err(),
            "{invalid}"
        );
    }
    assert!(!schema_accepts(
        "ClientFrame",
        &config(Some(json!([
            route("a"),
            route("b"),
            route("c"),
            route("d")
        ])))
    )?);
    Ok(())
}

#[test]
fn core_u_041_route_fallback_event_is_bounded() -> TestResult {
    let wire = json!({"type":"route.fallback","from":"local","to":"backup","reason":"http 503"});
    let parsed: Event = serde_json::from_value(wire.clone())?;
    assert_eq!(serde_json::to_value(&parsed)?, wire);
    assert!(schema_accepts("Event", &wire)?);
    let long = "r".repeat(129);
    let long_name = "n".repeat(257);
    for invalid in [
        json!({"type":"route.fallback","from":"local","to":"backup","reason":long}),
        json!({"type":"route.fallback","from":"","to":"backup","reason":"x"}),
        json!({"type":"route.fallback","from":"local","to":long_name,"reason":"x"}),
        json!({"type":"route.fallback","from":"local","to":"backup"}),
    ] {
        assert!(
            serde_json::from_value::<Event>(invalid.clone()).is_err(),
            "{invalid}"
        );
        assert!(!schema_accepts("Event", &invalid)?, "{invalid}");
    }
    let frame = json!({"type":"event","seq":3,"run_id":"r-1","step":2,"ts":"2026-09-22T00:00:00Z","event":wire});
    assert!(tqa_types::ServerFrame::parse(&serde_json::to_vec(&frame)?).is_ok());
    Ok(())
}
