//! HTTP handlers: one directory level, one file's preview, a download, and an upload (GW-I-045 / GW-I-047).
//! Authorization is workspace access (answers-066 Q7, answers-068 Q5): a read share may list and read, uploads need
//! write, and the administrator role grants nothing. Filesystem work runs on the blocking pool. On microVM installs
//! the same views are read through the workspace's runtime (ADR-0030); uploads there still answer `files_unavailable`.
use super::path::{self, Kind, WorkspacePath};
use super::upload::{self, Limits, Stored};
use crate::{
    GatewayError,
    app::State,
    auth::{Authz, User},
    workspaces::{self, Access},
};
use axum::{
    Json,
    body::{Body, Bytes},
    extract::{Path as UrlPath, Query, State as AppState},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::{os::fd::AsFd, path::PathBuf, sync::Arc};
use tqa_types::ConversationId;

/// Entries per page by default and at most.
const DEFAULT_PAGE: usize = 200;
const MAX_PAGE: usize = 500;
/// Download chunk size.
const CHUNK: usize = 64 * 1024;

/// The task's workspace directory when the caller may read it (or write, for uploads).
pub(super) async fn workspace(
    state: &State,
    user: uuid::Uuid,
    id: &ConversationId,
    write: bool,
) -> Result<PathBuf, GatewayError> {
    Ok(workspace_of(state, user, id, write).await?.1)
}

/// The task's workspace id and directory when the caller may read it (or write, for uploads). Directory workspaces
/// only: microVM installs answer `files_unavailable` (uploads and run changes, until their slices land).
pub(super) async fn workspace_of(
    state: &State,
    user: uuid::Uuid,
    id: &ConversationId,
    write: bool,
) -> Result<(uuid::Uuid, PathBuf), GatewayError> {
    if state.config.uses_block_images() {
        return Err(GatewayError::FilesUnavailable);
    }
    let workspace = access(state, user, id, write).await?;
    Ok((workspace, state.storage.workspace(workspace).await?))
}

/// Where a task's workspace is read.
pub(super) enum Tree {
    /// Its directory on the host (Docker).
    Directory(PathBuf),
    /// The runtime of this conversation, whose sandbox mounts the workspace image (microVM installs, ADR-0030).
    Runtime(ConversationId),
}

/// Where the caller may read (or, with `write`, change) the task's workspace; `start` lets a microVM install start
/// the caller's own sandbox.
///
/// # Errors
/// The access errors of [`workspace_of`]; on microVM installs, those of [`super::holder::runtime`].
pub(super) async fn tree(
    state: &State,
    user: &User,
    id: &ConversationId,
    (start, write): (bool, bool),
) -> Result<Tree, GatewayError> {
    let workspace = access(state, user.id, id, write).await?;
    if state.config.uses_block_images() {
        Ok(Tree::Runtime(
            super::holder::runtime(state, user, id, (workspace, start)).await?,
        ))
    } else {
        Ok(Tree::Directory(state.storage.workspace(workspace).await?))
    }
}

/// The task's workspace id when the caller may read it (or write).
pub(super) async fn access(
    state: &State,
    user: uuid::Uuid,
    id: &ConversationId,
    write: bool,
) -> Result<uuid::Uuid, GatewayError> {
    let conversation = crate::conversations::uuid(id)?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let binding = workspaces::of_conversation(&state.pool, conversation).await?;
        let mut connection = state.pool.acquire().await?;
        let access = workspaces::access(&mut connection, user, binding.id).await?;
        if write && access == Access::Read {
            return Err(GatewayError::Forbidden(
                "a write share is required to upload",
            ));
        }
        Ok(binding.id)
    })
    .await
}

pub(super) async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, GatewayError> + Send + 'static,
) -> Result<T, GatewayError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| GatewayError::Io)?
}

