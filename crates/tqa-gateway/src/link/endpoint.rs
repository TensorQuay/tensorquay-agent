use crate::{GatewayError, app, auth::check_origin, manager::Refused};
use axum::{
    extract::{ConnectInfo, State, WebSocketUpgrade},
    http::HeaderMap,
    response::Response,
};
use std::sync::Arc;

pub(crate) async fn upgrade(
    State(state): State<Arc<app::State>>,
    ConnectInfo(peer): ConnectInfo<std::net::SocketAddr>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Result<Response, GatewayError> {
    check_origin(&headers, &state.config.public_origin, false)?;
    let token = crate::auth::token::bearer(&headers)?;
    let (conversation, _) = state
        .runtime_tokens
        .resolve(&token, peer.ip())
        .await?
        .ok_or(GatewayError::Unauthorized)?;
    // A runtime below the protocol floor is refused before any frame, and its sandbox retired (ADR-0026): an old
    // runtime cannot understand the refusal and would otherwise redial for as long as it lives.
    let header = headers
        .get(tqa_types::PROTOCOL_REVISION_HEADER)
        .map(|value| value.to_str())
        .transpose()
        .map_err(|_| GatewayError::BadRequest("invalid protocol revision"))?;
    let has = tqa_types::parse_revision(header)
        .map_err(|_| GatewayError::BadRequest("invalid protocol revision"))?;
    if let Err(too_old) = tqa_types::check_runtime_revision(has) {
        state.manager.refuse(&conversation, Refused::TooOld)?;
        return Err(GatewayError::RuntimeTooOld(too_old));
    }
    // An agent team's journals hold the lead's brief (a member's, revision 12) and the gateway's wakes (the lead's and
    // members', revision 13): an older runtime is refused, never run with either as a person's words or unable to read
    // its own history (ADR-0026's notes of 2 and 5 Oct 2026).
    if has < tqa_types::TEAM_WAKE_REVISION
        && crate::teams::store::ever_in_team(&state, &conversation).await?
    {
        state.manager.refuse(&conversation, Refused::TeamTooOld)?;
        return Err(GatewayError::RuntimeTooOld(tqa_types::RuntimeTooOld {
            needs: tqa_types::TEAM_WAKE_REVISION,
            has,
        }));
    }
    // A desktop environment's runtime must have the computer tool (revision 14, ADR-0039): an older one is refused, never
    // run as a desktop task without it.
    if has < tqa_types::DESKTOP_REVISION && declares_desktop(&state, &conversation).await? {
        state
            .manager
            .refuse(&conversation, Refused::DesktopTooOld)?;
        return Err(GatewayError::RuntimeTooOld(tqa_types::RuntimeTooOld {
            needs: tqa_types::DESKTOP_REVISION,
            has,
        }));
    }
    if let Some(engine) = named_engine(&state, &conversation).await? {
        // A conversation whose profile names a decision engine needs a runtime that can take it (revision 9, also the
        // floor since ADR-0035): an older one is refused the same way, never run without the engine.
        if has < tqa_types::DECISION_ENGINE_REVISION {
            state.manager.refuse(&conversation, Refused::EngineTooOld)?;
            return Err(GatewayError::RuntimeTooOld(tqa_types::RuntimeTooOld {
                needs: tqa_types::DECISION_ENGINE_REVISION,
                has,
            }));
        }
        // An engine removed or disabled since the message was accepted: the sandbox retires without a replacement and
        // its token is revoked, so the runtime dials at most once more; the owner is told, and a message continues the
        // task once the engine is back (ADR-0013 point 6).
        if !super::session::engine_available(&state.settings.current().decision_engines, &engine) {
            state.manager.refuse(&conversation, Refused::EngineGone)?;
            return Err(GatewayError::Conflict("decision_engine_unavailable"));
        }
    }
    let maximum = state.config.limits.ingestion_bytes;
    let tasks = state.tasks.clone();
    Ok(upgrade
        .max_message_size(maximum)
        .max_frame_size(maximum)
        .on_upgrade(move |socket| {
            tasks.track_future(async move {
                super::socket::serve(socket, state, token, (peer.ip(), has)).await;
            })
        }))
}

/// Whether the conversation's environment declares `desktop`; a template removed since is not a desktop one.
async fn declares_desktop(
    state: &app::State,
    conversation: &tqa_types::ConversationId,
) -> Result<bool, GatewayError> {
    let id = crate::conversations::uuid(conversation)?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        sqlx::query_scalar!(
            "SELECT COALESCE('desktop'=ANY(t.capabilities),false) AS \"desktop!\" FROM conversations c LEFT JOIN templates t ON t.name=c.template WHERE c.id=$1",
            id
        )
        .fetch_optional(&state.pool)
        .await
        .map(|desktop| desktop.unwrap_or(false))
        .map_err(GatewayError::from)
    })
    .await
}

/// The decision engine the conversation's profile names, if any.
async fn named_engine(
    state: &app::State,
    conversation: &tqa_types::ConversationId,
) -> Result<Option<String>, GatewayError> {
    let id = crate::conversations::uuid(conversation)?;
    let record = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        sqlx::query!(
            "SELECT owner_id,profile->>'decision_engine' AS engine FROM conversations WHERE id=$1 AND state='active'",
            id
        )
        .fetch_optional(&state.pool)
        .await
        .map_err(GatewayError::from)
    })
    .await?;
    Ok(record.and_then(|record| record.engine))
}
