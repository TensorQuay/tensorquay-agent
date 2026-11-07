//! TK-U-013: ext-check's MCP checks against the `tqa-fake-mcp` scenarios, run on this host with the runtime's own
//! client. A good server passes; each defect fails or warns with its code and nothing else; names are the
//! registry's; no listed tool is called without fixtures.
#![cfg(target_os = "linux")]
use std::{error::Error, path::Path, time::Duration};

use serde_json::{Value, json};
use tqa_ext_check::{
    mcp::{CallFixture, Limits, check_stdio},
    report::{Report, Status, Target},
};
use tqa_types::McpServer;

type TestResult = Result<(), Box<dyn Error>>;

const FAKE: &str = env!("CARGO_BIN_EXE_tqa-fake-mcp");

/// The product's bounds, except a short handshake so the hanging server fails fast.
fn limits() -> Limits {
    Limits {
        handshake: Duration::from_secs(10),
        call_timeout: Duration::from_secs(10),
    }
}

fn server(env: &Value) -> Result<McpServer, Box<dyn Error>> {
    Ok(serde_json::from_value(json!({
        "name": "fake", "transport": {"kind": "stdio", "command": FAKE, "args": [], "env": env}
    }))?)
}

async fn check(
    env: &Value,
    calls: &[CallFixture],
    limits: &Limits,
) -> Result<Report, Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let mut report = Report::new(
        Target::stdio_host("fake", FAKE, &[], Vec::new(), true),
        "test",
        "2026-09-27T00:00:00Z",
    );
    check_stdio(&server(env)?, workspace.path(), calls, limits, &mut report).await;
    Ok(report)
}

/// Every check that did not pass, as (status, code).
fn not_passed(report: &Report) -> Vec<(Status, String)> {
    report
        .checks()
        .iter()
        .filter(|check| check.status != Status::Pass)
        .map(|check| (check.status, check.code.clone()))
        .collect()
}

fn codes(report: &Report) -> Vec<String> {
    report
        .checks()
        .iter()
        .map(|check| check.code.clone())
        .collect()
}

