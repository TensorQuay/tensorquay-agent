//! `POST /conversations/{id}/site`, `GET /conversations/{id}/site/{pass}/` and `GET /conversations/{id}/site/{pass}/{*path}`: a workspace folder served as a
//! site, for the Files panel's frame (18.0 previews for real sites, the design note of 14 November 2026). The page
//! view (`view.rs`) serves one file; a site's page loads its own CSS, scripts, images and fonts by relative address,
//! and the sandboxed frame has an opaque origin that sends no session cookie, so the credential is a pass in the
//! address: minted by the signed-in person for one folder of one task, read-only, forgotten at a restart.
use super::endpoints::{Tree, blocking, directory_download, tree};
use super::linked;
use super::path::WorkspacePath;
use crate::{GatewayError, app::State, auth::Authz, auth::User, request::ApiJson};
use axum::{
    body::{Body, Bytes},
    extract::{Path as UrlPath, State as AppState},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Json, Response},
};
use futures::StreamExt;
use rand::TryRngCore;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    os::fd::AsFd,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tqa_types::ConversationId;

#[cfg(test)]
#[path = "site_tests.rs"]
mod tests;

/// How long a pass lives from its last use.
pub(crate) const PASS_SECONDS: u64 = 900;
/// Live passes per task at most; the next mint is `409 site_passes_limit`.
pub(crate) const PASSES_PER_TASK: usize = 8;
/// The page a site's root, a directory or an unknown route answers.
const INDEX: &str = "index.html";

/// What a pass serves: one folder of one task's workspace, read as the person who minted it.
#[derive(Clone)]
struct Pass {
    conversation: ConversationId,
    user: User,
    folder: WorkspacePath,
    last_used: Instant,
}

/// The live passes, in memory: a restart forgets them and the frame reloads on its 404.
#[derive(Default)]
pub(crate) struct Passes(Mutex<HashMap<String, Pass>>);

impl Passes {
    /// Mints a pass for `folder` of `conversation`, read as `user`; expired passes go first.
    ///
    /// # Errors
    /// `409 site_passes_limit` at the cap; `Io` when the random source is unavailable.
    fn mint(
        &self,
        now: Instant,
        conversation: &ConversationId,
        user: &User,
        folder: WorkspacePath,
    ) -> Result<String, GatewayError> {
        let mut bytes = [0_u8; 32];
        rand::rngs::OsRng
            .try_fill_bytes(&mut bytes)
            .map_err(|_| GatewayError::Io)?;
        let pass = crate::auth::token::url_safe(&bytes);
        let mut passes = self.0.lock().map_err(|_| GatewayError::Io)?;
        passes.retain(|_, held| {
            now.duration_since(held.last_used) < Duration::from_secs(PASS_SECONDS)
        });
        if passes
            .values()
            .filter(|held| held.conversation == *conversation)
            .count()
            >= PASSES_PER_TASK
        {
            return Err(GatewayError::Conflict("site_passes_limit"));
        }
        passes.insert(
            pass.clone(),
            Pass {
                conversation: conversation.clone(),
                user: user.clone(),
                folder,
                last_used: now,
            },
        );
        Ok(pass)
    }

    /// The pass's folder and person if it is live for `conversation`; a use renews it. An unknown, expired or
    /// mismatched pass answers nothing, and nothing tells which.
    fn take(&self, now: Instant, conversation: &ConversationId, pass: &str) -> Option<Pass> {
        let mut passes = self.0.lock().ok()?;
        let held = passes.get_mut(pass)?;
        if now.duration_since(held.last_used) >= Duration::from_secs(PASS_SECONDS) {
            passes.remove(pass);
            return None;
        }
        if held.conversation != *conversation {
            return None;
        }
        held.last_used = now;
        Some(held.clone())
    }

