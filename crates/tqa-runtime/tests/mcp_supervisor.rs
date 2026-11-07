//! TOOL-I-012 (supervision part) and RT-I-008 (unit part): the session's MCP servers across runs (ADR-0011 decision 3).
//! Tools are registered as `mcp.<server>.<tool>` through the server's filter; a crash is restarted once
//! and a second crash disables the server; a server that cannot start is disabled with a reason while the others keep
//! working; a changed tool list applies at the next run; every transition is an `mcp.server` event.
#![cfg(target_os = "linux")]
use std::{error::Error, time::Duration};

use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use tqa_runtime::{SupervisedGroups, testing::McpSupervisor};
use tqa_tools::{ToolContext, ToolRegistry};
use tqa_types::{Event, McpServer, McpServerState};

type TestResult = Result<(), Box<dyn Error>>;

const FAKE: &str = env!("CARGO_BIN_EXE_tqa-fake-mcp");

fn stdio(name: &str, extra: &Value) -> Result<McpServer, Box<dyn Error>> {
    let mut server = json!({"name":name,"transport":{"kind":"stdio","command":FAKE,"env":{}}});
    if let (Some(server), Some(extra)) = (server.as_object_mut(), extra.as_object()) {
        for (key, value) in extra {
            if key == "env" {
                server["transport"]["env"] = value.clone();
            } else {
                server.insert(key.clone(), value.clone());
            }
        }
    }
    Ok(serde_json::from_value(server)?)
}
fn states(events: &[Event]) -> Vec<(String, McpServerState, u32, Option<String>)> {
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
fn names(registry: &ToolRegistry) -> Vec<String> {
    registry
        .specs()
        .map(|spec| spec.name.as_str().to_owned())
        .collect()
}
fn context(workspace: &std::path::Path) -> Result<ToolContext, Box<dyn Error>> {
    let clock = tqa_testkit::FakeClock::new();
    Ok(ToolContext {
        workspace: tqa_tools::WorkspaceRoot::open(workspace)?,
        cancel: CancellationToken::new(),
        clock: std::sync::Arc::new(clock.clone()),
        sleeper: std::sync::Arc::new(tqa_testkit::FakeSleeper::new(clock)),
        env_allow: vec![],
        limits: tqa_tools::ToolLimits::RECOMMENDED,
    })
}

#[tokio::test]
async fn tool_i_012_tools_register_through_the_filter() -> TestResult {
    let workspace = tempfile::tempdir()?;
    let groups = SupervisedGroups::default();
    let mut supervisor = McpSupervisor::new(groups.clone(), Duration::from_secs(10), None);
    supervisor
        .configure(&[stdio("fake", &json!({"tools":{"allow":["echo","image"]}}))?])
        .await;
    let mut registry = ToolRegistry::new();
    let events = supervisor
        .prepare_run(&mut registry, workspace.path())
        .await;
    assert_eq!(
        states(&events),
        [("fake".into(), McpServerState::Ready, 2, None)]
    );
    assert_eq!(names(&registry), ["mcp.fake.echo", "mcp.fake.image"]);
    // The next run: no transition, the same tools again.
    let mut next = ToolRegistry::new();
    assert!(states(&supervisor.prepare_run(&mut next, workspace.path()).await).is_empty());
    assert_eq!(names(&next), names(&registry));
    supervisor.stop_all().await;
    Ok(())
}

#[tokio::test]
async fn tool_i_012_a_crash_is_restarted_once_and_a_second_disables_the_server() -> TestResult {
    let workspace = tempfile::tempdir()?;
    let groups = SupervisedGroups::default();
    let mut supervisor = McpSupervisor::new(groups.clone(), Duration::from_secs(10), None);
    supervisor.configure(&[stdio("fake", &json!({}))?]).await;
    let cx = context(workspace.path())?;
    let mut expected = vec![McpServerState::Ready, McpServerState::Restarted];
    for round in 0..3 {
        let mut registry = ToolRegistry::new();
        let events = states(
            &supervisor
                .prepare_run(&mut registry, workspace.path())
                .await,
        );
        if round < 2 {
            assert_eq!(events.len(), 1, "{events:?}");
            assert_eq!(events[0].1, expected.remove(0));
            let crash = registry.get("mcp.fake.crash").ok_or("crash tool")?;
            let failure = crash
                .call(json!({}), &cx)
                .await
                .err()
                .ok_or("the crash fails the call")?;
            assert!(
                failure.model_message().starts_with("mcp_server_exited: "),
                "{}",
                failure.model_message()
            );
            tokio::time::sleep(Duration::from_millis(200)).await;
        } else {
            assert_eq!(
                events,
                [(
                    "fake".into(),
                    McpServerState::Disabled,
                    0,
                    Some("crashed".into())
                )]
            );
            assert!(
                names(&registry).is_empty(),
                "a disabled server offers nothing"
            );
        }
    }
    supervisor.stop_all().await;
    Ok(())
}

#[tokio::test]
async fn rt_i_008_a_server_that_cannot_start_is_disabled_and_the_others_keep_working() -> TestResult
{
    let workspace = tempfile::tempdir()?;
    let mut supervisor =
        McpSupervisor::new(SupervisedGroups::default(), Duration::from_secs(10), None);
    let missing: McpServer = serde_json::from_value(
        json!({"name":"missing","transport":{"kind":"stdio","command":"/nonexistent/server"}}),
    )?;
    let remote: McpServer = serde_json::from_value(
        json!({"name":"remote","transport":{"kind":"gateway","connector":"remote"}}),
    )?;
    supervisor
        .configure(&[
            stdio("good", &json!({}))?,
            stdio("broken", &json!({"env":{"FAKE_MCP_MODE":"exit_on_start"}}))?,
            missing,
            remote,
        ])
        .await;
    let mut registry = ToolRegistry::new();
    let events = states(
        &supervisor
            .prepare_run(&mut registry, workspace.path())
            .await,
    );
    assert_eq!(
        events,
        [
            ("good".into(), McpServerState::Ready, 6, None),
            (
                "broken".into(),
                McpServerState::Disabled,
                0,
                Some("protocol_error".into())
            ),
            (
                "missing".into(),
                McpServerState::Disabled,
                0,
                Some("command_not_found".into())
            ),
            (
                "remote".into(),
                McpServerState::Disabled,
                0,
                Some("transport_unavailable".into())
            ),
        ]
    );
    assert!(
        names(&registry)
            .iter()
            .all(|name| name.starts_with("mcp.good."))
    );
    let echo = registry.get("mcp.good.echo").ok_or("echo")?;
    let output = echo
        .call(json!({"text":"up"}), &context(workspace.path())?)
        .await?;
    // The server's text comes back marked as untrusted content (SEC-040).
    assert_eq!(
        output.text,
        tqa_tools::untrusted::wrap("mcp", &[("server", "good"), ("tool", "echo")], "up")?
    );
    // Disabled servers are reported once, not on every run.
    assert!(
        states(
            &supervisor
                .prepare_run(&mut ToolRegistry::new(), workspace.path())
                .await
        )
        .is_empty()
    );
    supervisor.stop_all().await;
    Ok(())
}

#[tokio::test]
async fn tool_i_012_a_changed_list_applies_at_the_next_run_and_reconfiguring_stops_removed_servers()
-> TestResult {
    let workspace = tempfile::tempdir()?;
    let groups = SupervisedGroups::default();
    let mut supervisor = McpSupervisor::new(groups.clone(), Duration::from_secs(10), None);
    let changing = stdio("changing", &json!({"env":{"FAKE_MCP_MODE":"list_changed"}}))?;
    supervisor.configure(std::slice::from_ref(&changing)).await;
    let first = states(
        &supervisor
            .prepare_run(&mut ToolRegistry::new(), workspace.path())
            .await,
    );
    assert_eq!(first[0].1, McpServerState::Ready);
    tokio::time::sleep(Duration::from_millis(200)).await;
    // The server announced a changed list after its handshake: the next run lists it afresh.
    let second = states(
        &supervisor
            .prepare_run(&mut ToolRegistry::new(), workspace.path())
            .await,
    );
    assert_eq!(second.len(), 1, "{second:?}");
    assert_eq!(second[0].1, McpServerState::Ready);
    // Removing the server from the session's list stops it and forgets its group.
    supervisor.configure(&[]).await;
    let mut registry = ToolRegistry::new();
    assert!(
        states(
            &supervisor
                .prepare_run(&mut registry, workspace.path())
                .await
        )
        .is_empty()
    );
    assert!(names(&registry).is_empty());
    supervisor.stop_all().await;
    Ok(())
}
