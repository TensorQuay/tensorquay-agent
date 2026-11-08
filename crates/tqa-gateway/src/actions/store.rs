use super::Action;
use crate::{
    GatewayError,
    app::State,
    auth::{User, token},
    conversations,
};
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use tqa_types::ConversationId;

/// A copy for the lead of what the person typed to a helper, once committed: the team, its channel and the copy's seq.
pub(super) type Copied = Option<(uuid::Uuid, uuid::Uuid, u64)>;

pub(super) async fn dispatch(
    state: &State,
    user: &User,
    id: &ConversationId,
    action: Action,
) -> Result<Response, GatewayError> {
    dispatch_copying(state, user, id, (action, None)).await
}

/// [`dispatch`], and with `copy` (the person's own message route only, never a turn the gateway sends) a message to a
/// helper of a running team is copied for its lead in the same transaction, `copy` naming it once committed.
pub(super) async fn dispatch_copying(
    state: &State,
    user: &User,
    id: &ConversationId,
    (action, mut copy): (Action, Option<&mut Copied>),
) -> Result<Response, GatewayError> {
    conversations::authorize(state, user, id, false).await?;
    // Outside the transaction below, which holds the task's row: a sandbox for the check may be started here.
    if let tqa_types::ClientFrame::UserMessage { attachments, .. } = &action.frame {
        // The person comes first: a title call holding the route's only slot gives it up.
        crate::relay::title::interrupt(state, id);
        Box::pin(crate::files::prepare_attachments(
            state,
            user,
            id,
            attachments,
        ))
        .await;
    }
    let conversation = conversations::uuid(id)?;
    let hash = token::digest(&format!("{}:{}", user.id, action.key));
    let lock = i64::from_be_bytes(hash[..8].try_into().map_err(|_| GatewayError::Io)?);
    let retention = f64::from(state.config.limits.idempotency_hours) * 3600.0;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        crate::db::queue(&mut transaction, crate::db::Queue::Key(lock)).await?;
        // Acquire the write lock before global admission, avoiding a shared-lock upgrade cycle.
        let lifecycle = sqlx::query_scalar!("SELECT state FROM conversations WHERE id=$1 FOR UPDATE", conversation).fetch_one(&mut *transaction).await?;
        if lifecycle != "active" { return Err(GatewayError::Conflict("conversation_deleted")); }
        sqlx::query!("DELETE FROM idempotency WHERE key=$1 AND user_id=$2 AND execution_status<>'queued' AND created_at<=CURRENT_TIMESTAMP-make_interval(secs=>$3::double precision)", action.key, user.id, retention).execute(&mut *transaction).await?;
        let cached = sqlx::query!("SELECT route,response_status,response_body,body_hash,execution_status FROM idempotency WHERE key=$1 AND user_id=$2", action.key, user.id)
            .fetch_optional(&mut *transaction).await?;
        if let Some(cached) = cached {
            if cached.route != action.route { return Err(GatewayError::Conflict("idempotency_route")); }
            if cached.body_hash.as_deref() != Some(action.fingerprint.as_slice()) { return Err(GatewayError::IdempotencyKeyReused); }
            let status = u16::try_from(cached.response_status).ok().and_then(|value| StatusCode::from_u16(value).ok()).ok_or(GatewayError::StoreUnavailable)?;
            let body = serde_json::from_slice(&cached.response_body).map_err(|_| GatewayError::StoreUnavailable)?;
            return response(status,body,&cached.execution_status);
        }
        // A new message names only uploaded files of the task's workspace;
        // checked after the replay lookup so a replayed key keeps its first answer.
        if let tqa_types::ClientFrame::UserMessage { attachments, .. } = &action.frame {
            // Boxed: its filesystem walk would otherwise enlarge every action future (clippy::large_futures).
            Box::pin(crate::files::check_attachments(state, user, id, attachments)).await?;
            // A new run needs a ready workspace within its quota: refused, nothing is queued.
            let workspace = sqlx::query!("SELECT w.state, w.size_bytes, w.quota_bytes FROM workspaces w JOIN conversations c ON c.workspace_id=w.id WHERE c.id=$1", conversation)
                .fetch_one(&mut *transaction).await?;
            if workspace.state != "ready" { return Err(GatewayError::Conflict("workspace_not_ready")); }
            if workspace.size_bytes.and_then(|size| u64::try_from(size).ok()).is_some_and(|size| size > crate::workspaces::quota(&state.config, workspace.quota_bytes)) {
                return Err(GatewayError::Conflict("workspace_quota"));
            }
            // A profile's decision engine the install no longer has, or has disabled, is refused, never silently
            // replaced by the model's own decisions (ADR-0013 point 6).
            let engine = sqlx::query_scalar!("SELECT profile->>'decision_engine' FROM conversations WHERE id=$1", conversation)
                .fetch_one(&mut *transaction).await?;
            if let Some(name) = engine
                && !state.settings.current().decision_engines.iter().any(|stored| stored.name == name && stored.enabled)
            {
                return Err(GatewayError::Conflict("decision_engine_unavailable"));
            }
        }
        // A new sandbox's slot comes with its admission and is held until its worker is scheduled (PERF-020).
        let (admission, _slot) = state.manager.admit(&mut transaction,conversation).await?;
        // `refuse`: a task that would wait for another task's running sandbox on its workspace is refused.
        if let crate::manager::Admission::WaitingForWorkspace(block) = admission
            && !block.handoff
            && !action.always_wait
            && state.config.workspaces.busy_policy == crate::config::BusyPolicy::Refuse
        {
            return Err(GatewayError::Conflict("workspace_busy"));
        }
        let pending=sqlx::query_scalar!("SELECT count(*) FROM idempotency WHERE conversation_id=$1 AND execution_status='queued'",conversation).fetch_one(&mut *transaction).await?.unwrap_or(0);
        if pending>=i64::try_from(state.config.limits.action_capacity).map_err(|_|GatewayError::Busy)? {return Err(GatewayError::Busy);}
        let (response, held) = record(&mut transaction, user, id, conversation, &action).await?;
        let copied = copy_for_lead(&mut transaction, state, (conversation, user), (&action, copy.is_some(), held)).await?;
        transaction.commit().await?;
        if let Some(copy) = copy.as_deref_mut() {
            *copy = copied;
        }
        // Again now the message is stored: a title call that looked for waiting messages before it was misses none.
        if matches!(action.frame, tqa_types::ClientFrame::UserMessage { .. }) {
            crate::relay::title::interrupt(state, id);
        }
        if held && let Err(error) = super::held::publish(&state.pool, &state.events, id, None).await {
            tracing::warn!(conversation_id = %id, code = error.code(), "held messages notice not delivered");
        }
        match admission {
            // The message is stored: never a 429 for work the gateway kept (PERF-020 on c16).
            crate::manager::Admission::Admitted => state.manager.schedule_admitted(id)?,
            // Queued behind other work: the admission task starts it and streams its position.
            crate::manager::Admission::Waiting | crate::manager::Admission::WaitingForWorkspace(_) => state.manager.nudge_admission(),
            crate::manager::Admission::Retiring => state.manager.accepted_while_retiring(id)?,
        }
        self::response(StatusCode::ACCEPTED,response,"queued")
    }).await
}

