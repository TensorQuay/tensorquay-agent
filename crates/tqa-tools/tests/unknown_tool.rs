//! TOOL-U-159: the unknown-tool answer lists only the tools the run is offered. A run whose agent-team role is `none`
//! keeps the twelve team tool names known so a call to one is refused in words (ADR-0034 A6); they are not offered,
//! so they are not in the list either (E2E-PROFILE-007: a task under a profile that allows `read_file` alone).
#[path = "support/context.rs"]
mod support;
use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio_util::sync::CancellationToken;
use tqa_tools::builtin::team::{ReadAnswer, RoleAnswer, TeamClient, TeamFailure, tools_for_answer};
use tqa_tools::{ToolError, ToolRegistry};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// A gateway that must never be asked: it counts every call and refuses it.
#[derive(Default)]
struct Unasked(AtomicUsize);

impl Unasked {
    fn asked(&self) -> Result<Value, TeamFailure> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(TeamFailure::new("asked", "the gateway was asked"))
    }
}

#[async_trait]
impl TeamClient for Unasked {
    async fn start(
        &self,
        _: &Value,
        _: (&str, &str),
        _: &CancellationToken,
    ) -> Result<Value, TeamFailure> {
        self.asked()
    }
    async fn post(
        &self,
        _: &Value,
        _: (&str, &str),
        _: &CancellationToken,
    ) -> Result<Value, TeamFailure> {
        self.asked()
    }
    async fn take_files(
        &self,
        _: &Value,
        _: (&str, &str),
        _: &CancellationToken,
    ) -> Result<Value, TeamFailure> {
        self.asked()
    }
    async fn done(
        &self,
        _: &Value,
        _: (&str, &str),
        _: &CancellationToken,
    ) -> Result<Value, TeamFailure> {
        self.asked()
    }
    async fn stop_member(
        &self,
        _: &str,
        _: &Value,
        _: (&str, &str),
        _: &CancellationToken,
    ) -> Result<Value, TeamFailure> {
        self.asked()
    }
    async fn transcript(
        &self,
        _: &str,
        _: (Option<u64>, Option<u32>),
        _: &str,
        _: &CancellationToken,
    ) -> Result<Value, TeamFailure> {
        self.asked()
    }
    async fn read(
        &self,
        _: (Option<u32>, Option<u32>),
        _: (&str, &str),
        _: &CancellationToken,
    ) -> Result<ReadAnswer, TeamFailure> {
        self.asked().map(|_| unreachable!("asked() always refuses"))
    }
}

const SWITCHED_OFF: &str = "Helpers are switched off for this installation. Do the work yourself, and say in your \
                            answer that you worked without helpers.";

// TEST: TOOL-U-159
#[tokio::test]
async fn tool_u_159_under_none_the_unknown_tool_answer_lists_only_the_offered_tools() -> TestResult
{
    let fixture = tqa_testkit::workspace_fixture()?;
    let cx = support::context(&fixture);
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(tqa_tools::builtin::ReadFile::new()))?;
    let gateway = Arc::new(Unasked::default());
    for tool in tools_for_answer(
        gateway.clone(),
        &RoleAnswer::parse("none", Some("teams_disabled")),
        None,
    ) {
        registry.register(tool)?;
    }
    let shell = registry.call("shell", json!({"command": "id"}), &cx).await;
    assert!(
        matches!(&shell, Err(ToolError::Unsupported(message)) if message == "unknown tool; known tools: read_file"),
        "{shell:?}"
    );
    // A team tool is still known, and a call to it is refused in words.
    let start = registry
        .get("start_member")
        .ok_or("start_member is still known")?;
    assert!(
        matches!(start.call(json!({}), &cx).await, Err(ToolError::Channel { code, detail }) if code == "teams_unavailable" && detail == SWITCHED_OFF)
    );
    assert_eq!(
        gateway.0.load(Ordering::SeqCst),
        0,
        "nothing reached the gateway"
    );
    Ok(())
}
