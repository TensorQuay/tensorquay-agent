//! The desktop's screen (ADR-0039 slice 5): the live viewer with Take over, the sandbox's bridges, the gateway's
//! notices, Sign out everywhere, and the screenshot store with its read route and retention purge.
mod bridge;
mod notices;
mod record;
mod relay;
mod rfb;
mod sign_out;
mod store;
mod viewers;

use axum::extract::ws::WebSocket;
use std::sync::LazyLock;
use tqa_types::ConversationId;

pub(crate) use bridge::{HOLD as DIAL_HOLD, dial};
pub(crate) use notices::list;
pub(crate) use relay::view;
pub(crate) use sign_out::{answered, sign_out};
pub(crate) use store::{purge_expired, read, uploaded};

/// Every task's viewers, bridges and Take over in this process (conversation ids are unique, so one process-wide map
/// serves every gateway in it).
static SCREENS: LazyLock<viewers::Screens<WebSocket>> = LazyLock::new(viewers::Screens::default);

/// Whether someone holds `conversation`'s screen: no queued turn starts meanwhile (section 6).
pub(crate) fn taken_over(conversation: &ConversationId) -> bool {
    SCREENS.taken_over(conversation)
}

/// Whether `conversation`'s idle desktop may give way to a waiting one: nobody holds its screen, and its browser's
/// profile is known empty (ADR-0039 section 10, amended 17 Oct 2026).
pub(crate) fn may_retire(conversation: &ConversationId) -> bool {
    SCREENS.may_retire(conversation)
}

/// A run of `conversation` ended: on a desktop the runtime's reset follows, and a Take over waits for it (GW-U-155).
pub(crate) fn run_ended(conversation: &ConversationId) {
    SCREENS.run_ended(conversation);
}

/// The runtime's run-end reset is over (`screen.reset`): a Take over waiting for it is granted control, told when the
/// reset couldn't empty the profile (GW-U-156).
pub(crate) fn reset_done(conversation: &ConversationId, ok: bool, ready: bool) {
    SCREENS.reset_done(conversation, ok, ready);
}

/// A new sandbox was made for `conversation`: its browser profile is empty, so the next Take over warns of nothing.
pub(crate) fn sandbox_created(conversation: &ConversationId) {
    SCREENS.forget(conversation);
}

/// `conversation`'s sandbox was removed: a run-end reset still due from it won't come.
pub(crate) fn sandbox_gone(conversation: &ConversationId) {
    SCREENS.sandbox_gone(conversation);
}

/// `conversation` was deleted: nothing of it is kept.
pub(crate) fn task_deleted(conversation: &ConversationId) {
    SCREENS.forget(conversation);
}

#[cfg(feature = "testing")]
impl crate::Gateway {
    /// Whether `conversation`'s next Take over would say earlier sign-ins may remain in its browser (GW-U-156): the
    /// in-memory mark no frame shows until then. Only with the `testing` feature, never in a release build.
    #[must_use]
    pub fn screen_unclean(conversation: &ConversationId) -> bool {
        SCREENS.is_unclean(conversation)
    }
}

/// Whether the conversation's environment declares `desktop`.
async fn declares_desktop(
    state: &crate::app::State,
    conversation: &ConversationId,
) -> Result<bool, crate::GatewayError> {
    let id = crate::conversations::uuid(conversation)?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query_scalar!(
            "SELECT EXISTS (SELECT 1 FROM conversations c JOIN templates t ON t.name=c.template \
             WHERE c.id=$1 AND 'desktop'=ANY(t.capabilities)) AS \"desktop!\"",
            id
        )
        .fetch_one(&state.pool)
        .await?)
    })
    .await
}

/// At start-up, before the gateway listens: the hand-backs a restart left unrecorded (see
/// [`record::handed_back_at_start`]). A failure is logged and start-up goes on.
pub(crate) async fn hand_back_at_start(state: &crate::app::State) {
    match record::handed_back_at_start(state).await {
        Ok(0) => {}
        Ok(ended) => tracing::info!(ended, "Take overs a restart cut short were handed back"),
        Err(error) => tracing::warn!(
            code = error.code(),
            "Take overs cut short by a restart weren't handed back"
        ),
    }
}