/// Records the accepted action as queued: its response and whether it is a message held behind the run (GW-I-129).
async fn record(
    transaction: &mut sqlx::PgConnection,
    user: &User,
    id: &ConversationId,
    conversation: uuid::Uuid,
    action: &Action,
) -> Result<(serde_json::Value, bool), GatewayError> {
    let mut response = serde_json::json!({"accepted":true, "conversation_id":id});
    let message = matches!(&action.frame, tqa_types::ClientFrame::UserMessage { .. });
    // A message waits while the run is in progress, or behind messages still waiting, and goes out with them as one
    // turn when the run ends (GW-I-129); or while the desktop is taken over, until the hand-back (GW-I-219).
    let held = message && (crate::screen::taken_over(id)
            || crate::events::run_activity(&mut *transaction, conversation).await?.unfinished()
            || sqlx::query_scalar!("SELECT EXISTS(SELECT 1 FROM idempotency WHERE conversation_id=$1 AND route=$2 AND held AND execution_status='queued')", conversation, action.route)
                .fetch_one(&mut *transaction).await?.unwrap_or(false));
    if message {
        sqlx::query!(
            "UPDATE conversations SET last_activity_at=CURRENT_TIMESTAMP WHERE id=$1",
            conversation
        )
        .execute(&mut *transaction)
        .await?;
    }
    // Exact bytes: a message may contain U+0000, which `jsonb` cannot hold.
    let frame = serde_json::to_vec(&action.frame)?;
    let ordinal = sqlx::query_scalar!("INSERT INTO idempotency (key,user_id,route,response_status,response_body,body_hash,conversation_id,queued_frame,execution_status,held) VALUES ($1,$2,$3,202,$4,$5,$6,$7,'queued',$8) RETURNING ordinal AS \"ordinal!\"", action.key, user.id, action.route, serde_json::to_vec(&response)?,action.fingerprint.as_slice(),conversation,frame,held)
            .fetch_one(&mut *transaction).await?;
    if message {
        // Its id, for the queue's list and cancel, and whether it waits; a replayed key answers the same.
        response["message_id"] = serde_json::json!(ordinal.to_string());
        response["held"] = serde_json::json!(held);
        sqlx::query!(
            "UPDATE idempotency SET response_body=$3 WHERE key=$1 AND user_id=$2",
            action.key,
            user.id,
            serde_json::to_vec(&response)?
        )
        .execute(&mut *transaction)
        .await?;
    }
    Ok((response, held))
}

fn response(
    status: StatusCode,
    body: serde_json::Value,
    execution: &str,
) -> Result<Response, GatewayError> {
    let value = match execution {
        "queued" => axum::http::HeaderValue::from_static("queued"),
        "delivered" | "sending" => axum::http::HeaderValue::from_static("delivered"),
        "failed" => axum::http::HeaderValue::from_static("failed"),
        "cancelled" => axum::http::HeaderValue::from_static("cancelled"),
        _ => return Err(GatewayError::StoreUnavailable),
    };
    let mut response = (status, Json(body)).into_response();
    response
        .headers_mut()
        .insert("x-tqa-execution-status", value);
    Ok(response)
}

/// A person's message to a helper reaches its lead as a copy (ADR-0034, the S6 contract), when `wanted`: the person's
/// own message route, never a turn the gateway sends. One to a leading lead that is `held` behind its run is copied
/// too, for the lead's next check of the team (GW-I-307).
async fn copy_for_lead(
    transaction: &mut sqlx::PgConnection,
    state: &State,
    (conversation, user): (uuid::Uuid, &User),
    (action, wanted, held): (&Action, bool, bool),
) -> Result<Copied, GatewayError> {
    match &action.frame {
        tqa_types::ClientFrame::UserMessage { text, .. }
            if wanted && state.config.agent_teams.enabled =>
        {
            let copied = crate::teams::people_write::copy_for_lead(
                &mut *transaction,
                (conversation, user),
                text,
            )
            .await?;
            if copied.is_some() || !held {
                return Ok(copied);
            }
            crate::teams::people_write::copy_to_lead(
                transaction,
                (conversation, user),
                (text, &action.key),
            )
            .await
        }
        _ => Ok(None),
    }
}
