//! `GET /conversations/{id}/files/view?path=`: an agent's HTML page, shown in the Files panel's frame (the file
//! previews design). The page is untrusted: its policy gives it an opaque origin, so its scripts never reach the
//! person's session or the API, and blocks every network request, form, popup and top-level navigation. The panel
//! frames it with the same sandbox; the header also holds when the URL is opened on its own.
use super::endpoints::{FileQuery, Tree, directory_download, tree};
use super::linked;
use super::path::WorkspacePath;
use crate::{GatewayError, app::State, auth::Authz};
use axum::{
    body::{Body, Bytes},
    extract::{Path as UrlPath, Query, State as AppState},
    http::{HeaderMap, HeaderValue, header},
    response::{IntoResponse, Response},
};
use futures::StreamExt;
use std::sync::Arc;
use tqa_types::ConversationId;

#[cfg(test)]
#[path = "view_tests.rs"]
mod tests;

/// The page's whole policy. `sandbox allow-scripts` without `allow-same-origin` gives it an opaque origin; the rest
/// allows only inline script, style and `data:` media, so nothing loads from or reaches the network.
const POLICY: &str = "sandbox allow-scripts; default-src 'none'; script-src 'unsafe-inline'; \
                      style-src 'unsafe-inline'; img-src data:; font-src data:; media-src data:; \
                      connect-src 'none'; form-action 'none'; frame-src 'none'; child-src 'none'; \
                      worker-src 'none'; base-uri 'none'; frame-ancestors 'self'";

/// Whether the file is an HTML page, the only type this view serves.
pub(super) fn viewable_path(path: &WorkspacePath) -> bool {
    path.split_last().is_some_and(|(_, name)| {
        name.rsplit_once('.').is_some_and(|(stem, extension)| {
            !stem.is_empty()
                && (extension.eq_ignore_ascii_case("html") || extension.eq_ignore_ascii_case("htm"))
        })
    })
}

/// The sandboxed page's headers.
pub(super) fn sandbox(headers: &mut HeaderMap, size: u64) {
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(size));
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(POLICY),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::X_DNS_PREFETCH_CONTROL,
        HeaderValue::from_static("off"),
    );
    headers.insert(
        "cross-origin-resource-policy",
        HeaderValue::from_static("same-origin"),
    );
}

/// `GET /conversations/{id}/files/view`: an HTML page of the task's workspace, sandboxed.
///
/// # Errors
/// `415 preview_unsupported` for any other file type, `413 body_too_large` over `files.preview_html_bytes`, and as
/// the file download otherwise (access, `path_invalid`, a missing file).
pub(crate) async fn view(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    UrlPath(id): UrlPath<ConversationId>,
    Query(query): Query<FileQuery>,
) -> Result<Response, GatewayError> {
    let target = WorkspacePath::parse(&query.path)?;
    if !viewable_path(&target) {
        return Err(GatewayError::PreviewUnsupported);
    }
    let files = &state.config.files;
    let limit = files.preview(files.preview_html_bytes);
    let (size, body) = match tree(&state, &user, &id, (true, false)).await? {
        Tree::Directory(root) => directory_download(root, target, limit).await?,
        Tree::Runtime(link) => {
            let (size, stream) = linked::download(state.clone(), link, &target, limit).await?;
            (
                size,
                Body::from_stream(stream.map(|chunk| chunk.map(Bytes::from))),
            )
        }
    };
    let mut response = body.into_response();
    sandbox(response.headers_mut(), size);
    Ok(response)
}