    /// Live passes of a task (tests).
    #[cfg(test)]
    fn live(&self, now: Instant, conversation: &ConversationId) -> usize {
        self.0.lock().map_or(0, |passes| {
            passes
                .values()
                .filter(|held| {
                    held.conversation == *conversation
                        && now.duration_since(held.last_used) < Duration::from_secs(PASS_SECONDS)
                })
                .count()
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SiteRequest {
    /// A folder relative to the workspace root, or an empty string for the root.
    folder: String,
}

/// What the install's network setting lets a site page reach: the any-host rule, and the exact hosts and `*.domain`
/// entries, so the panel can name what a page loads that the browser will block.
#[derive(Serialize)]
pub(crate) struct SiteNetwork {
    any: bool,
    hosts: Vec<String>,
}

#[derive(Serialize)]
pub(crate) struct SitePass {
    pass: String,
    folder: String,
    url: String,
    expires_in_seconds: u64,
    network: SiteNetwork,
}

/// The site's root under a pass, an absolute path on this gateway.
fn root_path(conversation: &ConversationId, pass: &str) -> String {
    format!(
        "/api/v1/conversations/{}/site/{pass}/",
        conversation.as_str()
    )
}

/// `POST /conversations/{id}/site`: a pass for `folder`, for the owner or a share (the files routes' access). Any
/// folder that exists: a page beside other files (an agent's `report.html`) is opened by its name under the pass;
/// `index.html` is only what the root and a route answer.
///
/// # Errors
/// The files routes' access errors; `400 path_invalid`; `404 not_found` when the folder doesn't exist;
/// `409 site_passes_limit`.
pub(crate) async fn mint(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    UrlPath(id): UrlPath<ConversationId>,
    ApiJson(body): ApiJson<SiteRequest>,
) -> Result<(StatusCode, Json<SitePass>), GatewayError> {
    let folder = WorkspacePath::parse(&body.folder)?;
    exists(&state, &user, &id, &folder).await?;
    let pass = state.site_passes.mint(Instant::now(), &id, &user, folder)?;
    let url = root_path(&id, &pass);
    let (any, hosts) = state.settings.current().egress.site_hosts();
    Ok((
        StatusCode::CREATED,
        Json(SitePass {
            pass,
            folder: body.folder,
            url,
            expires_in_seconds: PASS_SECONDS,
            network: SiteNetwork { any, hosts },
        }),
    ))
}

/// The folder exists in the task's workspace (the root always does), read as the files routes read.
async fn exists(
    state: &Arc<State>,
    user: &User,
    id: &ConversationId,
    folder: &WorkspacePath,
) -> Result<(), GatewayError> {
    match tree(state, user, id, (true, false)).await? {
        Tree::Directory(root) => {
            let folder = folder.clone();
            blocking(move || {
                let root = super::path::open_root(&root)?;
                super::path::open_dir(root.as_fd(), &folder)?;
                Ok(())
            })
            .await
        }
        Tree::Runtime(link) => {
            linked::list(state, &link, folder, None, 1).await?;
            Ok(())
        }
    }
}

/// `GET /conversations/{id}/site/{pass}/{*path}`: a file of the folder, or its `index.html` for the root, a directory
/// and any route the site has no file for. No cookie: the pass is the credential.
///
/// # Errors
/// `404 not_found` for an unknown or expired pass, a path that climbs, and no file and no `index.html`;
/// `413 body_too_large` above the page or download limit; host I/O failures.
pub(crate) async fn file(
    AppState(state): AppState<Arc<State>>,
    UrlPath((id, pass, rest)): UrlPath<(ConversationId, String, String)>,
) -> Result<Response, GatewayError> {
    serve(&state, &id, &pass, &rest).await
}

/// `GET /conversations/{id}/site/{pass}/`: the folder's root, which the wildcard route cannot match (it needs a
/// path); the folder's `index.html` answers, or 404 without one.
pub(crate) async fn root(
    AppState(state): AppState<Arc<State>>,
    UrlPath((id, pass)): UrlPath<(ConversationId, String)>,
) -> Result<Response, GatewayError> {
    serve(&state, &id, &pass, "").await
}

async fn serve(
    state: &Arc<State>,
    id: &ConversationId,
    pass: &str,
    rest: &str,
) -> Result<Response, GatewayError> {
    let held = state
        .site_passes
        .take(Instant::now(), id, pass)
        .ok_or(GatewayError::NotFound)?;
    let folder = folder_text(&held.folder);
    let files = &state.config.files;
    let page_limit = files.preview(files.preview_html_bytes);
    // The first of the address's candidates that is a file answers (GW-U-211's order).
    let mut found = None;
    for target in targets_for(&folder, rest).ok_or(GatewayError::NotFound)? {
        let kind = content_type(&target);
        let limit = if kind.starts_with("text/html") {
            page_limit
        } else {
            files.download_bytes
        };
        match read(state, &held.user, id, target, limit).await {
            Ok(served) => {
                found = Some((kind, served));
                break;
            }
            Err(GatewayError::NotFound | GatewayError::PathInvalid) => {}
            Err(error) => return Err(error),
        }
    }
    let (kind, (size, body)) = found.ok_or(GatewayError::NotFound)?;
    let root = root_path(id, pass);
    let origin = state
        .config
        .public_origin
        .as_str()
        .trim_end_matches('/')
        .to_owned();
    // A page's and a stylesheet's root paths, moved under the pass (GW-U-223); a stylesheet over the page limit and
    // every other file are served as they are.
    let (size, body) = if kind.starts_with("text/html")
        || (kind.starts_with("text/css") && size <= page_limit)
    {
        let bytes = axum::body::to_bytes(body, usize::try_from(page_limit).unwrap_or(usize::MAX))
            .await
            .map_err(|_| GatewayError::BodyTooLarge)?;
        let html = kind.starts_with("text/html");
        let (root, origin) = (root.clone(), origin.clone());
        let rewritten = blocking(move || {
            Ok(if html {
                Bytes::from(super::site_rewrite::html_under_root(
                    &bytes,
                    &root,
                    &super::site_rewrite::reporter(&root, &origin),
                ))
            } else {
                std::str::from_utf8(&bytes)
                    .ok()
                    .and_then(|css| super::site_rewrite::css_under_root(css, &root))
                    .map_or(bytes, Bytes::from)
            })
        })
        .await?;
        (
            u64::try_from(rewritten.len()).unwrap_or(u64::MAX),
            Body::from(rewritten),
        )
    } else {
        (size, body)
    };
    let mut response = body.into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(kind));
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(size));
    if kind.starts_with("text/html") {
        let own = format!("{origin}{root}");
        let outside = state.settings.current().egress.csp_sources();
        headers.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_str(&policy(&own, &outside)).map_err(|_| GatewayError::Io)?,
        );
    }
    common_headers(headers);
    Ok(response)
}

/// The folder as text, for joining.
fn folder_text(folder: &WorkspacePath) -> String {
    folder
        .split_last()
        .map(|(head, last)| {
            let mut parts: Vec<&str> = head.iter().map(String::as_str).collect();
            parts.push(last);
            parts.join("/")
        })
        .unwrap_or_default()
}

fn join(folder: &str, name: &str) -> String {
    if folder.is_empty() {
        name.to_owned()
    } else {
        format!("{folder}/{name}")
    }
}

/// The files an address under `folder` may name, in the order they are tried (GW-U-211, 18.1): the exact file; for
/// `x/`, or `x` without an extension, `x/index.html`; for `x` without an extension, `x.html` (a static export that
/// links `/about` writes `about.html`); then the folder's `index.html` (a single-page app's routes). A path that climbs
/// or holds the private area is refused as missing (`None`).
pub(super) fn targets_for(folder: &str, rest: &str) -> Option<Vec<WorkspacePath>> {
    let rest = rest.trim_start_matches('/');
    // The workspace's private area is never a site's file, whatever folder the site is.
    if rest.split('/').any(|part| part == super::remote::PRIVATE) {
        return None;
    }
    let page = WorkspacePath::parse(&join(folder, INDEX)).ok()?;
    if rest.is_empty() {
        return Some(vec![page]);
    }
    let trimmed = rest.trim_end_matches('/');
    let last = trimmed.rsplit('/').next().unwrap_or("");
    let route = rest.ends_with('/') || !last.contains('.');
    let mut names = Vec::new();
    if route {
        names.push(format!("{trimmed}/{INDEX}"));
        if !rest.ends_with('/') {
            names.push(format!("{trimmed}.html"));
        }
    } else {
        names.push(rest.to_owned());
    }
    let mut targets = Vec::new();
    for name in names {
        // A name that climbs or isn't a workspace path names nothing.
        targets.push(WorkspacePath::parse(&join(folder, &name)).ok()?);
    }
    if !targets.contains(&page) {
        targets.push(page);
    }
    Some(targets)
}

/// The type a file is served with, by its extension and never guessed from its bytes.
pub(super) fn content_type(path: &WorkspacePath) -> &'static str {
    let name = path.split_last().map_or("", |(_, name)| name);
    let extension = name
        .rsplit_once('.')
        .map_or("", |(_, extension)| extension)
        .to_ascii_lowercase();
    match extension.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// The site page's policy: an opaque origin (`sandbox allow-scripts`, no `allow-same-origin`), its own files under
/// the pass for every load, inline scripts and styles, `data:` and `blob:` media, and the install's network setting
/// as `outside` sources; no forms, frames, workers, base or ancestors but this site's pages.
pub(super) fn policy(own: &str, outside: &[String]) -> String {
    let more = outside.join(" ");
    let with = |extra: &str| {
        [own, extra, more.as_str()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    };
    format!(
        "sandbox allow-scripts; default-src 'none'; script-src {}; style-src {}; img-src {}; font-src {}; \
         media-src {}; connect-src {}; form-action 'none'; frame-src 'none'; child-src 'none'; worker-src 'none'; \
         base-uri 'none'; frame-ancestors 'self'",
        with("'unsafe-inline'"),
        with("'unsafe-inline'"),
        with("data: blob:"),
        with("data:"),
        with("data: blob:"),
        with("")
    )
}

/// The headers every served file carries.
fn common_headers(headers: &mut HeaderMap) {
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
    // The page's origin is opaque, so to the browser its own styles, scripts and images are cross-origin loads:
    // `same-origin` (the single-page view's choice) would refuse every one of them. The pass in the path is what
    // guards these files, not the policy; the page's own CSP still names what it may load.
    headers.insert(
        "cross-origin-resource-policy",
        HeaderValue::from_static("cross-origin"),
    );
    // A module script (a built site's entry and chunks) is fetched in CORS mode, and the page's origin is `null`:
    // without an open allow-origin the browser drops the answer and the site never starts. No credentials are
    // involved (the frame sends no cookie); the pass is the file's guard.
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
}

/// A file's size and body, read as the files routes read (the host directory, or the runtime on a microVM install).
async fn read(
    state: &Arc<State>,
    user: &User,
    id: &ConversationId,
    target: WorkspacePath,
    limit: u64,
) -> Result<(u64, Body), GatewayError> {
    match tree(state, user, id, (true, false)).await? {
        Tree::Directory(root) => directory_download(root, target, limit).await,
        Tree::Runtime(link) => {
            let (size, stream) = linked::download(state.clone(), link, &target, limit).await?;
            Ok((
                size,
                Body::from_stream(stream.map(|chunk| chunk.map(Bytes::from))),
            ))
        }
    }
}
