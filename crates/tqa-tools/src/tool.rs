//! The small tool boundary shared by builtins and downstream fakes.
use crate::{PreparedAction, ToolContext, ToolError};
use async_trait::async_trait;
use serde_json::Value;

/// A tool name: a builtin matching `[a-z][a-z0-9_]{0,63}`, or an MCP tool `mcp.<server>.<tool>` built only through
/// [`ToolName::mcp`] or read back with [`ToolName::parse`] (PROTO-U-016, TOOL-U-028).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ToolName(String);
// Ordering and equality are the inner string's, so maps keyed by a name can be searched with a `&str`.
impl std::borrow::Borrow<str> for ToolName {
    fn borrow(&self) -> &str {
        &self.0
    }
}
impl ToolName {
    /// Parses a builtin tool name; the MCP namespace stays reserved here.
    ///
    /// # Errors
    /// Returns `InvalidArgs` for invalid length or characters, including any MCP name.
    pub fn new(name: impl Into<String>) -> Result<Self, ToolError> {
        let name = name.into();
        if !tqa_types::valid_builtin_tool_name(&name) {
            return Err(ToolError::invalid("/name", "expected a builtin name"));
        }
        Ok(Self(name))
    }
    /// Builds the name of an MCP server's tool, `mcp.<server>.<tool>`.
    ///
    /// # Errors
    /// Returns `InvalidArgs` for a server outside the MCP name rule or a tool outside 1 to 128 bytes of `[A-Za-z0-9_.-]`.
    pub fn mcp(server: &str, tool: &str) -> Result<Self, ToolError> {
        let name = format!("mcp.{server}.{tool}");
        if tqa_types::split_mcp_tool_name(&name) != Some((server, tool)) {
            return Err(ToolError::invalid(
                "/name",
                "expected an MCP server and tool name",
            ));
        }
        Ok(Self(name))
    }
    /// Reads a stored name of either form, such as a journalled call.
    ///
    /// # Errors
    /// Returns `InvalidArgs` for a name that is neither a builtin nor an MCP tool name.
    pub fn parse(name: impl Into<String>) -> Result<Self, ToolError> {
        let name = name.into();
        if !tqa_types::valid_tool_name(&name) {
            return Err(ToolError::invalid(
                "/name",
                "expected a builtin or MCP tool name",
            ));
        }
        Ok(Self(name))
    }
    /// The server and tool of an MCP tool name; `None` for a builtin.
    #[must_use]
    pub fn mcp_parts(&self) -> Option<(&str, &str)> {
        tqa_types::split_mcp_tool_name(&self.0)
    }
    /// Borrows the validated name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub(crate) fn builtin(name: &'static str) -> Self {
        Self(name.into())
    }
}

/// Immutable tool metadata; the registry compiles its parameter schema at registration.
#[derive(Clone, Debug)]
pub struct ToolSpec {
    /// Validated identifier: a builtin, or an MCP tool registered through the MCP path.
    pub name: ToolName,
    /// Human-readable purpose, without secrets.
    pub description: String,
    /// JSON Schema object for call arguments.
    pub parameters: Value,
    /// Whether separate calls can safely run concurrently: true only for tools that read.
    pub parallel_safe: bool,
    /// Whether a successful call may have changed the workspace's files: true only for `shell`, `write_file`,
    /// `edit_file` and the team lead's `take_files` (which places a helper's files in the workspace). Such a success
    /// between two failures of the same call starts that call's repeated-failure count again (backlog #18); MCP tools
    /// and every other builtin, reads, notes, posts and fetches included, never do.
    pub changes_workspace: bool,
}

/// Reserved artifact representation; this skeleton cannot produce artifacts.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Artifact {}

/// A completed tool response; core adds result handling and spill later.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ToolOutput {
    /// Human-readable response text.
    pub text: String,
    /// Process exit status where applicable.
    pub exit_code: Option<i32>,
    /// Empty until artifact support is introduced.
    pub artifacts: Vec<Artifact>,
    /// Search completeness and continuation, when this response comes from search.
    pub search: Option<crate::SearchStatus>,
    /// The element a `computer` action acted on by ref, for its `tool.result` (ADR-0039); never the model's.
    pub target: Option<tqa_types::ScreenTarget>,
    /// A `computer` screenshot for the model (ADR-0039 slice 3): its bytes are kept in the run's own state, never
    /// journaled, and the model's request carries the latest few.
    pub image: Option<ToolImage>,
    /// Set by a `computer` action that reached the screen (ADR-0039 section 2): it took evidence, and this is the
    /// screen after it. The core stores both evidence captures and says on the `tool.result` which were kept.
    pub evidence: Option<ToolEvidence>,
}

/// What an action that reached the screen took for its evidence after acting; the before capture comes from
/// [`Tool::before`], ahead of the call's start.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolEvidence {
    /// The screen once the action had settled, unmarked; `None` when it couldn't be captured.
    pub after: Option<ToolImage>,
}

/// A screenshot a tool answers with, for the model.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolImage {
    /// The PNG's bytes.
    pub png: Vec<u8>,
    /// Its width in pixels.
    pub width: u32,
    /// Its height in pixels.
    pub height: u32,
    /// Whether it carries the snapshot's marks: only when at least one element was marked.
    pub marked: bool,
    /// The same screen without marks, when it has them: a request sends marks only on its latest screenshot, so a
    /// mark's number always means the current snapshot, and an older one goes as this.
    pub plain: Option<Vec<u8>>,
}
impl ToolOutput {
    /// Creates a text response without process status or artifacts.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            exit_code: None,
            artifacts: vec![],
            search: None,
            target: None,
            image: None,
            evidence: None,
        }
    }
}

/// A callable capability; callers use the registry to validate its arguments first.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Returns stable metadata for this tool.
    fn spec(&self) -> &ToolSpec;
    /// Whether a `null` argument means the same as leaving it out. True only for tools whose schemas we made
    /// strict-valid (every property required, optional ones nullable), so the run records such a call without the
    /// nulls (TOOL-U-034). False by default: an external tool's schema is its own, and a `null` it's sent is meant,
    /// so its arguments are recorded and dispatched exactly as sent.
    fn null_means_absent(&self) -> bool {
        false
    }
    /// Whether the next model call is offered this tool. True by default; an agent team's lead tools answer false until
    /// the run's first successful `start_member`, so the run's tools prefix changes exactly once (ADR-0034, TOOL-U-037).
    fn offered(&self) -> bool {
        true
    }
    /// The screen before this call, for its evidence (ADR-0039 section 2), taken in the core's pre-start step so its
    /// record comes ahead of the call's `tool.start`. `None` by default, and for any call that won't act on the screen:
    /// only a `computer` action its own checks will let through takes one.
    async fn before(&self, _args: &Value, _cx: &ToolContext) -> Option<ToolImage> {
        None
    }
    /// Executes a call under explicit workspace, cancellation and resource limits.
    ///
    /// # Errors
    /// Reports typed argument, path, I/O, deadline and cancellation failures.
    async fn call(&self, args: Value, cx: &ToolContext) -> Result<ToolOutput, ToolError>;
    /// Executes a consumed frozen action, retaining its exact identity.
    /// Ordinary tools use the same argument-only executor; the Git relay binds its push to the identity.
    ///
    /// # Errors
    /// Returns the tool's normal typed execution failures.
    async fn call_prepared(
        &self,
        action: PreparedAction,
        cx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let (_, arguments) = action.into_call();
        self.call(arguments, cx).await
    }
}
