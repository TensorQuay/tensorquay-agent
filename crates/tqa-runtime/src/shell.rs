//! The foreground tool joins its process before this decorator inspects escaped children.
use crate::ProcessCleanup;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tqa_tools::{PreparedAction, Tool, ToolContext, ToolError, ToolOutput, ToolSpec};
pub(crate) struct ReapingTool<T> {
    pub tool: T,
    pub cleanup: Arc<dyn ProcessCleanup>,
}
#[async_trait]
impl<T: Tool> Tool for ReapingTool<T> {
    fn spec(&self) -> &ToolSpec {
        self.tool.spec()
    }
    async fn call(&self, args: Value, cx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let result = self.tool.call(args, cx).await;
        self.reaped(result).await
    }
    async fn call_prepared(
        &self,
        action: PreparedAction,
        cx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let result = self.tool.call_prepared(action, cx).await;
        self.reaped(result).await
    }
}
impl<T: Tool> ReapingTool<T> {
    async fn reaped(&self, result: Result<ToolOutput, ToolError>) -> Result<ToolOutput, ToolError> {
        self.cleanup
            .reap()
            .await
            .map_err(|_| ToolError::Io("escaped process cleanup failed".into()))?;
        result
    }
}
