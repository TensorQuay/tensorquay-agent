//! TOOL-I-012 (connection part): a stdio MCP server started by the runtime (ADR-0011 decisions 3 and 6). The handshake,
//! discovery with its bounds, calls and their result mapping, timeouts and cancellation that send
//! `notifications/cancelled`, a crash mid-call, and the typed failures of a server that hangs, floods a line or writes
//! garbage. The server is the `tqa-fake-mcp` test binary.
#![cfg(target_os = "linux")]
use std::{collections::BTreeMap, error::Error, path::Path, time::Duration};

use serde_json::json;
use tokio_util::sync::CancellationToken;
use tqa_runtime::testing::{McpCallError, McpFailure, StdioServer};
use tqa_types::McpServer;

type TestResult = Result<(), Box<dyn Error>>;

const FAKE: &str = env!("CARGO_BIN_EXE_tqa-fake-mcp");
const HANDSHAKE: Duration = Duration::from_secs(10);

fn server(env: &[(&str, &str)]) -> Result<McpServer, Box<dyn Error>> {
    let env: BTreeMap<String, String> = env
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect();
    Ok(serde_json::from_value(json!({
        "name":"fake","transport":{"kind":"stdio","command":FAKE,"args":[],"env":env}
    }))?)
}
fn log_of(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

#[tokio::test]
async fn tool_i_012_handshake_lists_tools_and_calls_map_their_results() -> TestResult {
    let workspace = tempfile::tempdir()?;
    let log = workspace.path().join("methods.log");
    let log_text = log.to_string_lossy().into_owned();
    let started = StdioServer::start(
        &server(&[("FAKE_MCP_LOG", &log_text)])?,
        workspace.path(),
        HANDSHAKE,
    )
    .await?;
    let names: Vec<_> = started
        .tools()
        .iter()
        .map(|tool| tool.name.as_str())
        .collect();
    assert_eq!(
        names,
        ["echo", "slow", "crash", "fail", "structured", "image"]
    );
    assert!(started.process_group() > 1);
    let cancel = CancellationToken::new();
    let timeout = Duration::from_secs(5);
    let echo = started
        .call("echo", json!({"text":"hello"}), timeout, &cancel)
        .await?;
    assert_eq!((echo.text.as_str(), echo.is_error), ("hello", false));
    let fail = started.call("fail", json!({}), timeout, &cancel).await?;
    assert_eq!((fail.text.as_str(), fail.is_error), ("boom", true));
    let structured = started
        .call("structured", json!({}), timeout, &cancel)
        .await?;
    assert_eq!(structured.text, r#"{"a":1,"b":[true]}"#);
    let image = started.call("image", json!({}), timeout, &cancel).await?;
    assert_eq!(image.text, "[image/png content omitted]");
    started.stop().await;
    let methods = log_of(&log);
    for method in [
        "initialize",
        "notifications/initialized",
        "tools/list",
        "tools/call",
    ] {
        assert!(
            methods.lines().any(|line| line == method),
            "{method}: {methods}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn tool_i_012_a_timeout_or_a_cancelled_run_sends_notifications_cancelled() -> TestResult {
    let workspace = tempfile::tempdir()?;
    let log = workspace.path().join("methods.log");
    let log_text = log.to_string_lossy().into_owned();
    let started = StdioServer::start(
        &server(&[("FAKE_MCP_LOG", &log_text)])?,
        workspace.path(),
        HANDSHAKE,
    )
    .await?;
    let never = CancellationToken::new();
    let timed_out = started
        .call(
            "slow",
            json!({"ms":5000}),
            Duration::from_millis(200),
            &never,
        )
        .await;
    assert!(
        matches!(timed_out, Err(McpCallError::Timeout)),
        "{timed_out:?}"
    );
    let cancel = CancellationToken::new();
    let cancelling = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        cancelling.cancel();
    });
    let cancelled = started
        .call("slow", json!({"ms":5000}), Duration::from_secs(30), &cancel)
        .await;
    assert!(
        matches!(cancelled, Err(McpCallError::Cancelled)),
        "{cancelled:?}"
    );
    // The server logs each notice as it reads it; wait for both under load rather than for a fixed time.
    let notices = || {
        log_of(&log)
            .lines()
            .filter(|line| *line == "notifications/cancelled")
            .count()
    };
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while notices() < 2 && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(notices(), 2, "one per abandoned call");
    started.stop().await;
    Ok(())
}

#[tokio::test]
async fn tool_i_012_a_crash_mid_call_is_a_typed_error() -> TestResult {
    let workspace = tempfile::tempdir()?;
    let started = StdioServer::start(&server(&[])?, workspace.path(), HANDSHAKE).await?;
    let crashed = started
        .call(
            "crash",
            json!({}),
            Duration::from_secs(5),
            &CancellationToken::new(),
        )
        .await;
    assert!(matches!(crashed, Err(McpCallError::Crashed)), "{crashed:?}");
    assert!(started.has_exited().await);
    Ok(())
}

#[tokio::test]
async fn tool_i_012_start_failures_are_typed_with_a_bounded_reason() -> TestResult {
    let workspace = tempfile::tempdir()?;
    for (mode, handshake, expected) in [
        (
            "hang_handshake",
            Duration::from_millis(300),
            McpFailure::HandshakeTimeout,
        ),
        ("oversized_line", HANDSHAKE, McpFailure::ProtocolError),
        ("garbage", HANDSHAKE, McpFailure::ProtocolError),
        ("exit_on_start", HANDSHAKE, McpFailure::ProtocolError),
    ] {
        let started = std::time::Instant::now();
        let failure = StdioServer::start(
            &server(&[("FAKE_MCP_MODE", mode)])?,
            workspace.path(),
            handshake,
        )
        .await
        .err()
        .ok_or_else(|| format!("{mode} started"))?;
        assert_eq!(failure, expected, "{mode}");
        assert!(failure.code().len() <= 64, "{mode}");
        assert!(
            started.elapsed() < Duration::from_secs(9),
            "{mode} is bounded"
        );
    }
    let missing: McpServer = serde_json::from_value(json!({
        "name":"fake","transport":{"kind":"stdio","command":"/nonexistent/mcp-server"}
    }))?;
    let failure = StdioServer::start(&missing, workspace.path(), HANDSHAKE)
        .await
        .err()
        .ok_or("a missing command started")?;
    assert_eq!(
        (failure, failure.code()),
        (McpFailure::CommandNotFound, "command_not_found")
    );
    Ok(())
}

#[tokio::test]
async fn tool_i_012_the_server_environment_is_only_its_own_and_carries_no_runtime_variables()
-> TestResult {
    // The fake prints its environment through the echo tool when asked; here we check what it received.
    let workspace = tempfile::tempdir()?;
    let tools = json!([{"name":"env","description":"Environment","inputSchema":{"type":"object"}}])
        .to_string();
    let started = StdioServer::start(
        &server(&[("FAKE_MCP_TOOLS", &tools), ("CUSTOM_SETTING", "on")])?,
        workspace.path(),
        HANDSHAKE,
    )
    .await?;
    let listing = started
        .call(
            "env",
            json!({}),
            Duration::from_secs(5),
            &CancellationToken::new(),
        )
        .await?;
    let names: Vec<&str> = listing.text.lines().collect();
    assert!(names.contains(&"CUSTOM_SETTING"), "{names:?}");
    assert!(
        names.contains(&"PATH") && names.contains(&"HOME"),
        "{names:?}"
    );
    assert!(
        !names.iter().any(|name| name.starts_with("TQA_")),
        "{names:?}"
    );
    assert!(
        !names.iter().any(|name| name.contains("PROXY")),
        "{names:?}"
    );
    started.stop().await;
    Ok(())
}
