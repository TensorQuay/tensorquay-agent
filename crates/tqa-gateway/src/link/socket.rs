use super::Registration;
use crate::{GatewayError, app::State};
use axum::extract::ws::{CloseFrame, Message, WebSocket};
use std::{sync::Arc, time::Duration};
use tqa_types::{ConversationId, ErrorCode};

use super::liveness::{PING, PongDeadline};

pub(super) async fn serve(
    mut socket: WebSocket,
    state: Arc<State>,
    token: secrecy::SecretString,
    (peer, revision): (std::net::IpAddr, u16),
) {
    let result = match state.runtime_tokens.register(&token, peer, revision).await {
        Ok((conversation, mut registration)) => {
            let result = run(
                &mut socket,
                &state,
                (&conversation, revision),
                &mut registration,
            )
            .await;
            if matches!(result, Err(GatewayError::StoreUnavailable)) {
                // A notice leaves client streams open; the runtime replays its WAL on redial.
                if let Err(error) = state
                    .events
                    .control(&conversation, GatewayError::StoreUnavailable.frame())
                {
                    tracing::warn!(
                        code = error.code(),
                        "store outage notice could not be delivered"
                    );
                }
            }
            if let Err(error) = state
                .links
                .unregister(&conversation, registration.generation)
            {
                tracing::warn!(code = error.code(), "runtime unregistration failed");
            }
            result
        }
        Err(error) => Err(error),
    };
    if let Err(error) = result {
        tracing::info!(code = error.code(), "runtime link closed");
        finish(&mut socket, error, state.config.io_timeout()).await;
    }
}

async fn run(
    socket: &mut WebSocket,
    state: &Arc<State>,
    (conversation, revision): (&ConversationId, u16),
    registration: &mut Registration,
) -> Result<(), GatewayError> {
    let config = match super::session::encode(state, conversation, revision).await {
        // The engine went between the upgrade's check and here: refused like the check refuses it.
        Err(GatewayError::Conflict("decision_engine_unavailable")) => {
            state
                .manager
                .refuse(conversation, crate::manager::Refused::EngineGone)?;
            return Err(GatewayError::Conflict("decision_engine_unavailable"));
        }
        config => config?,
    };
    send(socket, Message::Text(config.into()), state, registration).await?;
    // Notify existing client streams before receiving this registration's durable replay.
    super::session::ready(state, conversation).await?;
    let mut heartbeat = tokio::time::interval_at(tokio::time::Instant::now() + PING, PING);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut pongs = PongDeadline::new(tokio::time::Instant::now());
    let mut inbox = super::inbox::Inbox::new(state, conversation);
    loop {
        tokio::select! {
            biased;
            () = registration.cancel.cancelled() => return Err(GatewayError::Cancelled),
            _ = heartbeat.tick() => {
                if pongs.expired(tokio::time::Instant::now()) { return Err(GatewayError::Timeout); }
                send(socket, Message::Ping(Vec::new().into()), state, registration).await?;
            }
            action = registration.actions.recv() => {
                let action = action.ok_or(GatewayError::Cancelled)?;
                let text = serde_json::to_string(&action.frame).map_err(|_| GatewayError::Protocol(ErrorCode::Internal))?;
                send(socket, Message::Text(text.into()), state, registration).await?;
                let _ = action.sent.send(());
            }
            // One frame at a time is received, in order; the socket is still read meanwhile (`Inbox`).
            result = inbox.received(), if inbox.receiving() => {
                inbound(socket, state, conversation, registration, result).await?;
                if let Some(end) = inbox.advance() { return end; }
            }
            frame = socket.recv(), if inbox.reading() => {
                let end = match frame {
                    Some(Ok(Message::Text(text))) => { inbox.frame(text.as_bytes().to_vec()); None }
                    Some(Ok(Message::Binary(bytes))) => { inbox.frame(bytes.to_vec()); None }
                    Some(Ok(Message::Pong(_))) => { pongs.observed(tokio::time::Instant::now()); None }
                    Some(Ok(Message::Ping(bytes))) => { send(socket, Message::Pong(bytes), state, registration).await?; None }
                    Some(Ok(Message::Close(_))) | None => inbox.end(Ok(())),
                    Some(Err(_)) => inbox.end(Err(GatewayError::Io)),
                };
                if let Some(end) = end { return end; }
            }
        }
    }
}

