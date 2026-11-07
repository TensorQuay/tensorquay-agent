//! The screenshot store (ADR-0039 section 9): the runtime uploads each screenshot in `screen.upload` chunks keyed by
//! the PNG's SHA-256, and the gateway keeps it under `screens/<conversation>/<id>.png` and answers `screen.stored`. A
//! screenshot past 512 KiB, not a PNG, or not matching its id is refused; so is one the state quota can't take, with
//! one notice to the owner per run. The owner reads a stored screenshot by its id until the retention purge removes it.
use crate::{GatewayError, app::State, auth::Authz};
use axum::{
    extract::Path,
    http::{HeaderValue, header},
    response::{IntoResponse, Response},
};
use base64::Engine as _;
use sha2::Digest as _;
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
    time::{Duration, Instant},
};
use tqa_types::{ClientFrame, ConversationId, StoreRefusal};

/// An upload left without a chunk this long is dropped.
const UNFINISHED: Duration = Duration::from_secs(10);
/// A PNG's first eight bytes.
const PNG: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
/// The owner's notice when the store is full: once per run.
const QUOTA_NOTICE: &str =
    "Screenshots aren't being kept: the task's storage is full. The task goes on without them.";

/// One upload being put together.
#[derive(Debug)]
pub(super) struct Assembly {
    id: String,
    next: u32,
    png: Vec<u8>,
    last_chunk: Instant,
    /// Refused already: its remaining chunks are ignored, and answered once.
    refused: bool,
}

/// What a chunk led to.
#[derive(Debug, PartialEq)]
pub(super) enum Step {
    /// More chunks to come, or the rest of a refused upload: nothing to answer.
    More,
    /// The whole PNG, checked: keep it.
    Whole(Vec<u8>),
    /// Refused now: answer it, once.
    Refused(StoreRefusal),
}

/// Adds a chunk to `slot`, the conversation's upload in flight. A new id, a first chunk (an upload of the same id begun
/// again, as two captures of an unchanged screen are, after the runtime gave up on the first), or one left unfinished
/// starts afresh.
pub(super) fn feed(
    slot: &mut Option<Assembly>,
    (id, chunk, last, data): (&str, u32, bool, &str),
    now: Instant,
) -> Step {
    if slot.as_ref().is_some_and(|upload| {
        upload.id != id || chunk == 0 || now.duration_since(upload.last_chunk) > UNFINISHED
    }) {
        *slot = None;
    }
    let upload = slot.get_or_insert_with(|| Assembly {
        id: id.to_owned(),
        next: 0,
        png: Vec::new(),
        last_chunk: now,
        refused: false,
    });
    upload.last_chunk = now;
    if upload.refused {
        if last {
            *slot = None;
        }
        return Step::More;
    }
    let step = add(upload, chunk, data, last);
    match &step {
        Step::Refused(_) if !last => upload.refused = true,
        Step::Refused(_) | Step::Whole(_) => *slot = None,
        Step::More => {}
    }
    step
}

fn add(upload: &mut Assembly, chunk: u32, data: &str, last: bool) -> Step {
    if chunk != upload.next {
        return Step::Refused(StoreRefusal::StoreFailed);
    }
    upload.next += 1;
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) else {
        return Step::Refused(StoreRefusal::BadImage);
    };
    upload.png.extend_from_slice(&bytes);
    if upload.png.len() as u64 > tqa_types::MAX_SCREENSHOT_BYTES {
        return Step::Refused(StoreRefusal::TooLarge);
    }
    if !last {
        return Step::More;
    }
    let digest = crate::hex::hex(&sha2::Sha256::digest(&upload.png));
    if !upload.png.starts_with(PNG) || digest != upload.id {
        return Step::Refused(StoreRefusal::BadImage);
    }
    Step::Whole(std::mem::take(&mut upload.png))
}

/// Every conversation's upload in flight, in this process.
static UPLOADS: LazyLock<Mutex<HashMap<ConversationId, Assembly>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
/// The run each conversation was last told its screenshots aren't being kept: one entry per conversation, dropped
/// once its link is down ([`sweep`]).
static TOLD: LazyLock<Mutex<HashMap<ConversationId, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Drops the uploads left without a chunk for longer than [`UNFINISHED`].
pub(super) fn drop_unfinished(uploads: &mut HashMap<ConversationId, Assembly>, now: Instant) {
    uploads.retain(|_, upload| now.duration_since(upload.last_chunk) <= UNFINISHED);
}

/// Frees what no upload will come back for: uploads left unfinished past [`UNFINISHED`] (a runtime that died
/// mid-upload, or a task that finished), and the quota notice's memory of tasks whose link is down. On every chunk and
/// in each purge pass.
pub(crate) fn sweep(state: &State, now: Instant) {
    if let Ok(mut uploads) = UPLOADS.lock() {
        drop_unfinished(&mut uploads, now);
    }
    if let Ok(mut told) = TOLD.lock() {
        told.retain(|conversation, _| state.links.connected(conversation));
    }
}

