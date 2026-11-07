//! The tool contract and registry, JSON Schema validation, builtin tools, workspace path
//! policy, shell process management, the MCP client, permission rules and prepared actions.

pub mod builtin;
mod context;
mod error;
mod git_env;
mod image;
mod model_names;
mod net;
mod prepared;
mod process_score;
mod proxy;
mod registry;
mod repository;
mod search_status;
mod tool;
pub mod untrusted;
mod workspace;
pub mod workspace_tree;

pub use builtin::MAX_NOTES_BYTES;
pub use context::{MAX_READ_FILE_BYTES, MIN_PAGE_BYTES, READ_PAGE_BYTES, ToolContext, ToolLimits};
pub use error::ToolError;
pub use git_env::{GIT_SAFETY_ENV, HostKeyEnv};
pub use image::{ImageData, ImageError, probe_image, read_image};
pub use model_names::{MAX_MODEL_TOOL_NAME_BYTES, ModelNames};
pub use prepared::PreparedAction;
pub use process_score::tool_command;
pub use proxy::ProxyEndpoint;
pub use registry::{RegistryError, ToolRegistry};
pub use repository::{CloneError, CloneRequest, GitCloner, RepositoryCloner};
pub use search_status::{SearchStatus, SearchStopReason};
pub use tool::{Artifact, Tool, ToolEvidence, ToolImage, ToolName, ToolOutput, ToolSpec};
pub use workspace::{MAX_PATH_BYTES, MAX_PATH_COMPONENTS, WorkspaceRoot};

/// Returns this crate's package name.
#[must_use]
pub fn crate_name() -> &'static str {
    "tqa-tools"
}

#[cfg(test)]
mod tests {
    #[test]
    fn returns_crate_name() {
        assert_eq!(super::crate_name(), "tqa-tools");
    }
}