/// Acts on one received runtime frame's outcome; nothing here waits on the store.
async fn inbound(
    socket: &mut WebSocket,
    state: &Arc<State>,
    conversation: &ConversationId,
    registration: &mut Registration,
    outcome: super::inbox::Outcome,
) -> Result<(), GatewayError> {
    let result = outcome.received;
    if let Err(GatewayError::Protocol(ErrorCode::UnknownFrame)) = result {
        let text = serde_json::to_string(&GatewayError::Protocol(ErrorCode::UnknownFrame).frame())
            .map_err(|_| GatewayError::Io)?;
        send(socket, Message::Text(text.into()), state, registration).await
    } else {
        match result? {
            super::Received::Heartbeat(run_id) => {
                state.links.saw_heartbeat(
                    conversation,
                    registration.generation,
                    run_id,
                    outcome.at,
                )?;
                // The lease renewal ran with the receive (`Inbox`); one the store fails is retried on the next
                // heartbeat while enough of the lease remains.
                registration.renewals.outcome(
                    tokio::time::Instant::now(),
                    state.config.workspaces.lease_ttl,
                    outcome.renewed.unwrap_or(Ok(())),
                )
            }
            super::Received::Transition { sandbox, completed } => {
                // A completed run may be the task's first: it may then get a short title (GW-I-138).
                if completed {
                    crate::relay::title::after_first_run(state, conversation);
                }
                // A lifecycle event is journaled, so the conversation already has history.
                super::session::notice(state, conversation, sandbox, true)
            }
            super::Received::Accepted => Ok(()),
            super::Received::Busy(frame) => {
                if state.manager.busy_refused(conversation) {
                    Ok(())
                } else {
                    state.events.control(conversation, *frame)
                }
            }
            // A screenshot chunk: the store answers the upload's last chunk, or its refusal, on this link.
            super::Received::ScreenUpload {
                id,
                chunk,
                last,
                data,
            } => {
                let answer =
                    crate::screen::uploaded(state, conversation, (&id, chunk, last, &data)).await;
                match answer {
                    Some(frame) => {
                        let text = serde_json::to_string(&frame).map_err(|_| GatewayError::Io)?;
                        send(socket, Message::Text(text.into()), state, registration).await
                    }
                    None => Ok(()),
                }
            }
            // A reply answers the gateway's own request on this link, never clients (ADR-0030).
            super::Received::Workspace(reply) => {
                state
                    .links
                    .workspace
                    .route(conversation, registration.generation, *reply);
                Ok(())
            }
        }
    }
}

async fn send(
    socket: &mut WebSocket,
    message: Message,
    state: &State,
    registration: &Registration,
) -> Result<(), GatewayError> {
    crate::io::bounded(state.config.io_timeout(), &registration.cancel, async {
        socket.send(message).await.map_err(|_| GatewayError::Io)
    })
    .await
}

/// How long a closing link waits for the runtime's close frame after sending its own.
const CLOSE_HANDSHAKE: Duration = Duration::from_secs(1);

async fn finish(socket: &mut WebSocket, error: GatewayError, timeout: Duration) {
    let result = tokio::time::timeout(timeout, async {
        let text = serde_json::to_string(&error.frame()).map_err(|_| GatewayError::Io)?;
        socket
            .send(Message::Text(text.into()))
            .await
            .map_err(|_| GatewayError::Io)?;
        socket
            .send(Message::Close(Some(CloseFrame {
                code: 1008,
                reason: error.code().into(),
            })))
            .await
            .map_err(|_| GatewayError::Io)
    })
    .await;
    if !matches!(result, Ok(Ok(()))) {
        tracing::debug!("runtime close notification could not be delivered");
        return;
    }
    // Complete the close handshake: dropping a socket with unread frames resets the connection, and the runtime
    // could then lose the error frame. Frames that arrive now are discarded; the runtime resends them on redial.
    let drained = tokio::time::timeout(CLOSE_HANDSHAKE, async {
        while let Some(Ok(message)) = socket.recv().await {
            if matches!(message, Message::Close(_)) {
                break;
            }
        }
    })
    .await;
    if drained.is_err() {
        tracing::debug!("runtime did not complete the close handshake");
    }
}