/// A `screen.upload` chunk from `conversation`'s runtime: the answer to send back, once the upload is decided.
pub(crate) async fn uploaded(
    state: &State,
    conversation: &ConversationId,
    (id, chunk, last, data): (&str, u32, bool, &str),
) -> Option<ClientFrame> {
    sweep(state, Instant::now());
    let step = {
        let mut uploads = UPLOADS.lock().ok()?;
        let mut slot = uploads.remove(conversation);
        let step = feed(&mut slot, (id, chunk, last, data), Instant::now());
        if let Some(upload) = slot {
            uploads.insert(conversation.clone(), upload);
        }
        step
    };
    let refused = match step {
        Step::More => return None,
        Step::Refused(reason) => Some(reason),
        Step::Whole(png) => keep(state, conversation, id, &png).await.err(),
    };
    Some(ClientFrame::ScreenStored {
        id: id.to_owned(),
        ok: refused.is_none(),
        reason: refused,
    })
}

/// Keeps a whole PNG within the state quota: one quota, `sandbox.state_bytes`, for the task's kept screenshots and its
/// run's state directory together (ADR-0039 section 9).
async fn keep(
    state: &State,
    conversation: &ConversationId,
    id: &str,
    png: &[u8],
) -> Result<(), StoreRefusal> {
    let task = crate::conversations::uuid(conversation).map_err(|_| StoreRefusal::StoreFailed)?;
    let size = i32::try_from(png.len()).map_err(|_| StoreRefusal::TooLarge)?;
    let held = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query!(
            "SELECT COALESCE(SUM(bytes) FILTER (WHERE purged_at IS NULL), 0)::bigint AS \"held!\", \
             bool_or(id = $2 AND purged_at IS NULL) AS kept FROM screenshots WHERE conversation_id = $1",
            task,
            id
        )
        .fetch_one(&state.pool)
        .await?)
    })
    .await
    .map_err(|_| StoreRefusal::StoreFailed)?;
    if held.kept == Some(true) {
        return Ok(());
    }
    let with_this = u64::try_from(held.held)
        .unwrap_or(u64::MAX)
        .saturating_add(png.len() as u64);
    let left = state.config.sandbox.state_bytes.checked_sub(with_this);
    let fits = match left {
        Some(left) => state_within(state, task, left).await?,
        None => false,
    };
    if !fits {
        quota_notice(state, conversation).await;
        return Err(StoreRefusal::QuotaFull);
    }
    state
        .storage
        .keep_screenshot(task, id, png)
        .await
        .map_err(|_| StoreRefusal::StoreFailed)?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        sqlx::query!(
            "INSERT INTO screenshots (conversation_id, id, bytes) VALUES ($1, $2, $3) \
             ON CONFLICT (conversation_id, id) DO UPDATE SET bytes = $3, stored_at = CURRENT_TIMESTAMP, purged_at = NULL",
            task,
            id,
            size
        )
        .execute(&state.pool)
        .await?;
        Ok(())
    })
    .await
    .map_err(|_| StoreRefusal::StoreFailed)
}

/// Whether the run's state directory holds at most `bytes` (none yet holds nothing).
async fn state_within(state: &State, task: uuid::Uuid, bytes: u64) -> Result<bool, StoreRefusal> {
    let path = state.storage.runtime_state_path(task);
    let measured = tokio::task::spawn_blocking(move || {
        match crate::workspaces::charged_within(&path, bytes) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Some(0)),
            other => other,
        }
    })
    .await
    .map_err(|_| StoreRefusal::StoreFailed)?
    .map_err(|_| StoreRefusal::StoreFailed)?;
    Ok(measured.is_some())
}

/// Tells the owner, once per run, that screenshots aren't being kept: never stopping the run.
async fn quota_notice(state: &State, conversation: &ConversationId) {
    let run = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        crate::teams::store::current_run(&mut connection, crate::conversations::uuid(conversation)?)
            .await
    })
    .await
    .ok()
    .flatten()
    .unwrap_or_default();
    let first = TOLD.lock().is_ok_and(|mut told| {
        told.insert(conversation.clone(), run.clone()).as_ref() != Some(&run)
    });
    if first {
        let notice = tqa_types::ServerFrame::Error {
            seq: None,
            code: tqa_types::ErrorCode::StateQuota,
            message: QUOTA_NOTICE.into(),
            retryable: false,
            target: None,
        };
        if let Err(error) = state.events.control(conversation, notice) {
            tracing::debug!(code = error.code(), "the store's quota notice wasn't sent");
        }
    }
}

