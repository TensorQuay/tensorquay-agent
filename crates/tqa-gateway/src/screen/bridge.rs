//! The sandbox's screen bridge (ADR-0039 section 5): `GET /runtime/screen`, dialled by the runtime of a desktop
//! session with its runtime token. The gateway holds the dial as a long poll: it upgrades it (`101`) for the oldest
//! viewer asking for a bridge, now or within 30 s, and otherwise answers `204` at 30 s, and the runtime dials again. One
//! bridge per viewer, at most four: while four are upgraded, the dial is refused `409` and the runtime waits for one of
//! its bridges to close. So no frame leaves a sandbox that nobody is watching.
use super::SCREENS;
use crate::{GatewayError, app::State};
use axum::{
    extract::{ConnectInfo, ws::WebSocketUpgrade},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use std::{sync::Arc, time::Duration};

/// A bridge granted to a viewer, counted against the four from [`super::viewers::Screen::take_ask`] on. It frees its
/// count on every path that never pairs: the upgrade never completing (the runtime's dial dropped after the `101`, so
/// the upgrade's callback, and this with it, is dropped unrun), or the viewer gone before the socket reached it. Once
/// paired, the relay frees it when the bridge ends.
pub(super) struct Granted<T: 'static> {
    screens: &'static super::viewers::Screens<T>,
    conversation: tqa_types::ConversationId,
    viewer: Option<tokio::sync::oneshot::Sender<T>>,
}

impl<T> Granted<T> {
    pub(super) const fn new(
        screens: &'static super::viewers::Screens<T>,
        conversation: tqa_types::ConversationId,
        viewer: tokio::sync::oneshot::Sender<T>,
    ) -> Self {
        Self {
            screens,
            conversation,
            viewer: Some(viewer),
        }
    }

    /// Gives the bridge's socket to its viewer; a viewer that left in between frees the bridge it would have had.
    pub(super) fn hand_over(mut self, socket: T) {
        if let Some(viewer) = self.viewer.take()
            && viewer.send(socket).is_err()
        {
            self.free();
        }
    }

    fn free(&self) {
        self.screens
            .with(&self.conversation, super::viewers::Screen::bridge_closed);
    }
}

impl<T> Drop for Granted<T> {
    fn drop(&mut self) {
        if self.viewer.take().is_some() {
            self.free();
        }
    }
}

/// How long a dial is held without a viewer asking. The router bounds the dial by it, not by `io_timeout`
/// (`app/long_reads.rs`): under the shorter bound a held dial ended as a 504, which the runtime backs off from.
pub(crate) const HOLD: Duration = Duration::from_secs(30);

/// `GET /runtime/screen` (WebSocket, the runtime token).
///
/// # Errors
/// `401` without a valid runtime token; `403` for a conversation without `desktop`; `409` while four bridges are up.
pub(crate) async fn dial(
    axum::extract::State(state): axum::extract::State<Arc<State>>,
    ConnectInfo(peer): ConnectInfo<std::net::SocketAddr>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Result<Response, GatewayError> {
    let token = crate::auth::token::bearer(&headers)?;
    let (conversation, _) = state
        .runtime_tokens
        .resolve(&token, peer.ip())
        .await?
        .ok_or(GatewayError::Unauthorized)?;
    if !super::declares_desktop(&state, &conversation).await? {
        return Err(GatewayError::Forbidden(
            "the conversation's environment has no desktop",
        ));
    }
    let deadline = tokio::time::Instant::now() + HOLD;
    let asked = SCREENS
        .with(&conversation, super::viewers::Screen::hold)
        .ok_or(GatewayError::StoreUnavailable)?;
    let _held = Held(conversation.clone());
    loop {
        let notified = asked.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        match SCREENS.with(&conversation, super::viewers::Screen::take_ask) {
            Some(Err(_)) => return Err(GatewayError::Conflict("screen_bridges")),
            Some(Ok(Some(viewer))) => {
                let granted = Granted::new(&SCREENS, conversation.clone(), viewer);
                return Ok(upgrade.on_upgrade(move |socket| async move {
                    granted.hand_over(socket);
                }));
            }
            Some(Ok(None)) => {}
            None => return Err(GatewayError::StoreUnavailable),
        }
        if tokio::time::timeout_at(deadline, notified).await.is_err() {
            return Ok(StatusCode::NO_CONTENT.into_response());
        }
    }
}

/// A held dial, released however the dial ends.
struct Held(tqa_types::ConversationId);

impl Drop for Held {
    fn drop(&mut self) {
        SCREENS.with(&self.0, super::viewers::Screen::unhold);
    }
}

#[cfg(test)]
#[path = "bridge_tests.rs"]
mod tests;
