//! Scripted tool responses with injected latency and cancellation.
use async_trait::async_trait;
use serde_json::Value;
use std::{
    collections::VecDeque,
    sync::{Mutex, PoisonError},
    time::Duration,
};
use tqa_tools::{PreparedAction, Tool, ToolContext, ToolError, ToolOutput, ToolSpec};

/// One scripted response, released after an injected-clock delay.
#[derive(Clone, Debug)]
pub struct FakeToolReply {
    /// Latency measured by the call context's sleeper.
    pub delay: Duration,
    /// Success or typed failure returned after the delay.
    pub result: Result<ToolOutput, ToolError>,
}
/// A tool fake with a finite response script and recorded validated arguments.
pub struct FakeTool {
    spec: ToolSpec,
    replies: Mutex<VecDeque<FakeToolReply>>,
    calls: Mutex<Vec<Value>>,
    prepared: Mutex<Vec<Value>>,
    null_means_absent: bool,
}
impl FakeTool {
    /// Creates a fake with an explicit spec and ordered responses.
    #[must_use]
    pub fn new(spec: ToolSpec, replies: Vec<FakeToolReply>) -> Self {
        Self {
            spec,
            replies: Mutex::new(replies.into()),
            calls: Mutex::new(vec![]),
            prepared: Mutex::new(vec![]),
            null_means_absent: false,
        }
    }
    /// Behaves as a strict-valid builtin, whose `null` arguments mean absent (TOOL-U-034); by default the fake is an
    /// external tool, whose arguments are kept exactly as sent.
    #[must_use]
    pub fn with_null_means_absent(mut self) -> Self {
        self.null_means_absent = true;
        self
    }
    /// Returns recorded calls in arrival order.
    #[must_use]
    pub fn calls(&self) -> Vec<Value> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
    /// Returns the immutable identity and arguments received through prepared dispatch.
    #[must_use]
    pub fn prepared_calls(&self) -> Vec<Value> {
        self.prepared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}
#[async_trait]
impl Tool for FakeTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn null_means_absent(&self) -> bool {
        self.null_means_absent
    }
    async fn call_prepared(
        &self,
        action: PreparedAction,
        cx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        self.prepared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(serde_json::json!({
                "tool":action.tool().as_str(), "arguments":action.arguments(),
                "context":action.context(), "action_hash":action.action_hash()
            }));
        let (_, arguments) = action.into_call();
        self.call(arguments, cx).await
    }
    async fn call(&self, args: Value, cx: &ToolContext) -> Result<ToolOutput, ToolError> {
        if cx.cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let reply = self
            .replies
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
            .ok_or_else(|| ToolError::Unsupported("fake tool script exhausted".into()))?;
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(args);
        tokio::select! {
            biased;
            () = cx.cancel.cancelled() => Err(ToolError::Cancelled),
            () = cx.sleeper.sleep(reply.delay) => reply.result,
        }
    }
}