fn timestamp(seconds: i64) -> String {
    time::OffsetDateTime::from_unix_timestamp(seconds)
        .ok()
        .and_then(|value| {
            value
                .format(&time::format_description::well_known::Rfc3339)
                .ok()
        })
        .unwrap_or_default()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListQuery {
    #[serde(default)]
    path: String,
    after: Option<String>,
    limit: Option<usize>,
}

#[derive(Serialize)]
pub(crate) struct FileEntry {
    name: String,
    kind: &'static str,
    size: u64,
    modified_at: String,
}

#[derive(Serialize)]
pub(crate) struct FilePage {
    path: String,
    items: Vec<FileEntry>,
    next_after: Option<String>,
}

/// `GET /conversations/{id}/files`: one directory level, directories first, then names; links are listed, never
/// followed; `.tqa/` is left out.
///
/// # Errors
/// `400 path_invalid`, `404`, `403`, `409 files_unavailable`, host I/O failures.
pub(crate) async fn list(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    UrlPath(id): UrlPath<ConversationId>,
    Query(query): Query<ListQuery>,
) -> Result<Json<FilePage>, GatewayError> {
    let limit = query.limit.unwrap_or(DEFAULT_PAGE);
    if !(1..=MAX_PAGE).contains(&limit) {
        return Err(GatewayError::BadRequest("limit must be 1 through 500"));
    }
    let target = WorkspacePath::parse(&query.path)?;
    let (items, next_after) = match tree(&state, &user, &id, (true, false)).await? {
        Tree::Directory(root) => directory_page(root, target.clone(), query.after, limit).await?,
        Tree::Runtime(link) => {
            let (items, next) =
                super::linked::list(&state, &link, &target, query.after, limit).await?;
            let items = items
                .into_iter()
                .map(|item| {
                    entry(
                        item.name,
                        super::linked::kind(item.kind),
                        item.size,
                        item.modified,
                    )
                })
                .collect();
            (items, next)
        }
    };
    Ok(Json(FilePage {
        path: target.to_string(),
        items,
        next_after,
    }))
}

/// One listed entry; only a regular file reports a size.
fn entry(name: String, kind: &'static str, size: u64, modified: i64) -> FileEntry {
    FileEntry {
        name,
        kind,
        size: if kind == Kind::File.as_str() { size } else { 0 },
        modified_at: timestamp(modified),
    }
}

/// One page of a directory on the host: directories first, then names, `.tqa/` left out at the root.
async fn directory_page(
    root: PathBuf,
    target: WorkspacePath,
    after: Option<String>,
    limit: usize,
) -> Result<(Vec<FileEntry>, Option<String>), GatewayError> {
    let root_level = target.is_root();
    let mut entries = blocking(move || {
        let root = path::open_root(&root)?;
        let directory = path::open_dir(root.as_fd(), &target)?;
        path::list(directory.as_fd())
    })
    .await?;
    path::hide_private(&mut entries, root_level);
    entries.sort_by(|a, b| (a.kind != Kind::Dir, &a.name).cmp(&(b.kind != Kind::Dir, &b.name)));
    let start = after.as_deref().map_or(0, |after| {
        entries
            .iter()
            .position(|entry| entry.name == after)
            .map_or(entries.len(), |index| index + 1)
    });
    let page: Vec<FileEntry> = entries
        .iter()
        .skip(start)
        .take(limit)
        .map(|item| {
            entry(
                item.name.clone(),
                item.kind.as_str(),
                item.size,
                item.modified,
            )
        })
        .collect();
    let next_after = (start + page.len() < entries.len())
        .then(|| page.last().map(|entry| entry.name.clone()))
        .flatten();
    Ok((page, next_after))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileQuery {
    pub(super) path: String,
}

#[derive(Serialize)]
pub(crate) struct FileContent {
    path: String,
    size: u64,
    modified_at: String,
    etag: String,
    binary: bool,
    truncated: bool,
    text: Option<String>,
}

/// A weak validator from size, modification time and inode: it changes whenever the file is replaced or written.
/// The runtime reports the same three, so both backends give the same validator.
fn etag(size: u64, modified: i64, inode: u64) -> String {
    format!("\"{size:x}-{modified:x}-{inode:x}\"")
}

/// Text when the previewed bytes are UTF-8 without NUL (a character cut at the preview's end is dropped).
fn preview(bytes: &[u8]) -> Option<String> {
    if bytes.contains(&0) {
        return None;
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => Some(text.to_owned()),
        Err(error) if error.error_len().is_none() => {
            Some(String::from_utf8_lossy(&bytes[..error.valid_up_to()]).into_owned())
        }
        Err(_) => None,
    }
}

/// `GET /conversations/{id}/files/content`: a regular file's metadata and, for text, up to the preview limit.
///
/// # Errors
/// As [`list`], plus `path_invalid` for anything but a regular file.
pub(crate) async fn content(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    UrlPath(id): UrlPath<ConversationId>,
    Query(query): Query<FileQuery>,
    headers: HeaderMap,
) -> Result<Response, GatewayError> {
    let target = WorkspacePath::parse(&query.path)?;
    let limit = state.config.files.preview_bytes;
    let (size, modified, inode, bytes) = match tree(&state, &user, &id, (true, false)).await? {
        Tree::Directory(root) => {
            blocking(move || {
                let root = path::open_root(&root)?;
                let (file, stat) = path::open_file(root.as_fd(), &target)?;
                let mut bytes = Vec::new();
                std::io::Read::read_to_end(
                    &mut std::io::Read::take(std::fs::File::from(file), limit),
                    &mut bytes,
                )
                .map_err(|_| GatewayError::Io)?;
                let size = u64::try_from(stat.st_size).unwrap_or(0);
                Ok((size, stat.st_mtime, stat.st_ino, bytes))
            })
            .await?
        }
        Tree::Runtime(link) => {
            let head = super::linked::head(&state, &link, &target, limit).await?;
            (head.size, head.modified, head.inode, head.bytes)
        }
    };
    let tag = etag(size, modified, inode);
    if headers
        .get(header::IF_NONE_MATCH)
        .is_some_and(|value| value.as_bytes() == tag.as_bytes())
    {
        return Ok(StatusCode::NOT_MODIFIED.into_response());
    }
    let text = preview(&bytes);
    let body = FileContent {
        path: query.path.trim_end_matches('/').to_owned(),
        size,
        modified_at: timestamp(modified),
        etag: tag.clone(),
        binary: text.is_none(),
        truncated: text.is_some() && size > limit,
        text,
    };
    let mut response = Json(body).into_response();
    if let Ok(value) = HeaderValue::from_str(&tag) {
        response.headers_mut().insert(header::ETAG, value);
    }
    Ok(response)
}

/// `GET /conversations/{id}/files/download`: the bytes as an attachment, never rendered by the browser.
///
/// # Errors
/// As [`content`], plus `413 body_too_large` above the download limit.
pub(crate) async fn download(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    UrlPath(id): UrlPath<ConversationId>,
    Query(query): Query<FileQuery>,
) -> Result<Response, GatewayError> {
    let target = WorkspacePath::parse(&query.path)?;
    let name = target
        .split_last()
        .map(|(_, name)| upload::sanitise(name))
        .ok_or(GatewayError::PathInvalid)?;
    let limit = state.config.files.download_bytes;
    let (size, body) = match tree(&state, &user, &id, (true, false)).await? {
        Tree::Directory(root) => directory_download(root, target, limit).await?,
        Tree::Runtime(link) => {
            let (size, stream) =
                super::linked::download(state.clone(), link, &target, limit).await?;
            (
                size,
                Body::from_stream(stream.map(|chunk| chunk.map(Bytes::from))),
            )
        }
    };
    let mut response = body.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(size));
    if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{name}\"")) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    Ok(response)
}

