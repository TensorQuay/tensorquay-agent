//! Which runtime answers for a block-image workspace (ADR-0030 D1). A live sandbox that mounts the workspace answers:
//! the task's own, else the lease holder's. Without one, the task's owner starts it through the ordinary admission (it
//! idles out as usual); anyone else is refused, so a read share never starts another person's sandbox.
use std::time::Duration;

use tqa_sandbox::ReadinessSource as _;
use tqa_types::ConversationId;

use crate::{GatewayError, app::State, auth::User};

/// How often a starting sandbox's link is looked for.
const POLL: Duration = Duration::from_millis(50);

/// The conversation whose runtime answers for `workspace`, the workspace of task `id`. Only with `start` may the
/// caller's own sandbox be started; a caller inside a transaction that holds the task's row must not start it.
///
/// # Errors
/// `409 sandbox_not_ready` when no sandbox is live and none may be started, or it did not become ready in time;
/// `429 busy` while a slot is awaited; store failures.
pub(super) async fn runtime(
    state: &State,
    user: &User,
    id: &ConversationId,
    (workspace, start): (uuid::Uuid, bool),
) -> Result<ConversationId, GatewayError> {
    if state.links.is_ready(id).await {
        return Ok(id.clone());
    }
    let held = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        crate::manager::lease::held(&mut connection, &[workspace]).await
    })
    .await?;
    for lease in held {
        let holder: ConversationId = lease
            .holder
            .to_string()
            .parse()
            .map_err(|_| GatewayError::Io)?;
        if state.links.is_ready(&holder).await {
            return Ok(holder);
        }
    }
    if !start
        || crate::conversations::authorize(state, user, id, false)
            .await
            .is_err()
    {
        return Err(GatewayError::Conflict("sandbox_not_ready"));
    }
    state.manager.start_for_files(id).await?;
    let deadline = tokio::time::Instant::now() + state.config.sandbox.ready();
    while tokio::time::Instant::now() < deadline {
        if state.links.is_ready(id).await {
            return Ok(id.clone());
        }
        tokio::select! {
            () = tokio::time::sleep(POLL) => {}
            () = state.cancel.cancelled() => return Err(GatewayError::Cancelled),
        }
    }
    Err(GatewayError::Conflict("sandbox_not_ready"))
}