/// `GET /api/v1/conversations/{id}/screenshots/{screenshot}`: one stored screenshot, the owner only.
///
/// # Errors
/// `403` for another person's task; `404` for a missing task or an id it never stored; `410 screenshot_expired` once
/// the retention purge removed it; a store failure.
pub(crate) async fn read(
    axum::extract::State(state): axum::extract::State<Arc<State>>,
    Authz(user): Authz,
    Path((id, screenshot)): Path<(ConversationId, String)>,
) -> Result<Response, GatewayError> {
    crate::conversations::authorize(&state, &user, &id, false).await?;
    if screenshot.len() != 64
        || !screenshot
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(GatewayError::NotFound);
    }
    let task = crate::conversations::uuid(&id)?;
    let purged = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query_scalar!(
            "SELECT purged_at IS NOT NULL AS \"purged!\" FROM screenshots WHERE conversation_id = $1 AND id = $2",
            task,
            screenshot
        )
        .fetch_optional(&state.pool)
        .await?)
    })
    .await?
    .ok_or(GatewayError::NotFound)?;
    if purged {
        return Err(GatewayError::ScreenshotExpired);
    }
    let path = state.storage.screenshot(task, &screenshot);
    let png = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        tokio::fs::read(&path)
            .await
            .map_err(|_| GatewayError::NotFound)
    })
    .await?;
    let mut response = png.into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/png"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("inline"),
    );
    Ok(response)
}

/// Expired screenshots read per page of the retention purge.
const PURGE_PAGE: i64 = 500;
/// Pages one purge pass reads at most, so a pass stays bounded however many files fail.
const PURGE_PAGES: usize = 20;

/// The screenshot files the purge couldn't remove, logged once each until one is removed.
static UNDELETABLE: LazyLock<Mutex<std::collections::HashSet<(uuid::Uuid, String)>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashSet::new()));

/// The retention purge (`desktop.screenshot_retention_days`): removes the files of screenshots stored longer ago and
/// marks them purged, so their reads answer `410` and their events remain. A file that can't be removed is skipped,
/// logged once, and tried again next pass: the pass reads on past it, page by page, so it never holds the rest back.
///
/// # Errors
/// A store failure.
pub(crate) async fn purge_expired(state: &State) -> Result<usize, GatewayError> {
    sweep(state, Instant::now());
    let days = i32::try_from(
        state
            .settings
            .current()
            .limits
            .desktop_screenshot_retention_days,
    )
    .unwrap_or(i32::MAX);
    let mut purged = 0;
    let mut after: Option<(time::OffsetDateTime, uuid::Uuid, String)> = None;
    for _ in 0..PURGE_PAGES {
        let (stored, conversation, id) = match after.clone() {
            Some((stored, conversation, id)) => (Some(stored), Some(conversation), Some(id)),
            None => (None, None, None),
        };
        let expired = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
            Ok(sqlx::query!(
                "SELECT conversation_id, id, stored_at FROM screenshots WHERE purged_at IS NULL \
                 AND stored_at < CURRENT_TIMESTAMP - make_interval(days => $1) \
                 AND ($2::timestamptz IS NULL OR (stored_at, conversation_id, id) > ($2, $3, $4)) \
                 ORDER BY stored_at, conversation_id, id LIMIT $5",
                days,
                stored,
                conversation,
                id,
                PURGE_PAGE
            )
            .fetch_all(&state.pool)
            .await?)
        })
        .await?;
        let full = i64::try_from(expired.len()).unwrap_or(i64::MAX) == PURGE_PAGE;
        for row in expired {
            after = Some((row.stored_at, row.conversation_id, row.id.clone()));
            if purge_one(state, row.conversation_id, &row.id).await? {
                purged += 1;
            }
        }
        if !full {
            break;
        }
    }
    Ok(purged)
}

/// Removes one expired screenshot's file and marks it purged; `false` when the file couldn't be removed (logged the
/// first time).
async fn purge_one(
    state: &State,
    conversation: uuid::Uuid,
    id: &str,
) -> Result<bool, GatewayError> {
    let key = (conversation, id.to_owned());
    if let Err(error) = state.storage.remove_screenshot(conversation, id).await {
        if UNDELETABLE
            .lock()
            .is_ok_and(|mut failed| failed.insert(key))
        {
            tracing::warn!(
                code = error.code(),
                screenshot = id,
                "an expired screenshot couldn't be removed; kept for the next pass"
            );
        }
        return Ok(false);
    }
    if let Ok(mut failed) = UNDELETABLE.lock() {
        failed.remove(&key);
    }
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        sqlx::query!(
            "UPDATE screenshots SET purged_at = CURRENT_TIMESTAMP WHERE conversation_id = $1 AND id = $2",
            conversation,
            id
        )
        .execute(&state.pool)
        .await?;
        Ok(())
    })
    .await?;
    Ok(true)
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