/// A host file's size and a body reading it in chunks; refused over `limit` before anything is sent.
pub(super) async fn directory_download(
    root: PathBuf,
    target: WorkspacePath,
    limit: u64,
) -> Result<(u64, Body), GatewayError> {
    let (file, size) = blocking(move || {
        let root = path::open_root(&root)?;
        let (file, stat) = path::open_file(root.as_fd(), &target)?;
        Ok((
            std::fs::File::from(file),
            u64::try_from(stat.st_size).unwrap_or(0),
        ))
    })
    .await?;
    if size > limit {
        return Err(GatewayError::BodyTooLarge);
    }
    let reader = tokio::fs::File::from_std(file);
    let stream = futures::stream::unfold(Some(reader), |reader| async move {
        let mut reader = reader?;
        let mut buffer = vec![0; CHUNK];
        match tokio::io::AsyncReadExt::read(&mut reader, &mut buffer).await {
            Ok(0) => None,
            Ok(read) => {
                buffer.truncate(read);
                Some((
                    Ok::<Bytes, std::io::Error>(Bytes::from(buffer)),
                    Some(reader),
                ))
            }
            Err(error) => Some((Err(error), None)),
        }
    });
    Ok((size, Body::from_stream(stream)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UploadQuery {
    name: String,
}

#[derive(Serialize)]
pub(crate) struct UploadResponse {
    path: String,
    name: String,
    size: u64,
}

/// `POST /conversations/{id}/uploads?name=`: the request body is the file. Over the per-file limit it is refused as
/// soon as the declared length or the received bytes pass it, before anything is stored.
///
/// # Errors
/// `413 upload_too_large`, `409 uploads_full`, `403` without write access, as [`list`] otherwise.
pub(crate) async fn upload(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    UrlPath(id): UrlPath<ConversationId>,
    Query(query): Query<UploadQuery>,
    request: axum::extract::Request,
) -> Result<(StatusCode, Json<UploadResponse>), GatewayError> {
    let (parts, body) = request.into_parts();
    let headers = parts.headers;
    let limits = Limits {
        per_file: state.config.files.upload_bytes,
        per_workspace: state.config.files.uploads_bytes,
    };
    let declared = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    if declared.is_some_and(|length| length > limits.per_file) {
        return Err(GatewayError::UploadTooLarge);
    }
    if state.config.uses_block_images() {
        let Tree::Runtime(link) = tree(&state, &user, &id, (true, true)).await? else {
            return Err(GatewayError::FilesUnavailable);
        };
        let name = upload::sanitise(&query.name);
        let stored =
            super::linked::upload(&state, &link, &name, body.into_data_stream(), limits).await?;
        return Ok((
            StatusCode::CREATED,
            Json(UploadResponse {
                path: stored.path,
                name: stored.name,
                size: stored.size,
            }),
        ));
    }
    let root = workspace(&state, user.id, &id, true).await?;
    let mut chunks = Vec::new();
    let mut received: u64 = 0;
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|_| GatewayError::BadRequest("the upload body could not be read"))?;
        received = received.saturating_add(chunk.len() as u64);
        if received > limits.per_file {
            return Err(GatewayError::UploadTooLarge);
        }
        chunks.push(chunk.to_vec());
    }
    let name = query.name;
    let Stored { path, size } = blocking(move || {
        let root = path::open_root(&root)?;
        upload::store(root.as_fd(), &name, &chunks, limits)
    })
    .await?;
    let name = path
        .strip_prefix(&format!("{}/", upload::DIRECTORY))
        .unwrap_or(&path)
        .to_owned();
    Ok((
        StatusCode::CREATED,
        Json(UploadResponse { path, name, size }),
    ))
}
