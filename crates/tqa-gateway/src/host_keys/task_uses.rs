//! A task's key uses (ADR-0037 §2), for a page opened after the live `host_key.used` and `host_key.refused` frames: the
//! task's audit rows of keyed calls, by run, newest first, paged as the uses lists are. Each row carries the run, the
//! sender, the outcome, the host and port and a count; the key's name only when the viewer is the row's sender and may
//! use the key, judged when read (least exposure, as the frames: a teammate's placeholder never names its key, and
//! someone removed from a shared key stops seeing its name in old rows too). Never a value or a placeholder. Read by
//! who may read the task's events.
use crate::{GatewayError, app::State, auth::Authz};
use axum::{
    Json,
    extract::{Path, RawQuery},
};
use serde::Serialize;
use std::sync::Arc;
use time::OffsetDateTime;
use tqa_types::ConversationId;
use uuid::Uuid;

/// A page of a task's key uses; `next` is the `before` of the following page, if there is one.
#[derive(Debug, Serialize)]
pub(crate) struct Page {
    items: Vec<Item>,
    next: Option<Uuid>,
}

/// One row: a run's uses of one key at one address with one outcome.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub(super) struct Item {
    id: Uuid,
    run_id: Option<String>,
    sender_id: Uuid,
    /// The key's variable, only for the row's sender.
    name: Option<String>,
    host: String,
    port: u16,
    outcome: String,
    count: i64,
    #[serde(with = "time::serde::rfc3339")]
    first_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    last_at: OffsetDateTime,
}

/// An audit row as read, before it is shaped for a viewer.
pub(super) struct Row {
    pub(super) id: Uuid,
    pub(super) run_id: Option<String>,
    pub(super) user_id: Option<Uuid>,
    pub(super) name: String,
    /// Whether the viewer may use the key now: their own, or a shared key for everyone or for them.
    pub(super) usable: bool,
    pub(super) consumer: String,
    pub(super) outcome: String,
    pub(super) count: i64,
    pub(super) first_at: OffsetDateTime,
    pub(super) last_at: OffsetDateTime,
}

/// `GET /api/v1/conversations/{id}/key-uses?limit=1..=100&before=<id>`: the task's key uses, newest first.
///
/// # Errors
/// As the events answer: `403` for a task the caller may not read the events of, `404` for one that doesn't exist;
/// `400 invalid_request` for the paging; a store failure.
pub(crate) async fn task_uses(
    axum::extract::State(state): axum::extract::State<Arc<State>>,
    Authz(user): Authz,
    Path(id): Path<ConversationId>,
    RawQuery(query): RawQuery,
) -> Result<Json<Page>, GatewayError> {
    crate::conversations::authorize(&state, &user, &id, false).await?;
    let paging = crate::secrets::reads::paging(query.as_deref())?;
    let conversation = crate::conversations::uuid(&id)?;
    let rows = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query_as!(
            Row,
            "SELECT r.id, r.run_id, r.user_id, k.name, (k.owner_id IS NOT DISTINCT FROM $4 OR (k.owner_id IS NULL AND (k.for_everyone \
             OR EXISTS (SELECT 1 FROM host_key_users u WHERE u.key_id=k.id AND u.user_id=$4)))) AS \"usable!\", \
             r.consumer, r.outcome, r.count, r.first_at, r.last_at \
             FROM secret_reads r JOIN host_keys k ON k.secret_ref=r.ref WHERE r.consumer_kind='host' \
             AND r.conversation_id=$1 AND ($2::uuid IS NULL OR (r.last_at, r.id) < \
             (SELECT last_at, id FROM secret_reads WHERE id=$2)) ORDER BY r.last_at DESC, r.id DESC LIMIT $3",
            conversation,
            paging.before,
            paging.limit + 1,
            user.id
        )
        .fetch_all(&state.pool)
        .await?)
    })
    .await?;
    let limit = usize::try_from(paging.limit).unwrap_or(usize::MAX);
    Ok(Json(page(rows, limit, user.id)))
}

/// Up to `limit` of `rows` (read one beyond it) shaped for `viewer`. `next` is the last row read within the limit,
/// even one left out, so the following page never repeats a row.
pub(super) fn page(rows: Vec<Row>, limit: usize, viewer: Uuid) -> Page {
    let more = rows.len() > limit;
    let mut next = None;
    let items = rows
        .into_iter()
        .take(limit)
        .filter_map(|row| {
            next = Some(row.id);
            item(row, viewer)
        })
        .collect();
    Page {
        items,
        next: next.filter(|_| more),
    }
}

/// A row shaped for `viewer`: the key's name only when the viewer is its sender and may use the key; the host and
/// port from the stored `host:port` (a canonical host, an IPv6 address bare). None for a row whose address doesn't
/// read, one without a sender (every keyed call has one) and an `absent` one (an unknown placeholder is no key's use;
/// none is written).
pub(super) fn item(row: Row, viewer: Uuid) -> Option<Item> {
    let (host, port) = row.consumer.rsplit_once(':')?;
    let sender = row.user_id?;
    if row.outcome == "absent" {
        return None;
    }
    Some(Item {
        id: row.id,
        run_id: row.run_id,
        sender_id: sender,
        name: (sender == viewer && row.usable).then_some(row.name),
        host: host.to_owned(),
        port: port.parse().ok()?,
        outcome: row.outcome,
        count: row.count,
        first_at: row.first_at,
        last_at: row.last_at,
    })
}

#[cfg(test)]
#[path = "task_uses_tests.rs"]
mod tests;
