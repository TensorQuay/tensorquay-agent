//! Owner-scoped, idempotent dispatch into the currently registered runtime link.

mod endpoints;
pub(crate) mod held;
mod retention;
mod store;
pub(crate) use retention::start as start_retention;
#[cfg(test)]
mod tests;
pub(crate) use endpoints::{cancel, message};

use crate::GatewayError;
use axum::http::HeaderMap;
use sha2::Digest;
use tqa_types::ClientFrame;

struct Action {
    key: String,
    route: String,
    frame: ClientFrame,
    fingerprint: [u8; 32],
    /// Waits for another task's sandbox on the workspace whatever the installation's busy policy says: an agent team
    /// member's first turn on its lead's files (ADR-0034, the lead's decision of 2 Oct 2026).
    always_wait: bool,
}

/// Reads the one bounded ASCII `Idempotency-Key` every retried write carries (ARCHITECTURE §9).
///
/// # Errors
/// Rejects a missing, repeated, empty, oversized or non-graphic key.
pub(crate) fn idempotency_key(headers: &HeaderMap) -> Result<&str, GatewayError> {
    let mut values = headers.get_all("idempotency-key").iter();
    let key = values
        .next()
        .and_then(|value| value.to_str().ok())
        .ok_or(GatewayError::BadRequest("Idempotency-Key is required"))?;
    if values.next().is_some()
        || !(1..=128).contains(&key.len())
        || !key.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(GatewayError::BadRequest(
            "Idempotency-Key must be one bounded ASCII value",
        ));
    }
    Ok(key)
}

impl Action {
    fn new(headers: &HeaderMap, route: String, frame: ClientFrame) -> Result<Self, GatewayError> {
        Self::keyed(idempotency_key(headers)?, route, frame)
    }

    fn keyed(key: &str, route: String, frame: ClientFrame) -> Result<Self, GatewayError> {
        let bytes =
            serde_json::to_vec(&frame).map_err(|_| GatewayError::BadRequest("invalid action"))?;
        ClientFrame::parse(&bytes).map_err(|error| GatewayError::Protocol(error.code))?;
        // Typed serialization fixes field order and defaults before hashing the request.
        let fingerprint = sha2::Sha256::digest(&bytes).into();
        Ok(Self {
            key: key.into(),
            route,
            frame,
            fingerprint,
            always_wait: false,
        })
    }
}

/// Sends `frame` on `user`'s task `id` under `key`, as the person's own message would go, waiting for the workspace
/// whatever the busy policy: an agent team member's first turn (ADR-0034). A repeat with the same key replays the
/// first answer and sends nothing again.
///
/// # Errors
/// As the message route.
pub(crate) async fn send_turn(
    state: &crate::app::State,
    user: &crate::auth::User,
    id: &tqa_types::ConversationId,
    (key, frame): (&str, ClientFrame),
) -> Result<axum::response::Response, GatewayError> {
    let mut action = Action::keyed(key, format!("/api/v1/conversations/{id}/messages"), frame)?;
    action.always_wait = true;
    store::dispatch(state, user, id, action).await
}