fn methods(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[tokio::test]
async fn tk_u_013_a_good_server_passes_and_no_listed_tool_is_called() -> TestResult {
    let dir = tempfile::tempdir()?;
    let log = dir.path().join("methods.log");
    let log_path = log.to_str().ok_or("log path")?;
    let report = check(&json!({"FAKE_MCP_LOG": log_path}), &[], &limits()).await?;
    assert_eq!(not_passed(&report), vec![], "{}", report.text());
    assert_eq!(
        codes(&report),
        vec![
            "handshake",
            "tools_listed",
            "discovery",
            "unknown_tool",
            "exits_on_input_closed"
        ]
    );
    assert_eq!(report.result(), "pass");
    let info = report.server.clone().ok_or("server info")?;
    assert_eq!((info.name.as_str(), info.version.as_str()), ("fake", "1"));
    let tools: Vec<(&str, &str)> = report
        .tools
        .iter()
        .map(|tool| (tool.name.as_str(), tool.model_name.as_str()))
        .collect();
    assert_eq!(tools.len(), 6);
    assert!(tools.contains(&("echo", "mcp__fake__echo")));
    assert!(tools.contains(&("image", "mcp__fake__image")));
    let hash = report.tools_sha256.clone().ok_or("tool hash")?;
    assert!(hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
    // Only the probe for a tool the server did not list is called.
    let calls = methods(&log)
        .iter()
        .filter(|method| *method == "tools/call")
        .count();
    assert_eq!(calls, 1, "{:?}", methods(&log));
    Ok(())
}

#[tokio::test]
async fn tk_u_013_the_same_list_gives_the_same_hash() -> TestResult {
    let first = check(&json!({}), &[], &limits()).await?;
    let second = check(&json!({}), &[], &limits()).await?;
    assert!(first.tools_sha256.is_some());
    assert_eq!(first.tools_sha256, second.tools_sha256);
    let other = check(
        &json!({"FAKE_MCP_TOOLS": r#"[{"name":"one","inputSchema":{"type":"object"}}]"#}),
        &[],
        &limits(),
    )
    .await?;
    assert_ne!(first.tools_sha256, other.tools_sha256);
    Ok(())
}

#[tokio::test]
async fn tk_u_013_a_server_the_runtime_cannot_start_fails_with_the_runtime_code() -> TestResult {
    let many: Vec<Value> = (0..65)
        .map(|index| json!({"name": format!("t{index}"), "inputSchema": {"type": "object"}}))
        .collect();
    let hanging = Limits {
        handshake: Duration::from_millis(300),
        ..limits()
    };
    for (env, limits, code) in [
        (
            json!({"FAKE_MCP_TOOLS": Value::Array(many).to_string()}),
            limits(),
            "too_many_tools",
        ),
        (
            json!({"FAKE_MCP_MODE": "oversized_line"}),
            limits(),
            "protocol_error",
        ),
        (
            json!({"FAKE_MCP_MODE": "garbage"}),
            limits(),
            "protocol_error",
        ),
        (
            json!({"FAKE_MCP_MODE": "exit_on_start"}),
            limits(),
            "protocol_error",
        ),
        (
            json!({"FAKE_MCP_MODE": "hang_handshake"}),
            hanging,
            "handshake_timeout",
        ),
    ] {
        let report = check(&env, &[], &limits).await?;
        assert_eq!(
            not_passed(&report),
            vec![(Status::Fail, code.to_owned())],
            "{env}: {}",
            report.text()
        );
        assert_eq!(report.result(), "fail");
        assert!(report.tools.is_empty() && report.tools_sha256.is_none());
    }
    Ok(())
}

#[tokio::test]
async fn tk_u_013_a_tool_the_registry_refuses_fails_discovery_with_its_code() -> TestResult {
    let big = "x".repeat(65 * 1024);
    for (tools, code) in [
        (
            json!([{"name":"bad","inputSchema":{"type":"object","properties":{"a":{"type":"nonsense"}}}}]),
            "schema_invalid",
        ),
        (
            json!([{"name":"big","inputSchema":{"type":"object","description":big}}]),
            "schema_too_large",
        ),
    ] {
        // A file, since a 65 KiB list is over an environment value's bound.
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("tools.json");
        std::fs::write(&file, tools.to_string())?;
        let report = check(
            &json!({"FAKE_MCP_TOOLS_FILE": file.to_str().ok_or("tools path")?}),
            &[],
            &limits(),
        )
        .await?;
        assert_eq!(
            not_passed(&report),
            vec![(Status::Fail, code.to_owned())],
            "{}",
            report.text()
        );
    }
    Ok(())
}

#[tokio::test]
async fn tk_u_013_folded_names_and_builtin_names_warn() -> TestResult {
    let tools = json!([
        {"name":"Get-Item","inputSchema":{"type":"object"}},
        {"name":"get_item","inputSchema":{"type":"object"}},
        {"name":"shell","inputSchema":{"type":"object"}}
    ]);
    let report = check(
        &json!({"FAKE_MCP_TOOLS": tools.to_string()}),
        &[],
        &limits(),
    )
    .await?;
    assert_eq!(
        not_passed(&report),
        vec![
            (Status::Warn, "name_folded".to_owned()),
            (Status::Warn, "shadows_builtin".to_owned())
        ],
        "{}",
        report.text()
    );
    assert_eq!(report.result(), "pass");
    let mut models: Vec<&str> = report
        .tools
        .iter()
        .map(|tool| tool.model_name.as_str())
        .collect();
    models.sort_unstable();
    // The registry's own folding: distinct names, the second of the pair suffixed, the builtin name namespaced.
    assert_eq!(
        models,
        vec![
            "mcp__fake__get_item",
            "mcp__fake__get_item_2",
            "mcp__fake__shell"
        ]
    );
    Ok(())
}

#[tokio::test]
async fn tk_u_013_behaviour_defects_fail_or_warn_with_their_code() -> TestResult {
    for (mode, expected) in [
        (
            "unknown_ok",
            vec![(Status::Fail, "unknown_tool_not_error".to_owned())],
        ),
        (
            "ignore_eof",
            vec![(Status::Warn, "no_exit_on_eof".to_owned())],
        ),
        // The client declares no sampling capability; the runtime refuses the request and the server carries on.
        ("sampling", vec![]),
    ] {
        let report = check(&json!({"FAKE_MCP_MODE": mode}), &[], &limits()).await?;
        assert_eq!(not_passed(&report), expected, "{mode}: {}", report.text());
    }
    Ok(())
}

#[tokio::test]
async fn tk_u_013_fixture_calls_are_made_and_checked() -> TestResult {
    let calls: Vec<CallFixture> = serde_json::from_value(json!([
        {"tool": "echo", "arguments": {"text": "hello there"}, "expect": "ok", "contains": "hello"},
        {"tool": "fail", "expect": "error"},
        {"tool": "echo", "arguments": {"text": "hi"}, "expect": "error"},
        {"tool": "echo", "arguments": {"text": "hi"}, "expect": "ok", "contains": "absent"},
        {"tool": "echo", "arguments": {"text": 5}, "expect": "ok"},
        {"tool": "missing", "expect": "ok"}
    ]))?;
    let report = check(&json!({}), &calls, &limits()).await?;
    let fixture_codes: Vec<(Status, String)> = report
        .checks()
        .iter()
        .filter(|check| check.code.starts_with("call"))
        .map(|check| (check.status, check.code.clone()))
        .collect();
    assert_eq!(
        fixture_codes,
        vec![
            (Status::Pass, "call".to_owned()),
            (Status::Pass, "call".to_owned()),
            (Status::Fail, "call_unexpected_ok".to_owned()),
            (Status::Fail, "call_text_missing".to_owned()),
            (Status::Fail, "call_arguments_invalid".to_owned()),
            (Status::Fail, "call_tool_not_listed".to_owned()),
        ],
        "{}",
        report.text()
    );
    Ok(())
}
