//! RT-I-020: a two-step tool run on an `openai_responses` route through the relay's Responses path.
#[path = "support/fixture.rs"]
mod support;
use serde_json::{Value, json};
use support::*;
use tqa_testkit::{Script, Scripted};
use tqa_types::{ClientFrame, Event, ServerFrame};

fn responses_session() -> Result<ClientFrame, Box<dyn std::error::Error>> {
    let mut value = serde_json::to_value(session()?)?;
    value["route"]["protocol"] = json!("openai_responses");
    Ok(ClientFrame::parse(&serde_json::to_vec(&value)?)?)
}

#[tokio::test]
async fn rt_i_020_a_tool_run_on_a_responses_route_uses_the_responses_relay_path() -> TestResult {
    let call = Script::new(vec![Scripted::ToolCall {
        name: "shell".into(),
        arguments_json: json!({"command":"echo relay-ok"}).to_string(),
        split_into: 2,
    }]);
    let f = Fixture::new(vec![call, answer()]).await?;
    f.gateway.set_session(responses_session()?)?;
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
    assert_eq!(paths, vec!["/relay/r/local/responses".to_owned(); 2]);
    let bodies: Vec<Value> = f
        .gateway
        .model_requests()
        .into_iter()
        .map(|request| {
            assert_eq!(request.path, "/v1/responses");
            request.body
        })
        .collect();
    assert_eq!(bodies.len(), 2);
    assert!(
        bodies
            .iter()
            .all(|body| body["store"] == json!(false) && body.get("messages").is_none())
    );
    let output = bodies[1]["input"]
        .as_array()
        .ok_or("no input")?
        .iter()
        .find(|item| item["type"] == "function_call_output")
        .ok_or("the tool result was not sent back")?
        .clone();
    assert!(
        output["output"]
            .as_str()
            .is_some_and(|text| text.contains("relay-ok")),
        "{output}"
    );
    let stored = f.gateway.stored();
    let completed_tool = stored.iter().any(|frame| {
        matches!(frame, ServerFrame::Event { event: Event::ToolResult { status, .. }, .. }
            if *status == tqa_types::ToolStatus::Ok)
    });
    assert!(completed_tool, "the shell call ran");
    Ok(())
}
