//! The foreground tool joins its process before this decorator inspects escaped children.
use crate::ProcessCleanup;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tqa_tools::{PreparedAction, Tool, ToolContext, ToolError, ToolImage, ToolOutput, ToolSpec};
pub(crate) struct ReapingTool<T> {
    pub tool: T,
    pub cleanup: Arc<dyn ProcessCleanup>,
}
#[async_trait]
impl<T: Tool> Tool for ReapingTool<T> {
    fn spec(&self) -> &ToolSpec {
        self.tool.spec()
    }
    // Every default the wrapped tool may override is its own, never the trait's: the registry reads
    // `null_means_absent` to normalise the tool's arguments (TOOL-U-034, TOOL-U-146).
    fn null_means_absent(&self) -> bool {
        self.tool.null_means_absent()
    }
    fn offered(&self) -> bool {
        self.tool.offered()
    }
    async fn before(&self, args: &Value, cx: &ToolContext) -> Option<ToolImage> {
        self.tool.before(args, cx).await
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
        self.cleanup.reap().await.map_err(|error| {
            // The model reads the short sentence; the log keeps which step failed, so a failure can be told apart.
            tracing::warn!(error = %error, "escaped process cleanup failed");
            ToolError::Io("escaped process cleanup failed".into())
        })?;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::ReapingTool;
    use crate::{ProcessCleanup, RuntimeError};
    use async_trait::async_trait;
    use serde_json::json;
    use std::sync::Arc;
    use tqa_tools::{Tool, ToolRegistry, builtin};

    struct Nothing;
    #[async_trait]
    impl ProcessCleanup for Nothing {
        async fn reap(&self) -> Result<(), RuntimeError> {
            Ok(())
        }
    }

    // TEST: TOOL-U-146
    /// The runtime registers the shell inside its cleanup wrapper, so the wrapper must keep the shell's own argument
    /// rules (TOOL-U-034's nulls, TOOL-U-146's literals). It didn't: the real stack journaled `"30000"` and refused it
    /// (18 Nov 2026), while the registry's and the runner's tests, which register the shell bare, passed.
    #[test]
    fn tool_u_146_the_wrapped_shell_keeps_its_argument_rules()
    -> Result<(), Box<dyn std::error::Error>> {
        let bare = builtin::Shell::new();
        let shell = ReapingTool {
            tool: builtin::Shell::new(),
            cleanup: Arc::new(Nothing) as Arc<dyn ProcessCleanup>,
        };
        assert_eq!(shell.null_means_absent(), bare.null_means_absent());
        assert_eq!(shell.offered(), bare.offered());
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(shell))?;
        let mut args = json!({"command": "true", "cwd": null, "timeout_ms": "30000"});
        registry.normalize_arguments("shell", &mut args);
        assert_eq!(args, json!({"command": "true", "timeout_ms": 30000}));
        Ok(())
    }
}
