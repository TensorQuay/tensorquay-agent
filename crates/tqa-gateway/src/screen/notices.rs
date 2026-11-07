//! The gateway's own screen notices (ADR-0039's contract): stored one row each, sent once as a `screen.notice` frame
//! on the task's stream, and read back by its owner, so a page that missed the frame still shows it. Never what was
//! typed or pasted.
use crate::{GatewayError, app::State, auth::Authz};
use axum::{Json, extract::Path};
use serde::Serialize;
use std::sync::Arc;
use time::OffsetDateTime;
use tqa_types::{ConversationId, ScreenEnded, ScreenNoticeKind, ScreenNoticeReason, ScreenPerson};

/// The newest notices a read returns.
const MAX_LISTED: i64 = 100;

/// The notices route's answer: oldest first.
#[derive(Debug, Serialize)]
pub(crate) struct Notices {
    items: Vec<Item>,
}

/// One notice as the route and the frame give it.
#[derive(Debug, Serialize)]
pub(super) struct Item {
    pub(super) kind: ScreenNoticeKind,
    pub(super) by: ScreenPerson,
    #[serde(with = "time::serde::rfc3339")]
    pub(super) at: OffsetDateTime,
    pub(super) after_seq: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) ended: Option<ScreenEnded>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) reason: Option<ScreenNoticeReason>,
}

/// `GET /api/v1/conversations/{id}/screen/notices`: the task's newest 100 screen notices, oldest first.
///
/// # Errors
/// `403` for another person's task, `404` for a missing one (as the transcript), or a store failure.
pub(crate) async fn list(
    axum::extract::State(state): axum::extract::State<Arc<State>>,
    Authz(user): Authz,
    Path(id): Path<ConversationId>,
) -> Result<Json<Notices>, GatewayError> {
    crate::conversations::authorize(&state, &user, &id, false).await?;
    let conversation = crate::conversations::uuid(&id)?;
    let rows = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query!(
            "SELECT kind, by_user, by_name, at, after_seq, seconds, ended, reason FROM (SELECT * FROM screen_notices \
             WHERE conversation_id=$1 ORDER BY at DESC, id DESC LIMIT $2) newest ORDER BY at, id",
            conversation,
            MAX_LISTED
        )
        .fetch_all(&state.pool)
        .await?)
    })
    .await?;
    let items = rows
        .into_iter()
        .map(|row| {
            Ok(Item {
                kind: parse_kind(&row.kind)?,
                by: ScreenPerson {
                    id: row.by_user.to_string(),
                    name: row.by_name,
                },
                at: row.at,
                after_seq: u64::try_from(row.after_seq)
                    .map_err(|_| GatewayError::StoreUnavailable)?,
                seconds: row
                    .seconds
                    .map(u64::try_from)
                    .transpose()
                    .map_err(|_| GatewayError::StoreUnavailable)?,
                ended: row.ended.as_deref().map(parse_ended).transpose()?,
                reason: row.reason.as_deref().map(parse_reason).transpose()?,
            })
        })
        .collect::<Result<Vec<_>, GatewayError>>()?;
    Ok(Json(Notices { items }))
}

fn parse_kind(kind: &str) -> Result<ScreenNoticeKind, GatewayError> {
    match kind {
        "screen.taken_over" => Ok(ScreenNoticeKind::TakenOver),
        "screen.handed_back" => Ok(ScreenNoticeKind::HandedBack),
        "screen.signed_out" => Ok(ScreenNoticeKind::SignedOut),
        "screen.sign_out_failed" => Ok(ScreenNoticeKind::SignOutFailed),
        _ => Err(GatewayError::StoreUnavailable),
    }
}

fn parse_reason(reason: &str) -> Result<ScreenNoticeReason, GatewayError> {
    match reason {
        "not_cleared" => Ok(ScreenNoticeReason::NotCleared),
        "unconfirmed" => Ok(ScreenNoticeReason::Unconfirmed),
        _ => Err(GatewayError::StoreUnavailable),
    }
}

fn parse_ended(ended: &str) -> Result<ScreenEnded, GatewayError> {
    match ended {
        "hand_back" => Ok(ScreenEnded::HandBack),
        "socket_closed" => Ok(ScreenEnded::SocketClosed),
        "restart" => Ok(ScreenEnded::Restart),
        _ => Err(GatewayError::StoreUnavailable),
    }
}
