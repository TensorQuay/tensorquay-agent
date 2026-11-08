//! Workspace files and uploads (FR-36, GW-I-045/047): list, read and download a task's workspace files and
//! upload into `/workspace/uploads/`, through the gateway and never through the model. On Firecracker installs the
//! views are read through the workspace's runtime (ADR-0030); uploads and run changes there answer `files_unavailable`
//! until their slices land.
pub(crate) mod changes;
pub(crate) mod config;
pub(crate) mod handed;
pub(crate) mod path;
pub(crate) mod upload;
pub(crate) use config::FilesConfig;
mod check;
mod endpoints;
mod holder;
mod linked;
mod remote;
mod sheet;
mod sheet_ods;
mod view;
pub(crate) use check::{check_attachments, prepare_attachments};
pub(crate) use endpoints::{content, download, list, upload};
pub(crate) use sheet::sheet;
pub(crate) use view::view;

/// The outer request-body bound for uploads: the per-file limit plus one chunk, so the handler's own check answers
/// `upload_too_large` first.
pub(crate) fn upload_body_limit(config: &FilesConfig) -> usize {
    usize::try_from(config.upload_bytes.saturating_add(1024 * 1024)).unwrap_or(usize::MAX)
}

/// Upload requests get `files.upload_seconds` instead of the general I/O bound (a slow network moving the largest
/// file).
pub(crate) fn is_upload(path: &str) -> bool {
    path.strip_prefix("/api/v1/conversations/")
        .and_then(|rest| rest.split_once('/'))
        .is_some_and(|(_, rest)| rest == "uploads")
}
