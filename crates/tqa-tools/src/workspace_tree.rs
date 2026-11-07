//! A workspace as a directory tree, walked one no-follow directory descriptor at a time: validated relative paths,
//! entries, and the manifest the run-changes view compares. Shared by the gateway (Docker workspaces on the host) and
//! the runtime (the workspace service inside a microVM, ADR-0030), so both produce identical results. Model-facing path
//! resolution is [`crate::WorkspaceRoot`]'s job, not this module's.
pub mod manifest;
pub mod path;
pub mod upload;

/// Why a workspace path or walk failed; each kind maps to one HTTP outcome at the gateway.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum TreeError {
    /// A malformed path, a link anywhere in it, or not the expected kind of entry.
    #[error("the workspace path is invalid")]
    PathInvalid,
    /// A missing component, or a private area that is never shown.
    #[error("the workspace path does not exist")]
    NotFound,
    /// Any other host I/O failure.
    #[error("workspace I/O failed")]
    Io,
}

impl TreeError {
    /// The stable code of the failure, as the gateway reports it.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::PathInvalid => "path_invalid",
            Self::NotFound => "not_found",
            Self::Io => "io",
        }
    }
}
