//! `/api/v1/admin/sandboxes` (the Sandboxes page design note, §2): every running sandbox with its owner, environment,
//! state, size and measured use, the install's limits and totals, and what the sweeps left behind; and
//! `POST …/{conversation_id}/stop`, an administrator's stop of an idle sandbox through the retire path. Nothing of a
//! task but its id is in any answer: no title, message, file, workspace, route or profile.
use super::require_admin;
use crate::{
    GatewayError, InvalidReason,
    app::State as Shared,
    auth::{Authz, User},
    conversations::idempotent::Keyed,
    manager::{Stop, Usage, verdict::Survivor},
};
use axum::{
    Json,
    extract::{Path, RawQuery, State as AppState},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use std::sync::Arc;
use time::{Duration, OffsetDateTime};
use tqa_types::ConversationId;
use uuid::Uuid;

/// Rows per page.
pub(crate) const PAGE: usize = 200;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum State {
    Starting,
    Working,
    Idle,
    Stopping,
}

/// Why an administrator's Stop is refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Refusal {
    Starting,
    Working,
    Stopping,
    WorkWaiting,
}

/// The page's order: the people most affected first.
pub(crate) struct Order;

impl Order {
    pub(crate) const fn of(state: State) -> u8 {
        match state {
            State::Working => 0,
            State::Starting => 1,
            State::Stopping => 2,
            State::Idle => 3,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Owner {
    id: Uuid,
    email: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Environment {
    name: String,
    display_name: String,
    desktop: bool,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) struct Size {
    cpu_millicores: Option<u32>,
    memory_bytes: Option<u64>,
    pids: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Item {
    pub(crate) conversation_id: Uuid,
    owner: Owner,
    environment: Environment,
    pub(crate) state: State,
    #[serde(with = "time::serde::rfc3339")]
    started_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub(crate) last_activity_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub(crate) stops_at: Option<OffsetDateTime>,
    kept_on: bool,
    size: Size,
    usage: Option<Usage>,
    can_stop: bool,
    stop_refusal: Option<Refusal>,
    /// A person holds its screen now (a Take over in progress): the page says so, where a Stop is refused as
    /// `working` (18.2.2, GW-I-399). Never true for a sandbox without a desktop.
    screen_held: bool,
}

impl Item {
    #[cfg(test)]
    pub(crate) fn for_test(
        conversation_id: Uuid,
        state: State,
        last_activity_at: OffsetDateTime,
        stops_at: Option<OffsetDateTime>,
    ) -> Self {
        Self {
            conversation_id,
            owner: Owner {
                id: Uuid::nil(),
                email: "owner@example.invalid".into(),
            },
            environment: Environment {
                name: "minimal".into(),
                display_name: "Minimal".into(),
                desktop: false,
            },
            state,
            started_at: last_activity_at,
            last_activity_at,
            stops_at,
            kept_on: false,
            size: Size {
                cpu_millicores: None,
                memory_bytes: None,
                pids: None,
            },
            usage: None,
            can_stop: refusal(state, false, false).is_none(),
            stop_refusal: refusal(state, false, false),
            screen_held: false,
        }
    }
}

#[derive(Serialize)]
struct Limits {
    slots: u32,
    per_user: u32,
    desktops: u32,
    idle_seconds: u32,
    max_lifetime_seconds: u32,
}

#[derive(Default, Serialize)]
struct Reserved {
    cpu_millicores: u64,
    memory_bytes: u64,
}

#[derive(Serialize)]
struct TotalUsage {
    cpu_millicores: u64,
    memory_bytes: u64,
    #[serde(with = "time::serde::rfc3339")]
    measured_at: OffsetDateTime,
}

#[derive(Default, Serialize)]
struct Totals {
    in_use: u64,
    starting: u64,
    working: u64,
    idle: u64,
    stopping: u64,
    desktops: u64,
    waiting: u64,
    reserved: Reserved,
    usage: Option<TotalUsage>,
}

#[derive(Serialize)]
pub(crate) struct Listing {
    backend: &'static str,
    limits: Limits,
    totals: Totals,
    items: Vec<Item>,
    next_after: Option<String>,
    left_behind: Vec<Survivor>,
}

/// A page of the sorted rows and the token of the next page.
pub(crate) struct Page {
    pub(crate) items: Vec<Item>,
    pub(crate) next_after: Option<String>,
}

/// The state from the record's phase and whether its latest run is going.
pub(crate) fn item_state(phase: &str, run_active: bool) -> State {
    match phase {
        "starting" => State::Starting,
        "destroying" => State::Stopping,
        _ if run_active => State::Working,
        _ => State::Idle,
    }
}

/// When an idle sandbox stops: its window's end from the last activity, or, kept on, its lifetime's end from the
/// start; nothing for a working, starting or stopping one. The row's own timers win over the current limits.
#[allow(clippy::too_many_arguments)]
pub(crate) fn stops_at(
    state: State,
    kept_on: bool,
    started_at: OffsetDateTime,
    last_activity_at: OffsetDateTime,
    row_idle: Option<u32>,
    row_lifetime: Option<u32>,
    idle_seconds: u32,
    max_lifetime_seconds: u32,
) -> Option<OffsetDateTime> {
    if state != State::Idle {
        return None;
    }
    Some(if kept_on {
        started_at + Duration::seconds(i64::from(row_lifetime.unwrap_or(max_lifetime_seconds)))
    } else {
        last_activity_at + Duration::seconds(i64::from(row_idle.unwrap_or(idle_seconds)))
    })
}

/// Why a Stop would be refused now: only an idle sandbox with nothing waiting and no held screen can be stopped. Kept
/// on is information, not a refusal (decision 1 of the review).
pub(crate) fn refusal(state: State, work_waiting: bool, screen_held: bool) -> Option<Refusal> {
    match state {
        State::Starting => Some(Refusal::Starting),
        State::Working => Some(Refusal::Working),
        State::Stopping => Some(Refusal::Stopping),
        State::Idle if work_waiting => Some(Refusal::WorkWaiting),
        State::Idle if screen_held => Some(Refusal::Working),
        State::Idle => None,
    }
}

/// Working, starting, stopping, then idle by soonest stop; among the working the most recently active first.
pub(crate) fn sort(items: &mut [Item]) {
    items.sort_by(|a, b| {
        Order::of(a.state)
            .cmp(&Order::of(b.state))
            .then_with(|| match a.state {
                State::Idle => a.stops_at.cmp(&b.stops_at),
                _ => b.last_activity_at.cmp(&a.last_activity_at),
            })
    });
}

/// The page after `after` (the last row of the previous page), or the first; a token nothing matches starts over.
pub(crate) fn page(items: Vec<Item>, after: Option<&str>) -> Page {
    let start = after
        .and_then(|token| {
            items
                .iter()
                .position(|item| item.conversation_id.to_string() == token)
        })
        .map_or(0, |position| position + 1);
    let rest = items.into_iter().skip(start).collect::<Vec<_>>();
    let more = rest.len() > PAGE;
    let items: Vec<Item> = rest.into_iter().take(PAGE).collect();
    let next_after = more.then(|| items[PAGE - 1].conversation_id.to_string());
    Page { items, next_after }
}

/// Only `after=<id>` is accepted; anything else or repeated is refused.
fn after(query: Option<&str>) -> Result<Option<String>, GatewayError> {
    let mut after = None;
    for (key, value) in url::form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
        if key != "after" || after.is_some() {
            return Err(GatewayError::Invalid {
                field: "query",
                reason: InvalidReason::ParameterInvalid,
            });
        }
        after = Some(value.into_owned());
    }
    Ok(after)
}

/// One running sandbox as the store holds it, with its owner, environment and whether work is going or waiting.
struct Row {
    conversation_id: uuid::Uuid,
    state: State,
    created_at: OffsetDateTime,
    last_action_at: OffsetDateTime,
    idle_seconds: Option<i32>,
    max_lifetime_seconds: Option<i32>,
    cpu_millicores: Option<i32>,
    memory_bytes: Option<i64>,
    pids: Option<i32>,
    owner_id: uuid::Uuid,
    email: String,
    template: String,
    display_name: String,
    desktop: bool,
    /// Pinned, or a Desktop whose sign-in is kept: it stops at its lifetime, not after its idle window.
    kept_on: bool,
    work_waiting: bool,
}

/// The running sandboxes (at most `max_live`) and the tasks waiting for a slot, read in one transaction.
async fn read_rows(state: &Shared, max_live: i64) -> Result<(Vec<Row>, i64), GatewayError> {
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        let rows = sqlx::query!(

            "SELECT s.conversation_id, s.phase, s.created_at, s.last_action_at, s.idle_seconds, s.max_lifetime_seconds, \
             s.cpu_millicores, s.memory_bytes, s.pids, c.owner_id, u.email::text AS \"email!\", c.template, \
             COALESCE(t.display_name, c.template) AS \"display_name!\", \
             COALESCE('desktop'=ANY(t.capabilities), false) AS \"desktop!\", c.pinned, c.keep_sign_in, \
             COALESCE((SELECT e.lifecycle FROM events e WHERE e.conversation_id=s.conversation_id AND e.lifecycle IS NOT NULL \
                       ORDER BY e.seq DESC LIMIT 1) IN ('run.started','run.resumed'), false) AS \"working!\", \
             EXISTS (SELECT 1 FROM idempotency i WHERE i.conversation_id=s.conversation_id \
                     AND i.execution_status IN ('queued','sending')) AS \"work_waiting!\" \
             FROM sandbox_instances s JOIN conversations c ON c.id=s.conversation_id JOIN users u ON u.id=c.owner_id \
             LEFT JOIN templates t ON t.name=c.template ORDER BY s.created_at, s.conversation_id LIMIT $1",
            max_live
        )
        .fetch_all(&mut *transaction)
        .await?
        .into_iter()
        .map(|row| Row {
            conversation_id: row.conversation_id,
            state: item_state(&row.phase, row.working),
            created_at: row.created_at,
            last_action_at: row.last_action_at,
            idle_seconds: row.idle_seconds,
            max_lifetime_seconds: row.max_lifetime_seconds,
            cpu_millicores: row.cpu_millicores,
            memory_bytes: row.memory_bytes,
            pids: row.pids,
            owner_id: row.owner_id,
            email: row.email,
            template: row.template,
            display_name: row.display_name,
            desktop: row.desktop,
            kept_on: row.pinned || (row.keep_sign_in && row.desktop),
            work_waiting: row.work_waiting,
        })
        .collect();
        let waiting = sqlx::query_scalar!(
            "SELECT count(DISTINCT i.conversation_id) AS \"waiting!\" FROM idempotency i WHERE i.execution_status='queued' \
             AND NOT EXISTS (SELECT 1 FROM sandbox_instances s WHERE s.conversation_id=i.conversation_id)"
        )
        .fetch_one(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok((rows, waiting))
    })
    .await
}

/// One row as the page shows it: its state, when it stops, its size and measured use, and whether it may be stopped.
fn item(
    state: &Shared,
    row: Row,
    limits: &crate::settings::limits::Limits,
) -> Result<Item, GatewayError> {
    let id: ConversationId = row
        .conversation_id
        .to_string()
        .parse()
        .map_err(|_| GatewayError::StoreUnavailable)?;
    let item_state = row.state;
    let kept_on = row.kept_on;
    // Only a person holding the screen now (a Take over in progress) refuses a Stop (decision 5). Whether the profile
    // may hold a sign-in (`screen::may_retire`) is the question for giving way unasked, not for a person's Stop: asked
    // here, it left an idle Desktop with a kept sign-in unstoppable (18.1.0 to 18.2.0; GW-I-398).
    let screen_held = row.desktop && crate::screen::taken_over(&id);
    let stop_refusal = refusal(item_state, row.work_waiting, screen_held);
    let size = Size {
        cpu_millicores: row.cpu_millicores.and_then(|n| u32::try_from(n).ok()),
        memory_bytes: row.memory_bytes.and_then(|n| u64::try_from(n).ok()),
        pids: row.pids.and_then(|n| u32::try_from(n).ok()),
    };
    let usage = state.manager.usage_of(&id);
    Ok(Item {
        conversation_id: row.conversation_id,
        owner: Owner {
            id: row.owner_id,
            email: row.email,
        },
        environment: Environment {
            name: row.template,
            display_name: row.display_name,
            desktop: row.desktop,
        },
        state: item_state,
        started_at: row.created_at,
        last_activity_at: row.last_action_at,
        stops_at: stops_at(
            item_state,
            kept_on,
            row.created_at,
            row.last_action_at,
            row.idle_seconds.and_then(|n| u32::try_from(n).ok()),
            row.max_lifetime_seconds.and_then(|n| u32::try_from(n).ok()),
            limits.idle_seconds,
            limits.max_lifetime_seconds,
        ),
        kept_on,
        size,
        usage,
        can_stop: stop_refusal.is_none(),
        stop_refusal,
        screen_held,
    })
}

/// Adds `item` to the totals: its state, a Desktop, and the size it reserves.
fn count(totals: &mut Totals, item: &Item) {
    match item.state {
        State::Starting => totals.starting += 1,
        State::Working => totals.working += 1,
        State::Idle => totals.idle += 1,
        State::Stopping => totals.stopping += 1,
    }
    totals.in_use += 1;
    totals.desktops += u64::from(item.environment.desktop);
    totals.reserved.cpu_millicores += u64::from(item.size.cpu_millicores.unwrap_or(0));
    totals.reserved.memory_bytes += item.size.memory_bytes.unwrap_or(0);
}

/// `GET /api/v1/admin/sandboxes` (administrator, session or token): one consistent read of the records under the
/// gateway's `io_timeout`; the engine is never called, the measured use comes from the sampler's last values.
///
/// # Errors
/// Not an administrator, an unknown query parameter, or a store failure.
pub(crate) async fn list(
    AppState(state): AppState<Arc<Shared>>,
    Authz(user): Authz,
    RawQuery(query): RawQuery,
) -> Result<Json<Listing>, GatewayError> {
    require_admin(&user)?;
    let after = after(query.as_deref())?;
    let limits = state.settings.current().limits;
    let max_live = i64::from(state.config.sandbox.max_live);
    let (rows, waiting) = read_rows(&state, max_live).await?;
    let mut totals = Totals {
        waiting: u64::try_from(waiting).unwrap_or(0),
        ..Totals::default()
    };
    let mut measured = (0u64, 0u64, false);
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let item = item(&state, row, &limits)?;
        count(&mut totals, &item);
        if let Some(usage) = item.usage {
            measured = (
                measured.0 + usage.cpu_millicores,
                measured.1 + usage.memory_bytes,
                true,
            );
        }
        items.push(item);
    }
    if measured.2
        && let Some(measured_at) = state.manager.usage_measured_at()
    {
        totals.usage = Some(TotalUsage {
            cpu_millicores: measured.0,
            memory_bytes: measured.1,
            measured_at,
        });
    }
    sort(&mut items);
    let Page { items, next_after } = page(items, after.as_deref());
    Ok(Json(Listing {
        backend: state.config.backend_name(),
        limits: Limits {
            slots: limits.slots,
            per_user: limits.per_user,
            desktops: limits.desktop_max_sessions,
            idle_seconds: limits.idle_seconds,
            max_lifetime_seconds: limits.max_lifetime_seconds,
        },
        totals,
        items,
        next_after,
        left_behind: state.manager.leak_verdict().left_behind,
    }))
}

/// `POST /api/v1/admin/sandboxes/{conversation_id}/stop` (administrator, session or token; Idempotency-Key required):
/// asks the retire path to stop an idle sandbox, as an eviction does, and audits it; the owner reads a notice in the
/// task, which starts a new sandbox on its files at the next message.
///
/// # Errors
/// `403`; `409 working` (a run is going, the sandbox is starting, or a person holds its screen), `409 work_waiting`
/// (a message is queued for it); `404 not_found` (no running sandbox); `400` without a key; a store failure.
pub(crate) async fn stop(
    AppState(state): AppState<Arc<Shared>>,
    Authz(user): Authz,
    Path(id): Path<ConversationId>,
    headers: HeaderMap,
) -> Result<Response, GatewayError> {
    require_admin(&user)?;
    let keyed = Keyed::new(
        user.id,
        &headers,
        format!("POST /api/v1/admin/sandboxes/{id}/stop"),
        &serde_json::Value::Null,
    )?;
    let body = serde_json::json!({"stopping": true});
    let (status, response, outcome) =
        crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
            let mut transaction = state.pool.begin().await?;
            if let Some((status, cached)) = keyed.begin(&state, &mut transaction).await? {
                return Ok((status, cached, None));
            }
            let outcome = state.manager.stop_by_administrator(&id).await?;
            if outcome == Stop::Requested {
                crate::settings::audit(
                    &mut transaction,
                    &crate::settings::Change {
                        area: "sandboxes",
                        actor: Some(user.id),
                        item: &id.to_string(),
                        action: "stop",
                        before: None,
                        after: None,
                        value_changed: false,
                    },
                )
                .await?;
            }
            keyed
                .record(&mut transaction, StatusCode::ACCEPTED, &body)
                .await?;
            transaction.commit().await?;
            Ok((StatusCode::ACCEPTED, body.clone(), Some(outcome)))
        })
        .await?;
    if outcome == Some(Stop::Requested) {
        tracing::info!(conversation_id = %id, reason = "administrator", "an administrator stopped a sandbox");
        state.telemetry.sandbox_retired("administrator");
        notify_owner(&state, &id, &user).await;
    }
    Ok((status, Json(response)).into_response())
}

/// The owner's notice: stored and sent on the task's stream; a failure is logged, never the stop's failure.
async fn notify_owner(state: &Shared, id: &ConversationId, user: &User) {
    if let Err(error) =
        crate::screen::record(state, id, user, crate::screen::Notice::SandboxStopped).await
    {
        tracing::warn!(conversation_id = %id, code = error.code(), "the stop's notice could not be recorded");
    }
}

#[cfg(test)]
#[path = "sandboxes_tests.rs"]
mod tests;
