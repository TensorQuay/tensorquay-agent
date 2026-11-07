//! Sign out everywhere (ADR-0039 section 6): the person empties the desktop's browser profile, so no account stays
//! signed in for the agent. Refused while a run is active, so the screen never changes under the agent. With the task's
//! runtime linked, the gateway asks it (`screen.sign_out`) and waits for its answer (`screen.sign_out.done`) to record
//! `screen.signed_out`, or `screen.sign_out_failed` when it couldn't clear the profile or never answered. With no
//! sandbox up there is nothing to clear: the profile lives in the sandbox's own tmpfs, and a new sandbox starts signed
//! out. It reaches the browser only, never a desktop application's own sign-in.
use super::record::{Notice, record};
use crate::{GatewayError, app::State, auth::Authz, auth::User, conversations::idempotent::Keyed};
use axum::{
    extract::Path,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::json;
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};
use tokio::sync::oneshot;
use tqa_types::{ClientFrame, ConversationId, ScreenNoticeReason, SignOutFailure};

/// How long the gateway waits for the runtime's answer before it records the sign-out as unconfirmed. The runtime
/// answers once its browser has started again, which it waits up to 20 s for (its `READY_WAIT`), after up to 2 s for a
/// holder of the profile to exit (GW-U-204).
const ANSWER_SECONDS: u64 = 30;

/// The runtime's answer: emptied, or why not.
type Answer = Result<(), SignOutFailure>;

/// The sign-outs waiting for their runtime's answer, by task.
static WAITING: LazyLock<Waiters> = LazyLock::new(Waiters::default);

/// A task's sign-out in flight: one ask to the runtime at a time. A second request (another tab, a new key) while one
/// waits joins it: no second ask, and the first records the one outcome for both (the page shows it once). The entry
/// stays until the outcome is recorded, so a request that arrives after the answer still joins.
#[derive(Default)]
struct Waiters(Mutex<HashMap<ConversationId, Option<oneshot::Sender<Answer>>>>);

/// What a sign-out does: ask the runtime and wait for this answer, or join the one already waiting.
#[derive(Debug)]
enum Turn {
    Ask(oneshot::Receiver<Answer>),
    Joined,
}

impl Waiters {
    fn join_or_start(&self, id: &ConversationId) -> Turn {
        let Ok(mut waiting) = self.0.lock() else {
            return Turn::Joined;
        };
        if waiting.contains_key(id) {
            return Turn::Joined;
        }
        let (sender, receiver) = oneshot::channel();
        waiting.insert(id.clone(), Some(sender));
        Turn::Ask(receiver)
    }

    /// Hands the runtime's answer to the waiting sign-out, if one is.
    fn answer(&self, id: &ConversationId, answer: Answer) {
        let sender = self
            .0
            .lock()
            .ok()
            .and_then(|mut waiting| waiting.get_mut(id).and_then(Option::take));
        if let Some(sender) = sender {
            let _ = sender.send(answer);
        }
    }

    /// The outcome is recorded: the next sign-out asks again.
    fn finish(&self, id: &ConversationId) {
        if let Ok(mut waiting) = self.0.lock() {
            waiting.remove(id);
        }
    }
}

/// The runtime's `screen.sign_out.done` for `conversation`: handed to the sign-out waiting for it, if one still is.
/// `ready` says whether the browser started again in time for a person's keys (GW-U-204).
pub(crate) fn answered(
    conversation: &ConversationId,
    ok: bool,
    reason: Option<SignOutFailure>,
    ready: bool,
) {
    // An emptied profile leaves no earlier sign-in for the next Take over to warn of; a failure does (GW-U-156).
    super::SCREENS.signed_out(conversation, ok, ready);
    WAITING.answer(
        conversation,
        if ok {
            Ok(())
        } else {
            Err(reason.unwrap_or(SignOutFailure::IoError))
        },
    );
}

