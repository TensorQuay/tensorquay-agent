//! RT-I-007: a two-step tool run on an `anthropic_messages` route through the relay's Messages path.
#[path = "support/fixture.rs"]
mod support;
use serde_json::{Value, json};
use support::*;
use tqa_testkit::{Script, Scripted};
use tqa_types::{ClientFrame, Event, ServerFrame};

fn messages_session() -> Result<ClientFrame, Box<dyn std::error::Error>> {
    let mut value = serde_json::to_value(session()?)?;
    value["route"]["protocol"] = json!("anthropic_messages");
    Ok(ClientFrame::parse(&serde_json::to_vec(&value)?)?)
}

#[tokio::test]
async fn rt_i_007_a_tool_run_on_a_messages_route_uses_the_messages_relay_path() -> TestResult {
    let call = Script::new(vec![Scripted::ToolCall {
        name: "shell".into(),
        arguments_json: json!({"command":"echo relay-ok"}).to_string(),
        split_into: 2,
    }]);
    let f = Fixture::new(vec![call, answer()]).await?;
    f.gateway.set_session(messages_session()?)?;
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(user()).await?;
    f.wait_event("run.completed").await?;
    stop(shutdown, task).await?;
    let paths: Vec<String> = f
        .gateway
        .requests()
        .into_iter()
        .filter(|request| request.path.starts_with("/relay/"))
        .map(|request| request.path)
        .collect();
    assert_eq!(paths, vec!["/relay/r/local/messages".to_owned(); 2]);
    let bodies: Vec<Value> = f
        .gateway
        .model_requests()
        .into_iter()
        .map(|request| {
            assert_eq!(request.path, "/v1/messages");
            request.body
        })
        .collect();
    assert_eq!(bodies.len(), 2);
    assert!(bodies.iter().all(|body| body["max_tokens"] == json!(4096)
        && body["stream"] == json!(true)
        && body["system"].is_array()));
    let turns = bodies[1]["messages"].as_array().ok_or("no messages")?;
    let result = turns
        .iter()
        .flat_map(|turn| turn["content"].as_array().into_iter().flatten())
        .find(|block| block["type"] == "tool_result")
        .ok_or("the tool result was not sent back")?;
    assert!(
        result["content"]
            .as_str()
            .is_some_and(|text| text.contains("relay-ok")),
        "{result}"
    );
    let stored = f.gateway.stored();
    let completed_tool = stored.iter().any(|frame| {
        matches!(frame, ServerFrame::Event { event: Event::ToolResult { status, .. }, .. }
            if *status == tqa_types::ToolStatus::Ok)
    });
    assert!(completed_tool, "the shell call ran");
    f.gateway.shutdown().await?;
    Ok(())
}

/// Cache markers (`cache_control: {type: "ephemeral"}`) in one request body: on system blocks, and on tools.
fn markers(body: &Value) -> (usize, usize) {
    let marked = |items: &Value| {
        items.as_array().map_or(0, |items| {
            items
                .iter()
                .filter(|item| item["cache_control"] == json!({"type": "ephemeral"}))
                .count()
        })
    };
    (marked(&body["system"]), marked(&body["tools"]))
}

/// Whether the last system block of a request body carries the cache marker.
fn last_system_marked(body: &Value) -> bool {
    body["system"]
        .as_array()
        .and_then(|system| system.last())
        .is_some_and(|last| last["cache_control"] == json!({"type": "ephemeral"}))
}

/// RT-I-007: the route's `cache_hint` (revision 7) reaches the runtime's own requests. With
/// `anthropic_cache_control`, every Messages request marks the last system block and the last tool; without a hint,
/// nothing is marked.
#[tokio::test]
async fn rt_i_007_a_messages_route_with_a_cache_hint_marks_the_stable_prefix() -> TestResult {
    for (hint, expected) in [(Some("anthropic_cache_control"), (1, 1)), (None, (0, 0))] {
        let f = Fixture::new(vec![answer()]).await?;
        let mut value = serde_json::to_value(messages_session()?)?;
        if let Some(hint) = hint {
            value["route"]["cache_hint"] = json!(hint);
        }
        f.gateway
            .set_session(ClientFrame::parse(&serde_json::to_vec(&value)?)?)?;
        f.gateway.set_bound(true);
        let (shutdown, task) = f.launch()?;
        f.gateway.wait_for_registrations(1).await?;
        f.gateway.send(user()).await?;
        f.wait_event("run.completed").await?;
        stop(shutdown, task).await?;
        let bodies: Vec<Value> = f
            .gateway
            .model_requests()
            .into_iter()
            .map(|request| request.body)
            .collect();
        assert!(!bodies.is_empty());
        for body in &bodies {
            assert_eq!(markers(body), expected, "{hint:?}");
            assert_eq!(
                last_system_marked(body),
                expected.0 == 1,
                "the last system block carries the marker"
            );
        }
    }
    Ok(())
}
