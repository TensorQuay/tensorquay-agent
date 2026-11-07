//! RT-I-008: MCP servers from `session.config` reach a run through the runtime (ADR-0011). One good and one failing
//! server: the good server's tools are offered to the model under their model-facing names, a call runs and is journalled
//! under the internal name, the failure is an `mcp.server` event, and the run and the runtime carry on.
//! RT-I-043: an MCP session's first run, from the message the gateway stored before the sandbox existed (the start-up
//! pass runs it, its state on disk), completes; so does the next run after a restart resumes the session.
#![cfg(target_os = "linux")]
#[path = "support/fixture.rs"]
mod support;
use serde_json::{Value, json};
use support::*;
use tqa_testkit::{Script, Scripted};
use tqa_types::{ClientFrame, Event, McpServerState, ServerFrame};

const FAKE: &str = env!("CARGO_BIN_EXE_tqa-fake-mcp");

fn session_with_mcp() -> Result<ClientFrame, Box<dyn std::error::Error>> {
    let mut value = serde_json::to_value(session()?)?;
    value["mcp"] = json!([
        {"name":"good","transport":{"kind":"stdio","command":FAKE}},
        {"name":"broken","transport":{"kind":"stdio","command":FAKE,"env":{"FAKE_MCP_MODE":"exit_on_start"}}}
    ]);
    Ok(ClientFrame::parse(&serde_json::to_vec(&value)?)?)
}

fn stored_events(f: &Fixture) -> Vec<Event> {
    f.gateway
        .stored()
        .into_iter()
        .filter_map(|frame| match frame {
            ServerFrame::Event { event, .. } => Some(event),
            _ => None,
        })
        .collect()
}
fn server_states(events: &[Event]) -> Vec<(String, McpServerState, u32, Option<String>)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::McpServer {
                name,
                state,
                tools,
                reason,
            } => Some((name.clone(), *state, *tools, reason.clone())),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn rt_i_008_a_good_and_a_failing_server_the_good_one_serves_the_run() -> TestResult {
    let call = Script::new(vec![Scripted::ToolCall {
        name: "mcp__good__echo".into(),
        arguments_json: r#"{"text":"from mcp"}"#.into(),
        split_into: 2,
    }]);
    let f = Fixture::in_memory(vec![call, answer()]).await?;
    f.gateway.set_session(session_with_mcp()?)?;
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(user()).await?;
    f.wait_event("run.completed").await?;
    stop(shutdown, task).await?;
    let events = stored_events(&f);
    let servers = server_states(&events);
    assert_eq!(
        servers,
        [
            ("good".to_owned(), McpServerState::Ready, 6, None),
            (
                "broken".to_owned(),
                McpServerState::Disabled,
                0,
                Some("protocol_error".to_owned())
            ),
        ]
    );
    let calls: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            Event::ToolCall { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        calls,
        ["mcp.good.echo"],
        "journalled under the internal name"
    );
    let result = events
        .iter()
        .find_map(|event| match event {
            Event::ToolResult { content, .. } => Some(content.clone()),
            _ => None,
        })
        .ok_or("no tool result")?;
    assert!(result.contains("from mcp"), "{result}");
    let bodies: Vec<Value> = f
        .gateway
        .model_requests()
        .into_iter()
        .map(|request| request.body)
        .collect();
    let offered: Vec<&str> = bodies[0]["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tool| tool["function"]["name"].as_str())
        .collect();
    assert!(offered.contains(&"mcp__good__echo"), "{offered:?}");
    assert!(
        !offered.iter().any(|name| name.contains("broken")),
        "{offered:?}"
    );
    Ok(())
}

/// The person's first message as the gateway stores it before the sandbox exists (from its wire form, so the frame needs
/// no field this test doesn't care about).
fn stored_message(seq: u64) -> Result<ServerFrame, Box<dyn std::error::Error>> {
    let frame = json!({"type": "event", "seq": seq, "run_id": null, "step": null, "ts": "2000-01-01T00:00:00Z",
                       "event": {"type": "message.user", "text": "first", "attachments": []}});
    Ok(ServerFrame::parse(frame.to_string().as_bytes())?)
}

#[tokio::test]
async fn rt_i_043_an_mcp_sessions_first_run_from_a_stored_message_and_its_resume() -> TestResult {
    // First: the stored message is run by the start-up pass, the servers prepared, the state on disk.
    let f = Fixture::with_history(vec![answer(), answer()], vec![stored_message(1)?]).await?;
    f.gateway.set_session(session_with_mcp()?)?;
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    let first = f.wait_event("run.completed").await;
    stop(shutdown, task).await?;
    first?;
    assert!(
        server_states(&stored_events(&f))
            .iter()
            .any(|(name, state, ..)| name == "good" && *state == McpServerState::Ready),
        "the servers were prepared for the first run"
    );
    // Then a restart: the runtime registers again over the same state and runs the next message.
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(2).await?;
    f.gateway.send(user()).await?;
    let resumed = wait_for(|| {
        stored_events(&f)
            .iter()
            .filter(|event| matches!(event, Event::RunCompleted { .. }))
            .count()
            >= 2
    })
    .await;
    stop(shutdown, task).await?;
    resumed
}