/// `POST /api/v1/conversations/{id}/screen/sign-out` `{}`: `202` with no body; the outcome arrives as a screen notice.
///
/// # Errors
/// `403` for another person's task, `404` for a missing one; `409 no_desktop` when its environment has no desktop,
/// `409 run_active` while a run is active; a missing or reused `Idempotency-Key`; a store failure.
pub(crate) async fn sign_out(
    axum::extract::State(state): axum::extract::State<Arc<State>>,
    Authz(user): Authz,
    Path(id): Path<ConversationId>,
    headers: HeaderMap,
) -> Result<Response, GatewayError> {
    crate::conversations::authorize(&state, &user, &id, false).await?;
    let conversation = crate::conversations::uuid(&id)?;
    let keyed = Keyed::new(
        user.id,
        &headers,
        format!("POST /api/v1/conversations/{id}/screen/sign-out"),
        &json!({}),
    )?;
    let first = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        if keyed.begin(&state, &mut transaction).await?.is_some() {
            return Ok(false);
        }
        let row = sqlx::query!(
            "SELECT EXISTS (SELECT 1 FROM conversations c JOIN templates t ON t.name=c.template \
             WHERE c.id=$1 AND 'desktop'=ANY(t.capabilities)) AS \"desktop!\", \
             (SELECT e.lifecycle FROM events e WHERE e.conversation_id=$1 AND e.lifecycle IS NOT NULL \
              ORDER BY e.seq DESC LIMIT 1) AS latest",
            conversation
        )
        .fetch_one(&mut *transaction)
        .await?;
        if !row.desktop {
            return Err(GatewayError::Conflict("no_desktop"));
        }
        if crate::channels::store::RunStatus::of(row.latest.as_deref())
            == crate::channels::store::RunStatus::Running
        {
            return Err(GatewayError::Conflict("run_active"));
        }
        keyed
            .record(&mut transaction, StatusCode::ACCEPTED, &json!({}))
            .await?;
        transaction.commit().await?;
        Ok(true)
    })
    .await?;
    if first {
        let state = state.clone();
        state
            .tasks
            .clone()
            .spawn(async move { finish(&state, &id, &user).await });
    }
    Ok(StatusCode::ACCEPTED.into_response())
}

/// Asks the linked runtime, waits for its answer within the bound, and records the outcome.
async fn finish(state: &State, id: &ConversationId, user: &User) {
    if !state.links.connected(id) {
        // No sandbox up: nothing to clear.
        if let Err(error) = record(state, id, user, Notice::SignedOut).await {
            tracing::warn!(code = error.code(), "a sign-out's notice wasn't recorded");
        }
        return;
    }
    let Turn::Ask(receiver) = WAITING.join_or_start(id) else {
        // Joined the sign-out already waiting: it records the one outcome.
        return;
    };
    // Its browser closes and starts again: a Take over meanwhile waits for the answer (GW-U-204).
    super::SCREENS.sign_out_asked(id);
    let notice = match ask(state, id, receiver).await {
        Some(Ok(())) => Notice::SignedOut,
        Some(Err(_)) => Notice::SignOutFailed(ScreenNoticeReason::NotCleared),
        None => {
            // Unanswered: nothing is known of the profile or the browser, so nothing waits on it any longer, and a Take
            // over says a sign-in may not last until a reset says otherwise.
            super::SCREENS.signed_out(id, false, false);
            Notice::SignOutFailed(ScreenNoticeReason::Unconfirmed)
        }
    };
    if let Err(error) = record(state, id, user, notice).await {
        tracing::warn!(code = error.code(), "a sign-out's notice wasn't recorded");
    }
    WAITING.finish(id);
}

/// The runtime's answer, or `None` when it couldn't be asked, didn't answer in time, or its link dropped.
async fn ask(
    state: &State,
    id: &ConversationId,
    receiver: oneshot::Receiver<Answer>,
) -> Option<Answer> {
    state
        .links
        .deliver(id, ClientFrame::ScreenSignOut {})
        .await
        .ok()?;
    tokio::time::timeout(Duration::from_secs(ANSWER_SECONDS), receiver)
        .await
        .ok()
        .and_then(Result::ok)
}

#[cfg(test)]
#[path = "sign_out_tests.rs"]
mod tests;
