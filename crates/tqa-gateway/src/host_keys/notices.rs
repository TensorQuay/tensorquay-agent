//! The task's live notices of keyed calls (ADR-0037, Amendment 1): `host_key.used` once per run and key, and
//! `host_key.refused` at most once a minute per run, key and outcome, so a retry loop doesn't flood the page. Each
//! carries the run, the step running when the proxy saw the call and the run's sender; the key's name reaches only the
//! sender's own streams ([`for_viewer`]).
use super::use_rule::Outcome;
use crate::app::State;
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};
use tqa_types::{ConversationId, HostKeyOutcome, RunId, ServerFrame};
use uuid::Uuid;

/// A refusal's notice repeats at most this often per run, key and outcome.
const REFUSED_WINDOW: Duration = Duration::from_mins(1);
/// Entries kept; the oldest go first, so a scan of many hosts cannot grow memory.
const MAX_ENTRIES: usize = 4096;

/// One keyed call the proxy saw.
pub(crate) struct Call<'a> {
    pub(crate) conversation: &'a ConversationId,
    pub(crate) run: &'a str,
    pub(crate) sender: Uuid,
    /// The key, when the placeholder named one; none for `absent`.
    pub(crate) key: Option<Uuid>,
    /// Its name, only when the caller is the key's own sender: a teammate's key is never named to anyone else.
    pub(crate) name: Option<&'a str>,
    pub(crate) host: &'a str,
    pub(crate) port: u16,
}

/// Which notices went out: `used` for good per run and key, a refusal per run, key and outcome until its window ends.
#[derive(Default)]
pub(crate) struct Notices(Mutex<HashMap<Slot, Instant>>);

/// A notice's run, key (none for `absent`) and outcome.
type Slot = (String, Option<Uuid>, &'static str);

impl Notices {
    /// Whether this notice is due now; records it when it is.
    pub(crate) fn due(&self, run: &str, key: Option<Uuid>, outcome: Outcome, now: Instant) -> bool {
        let Ok(mut sent) = self.0.lock() else {
            return false;
        };
        let slot = (run.to_owned(), key, outcome.as_str());
        if let Some(last) = sent.get(&slot)
            && (outcome == Outcome::Ok || now.duration_since(*last) < REFUSED_WINDOW)
        {
            return false;
        }
        if sent.len() >= MAX_ENTRIES {
            sent.retain(|_, last| now.duration_since(*last) < REFUSED_WINDOW);
            if sent.len() >= MAX_ENTRIES {
                return false;
            }
        }
        sent.insert(slot, now);
        true
    }
}

/// Publishes the notice of `call` with `outcome` to the task's streams, when due. The step is the run's latest the
/// gateway holds, while the run is going.
pub(crate) async fn notify(state: &State, call: &Call<'_>, outcome: Outcome) {
    if !state
        .host_keys
        .notices
        .due(call.run, call.key, outcome, Instant::now())
    {
        return;
    }
    let Ok(run_id) = call.run.parse::<RunId>() else {
        return;
    };
    let step = step(state, call).await;
    let (sender_id, name) = (call.sender.to_string(), call.name.map(str::to_owned));
    let (host, port) = (call.host.to_owned(), call.port);
    let frame = match wire(outcome) {
        None => ServerFrame::HostKeyUsed {
            run_id,
            step,
            sender_id,
            name,
            host,
            port,
        },
        Some(outcome) => ServerFrame::HostKeyRefused {
            run_id,
            step,
            sender_id,
            name,
            host,
            port,
            outcome,
        },
    };
    if let Err(error) = state.events.control(call.conversation, frame) {
        tracing::debug!(
            code = error.code(),
            "a host key notice could not be published"
        );
    }
}

/// The frame's outcome for a refusal; None for a use.
const fn wire(outcome: Outcome) -> Option<HostKeyOutcome> {
    Some(match outcome {
        Outcome::Ok => return None,
        Outcome::Absent => HostKeyOutcome::Absent,
        Outcome::Revoked => HostKeyOutcome::Revoked,
        Outcome::ForeignSandbox => HostKeyOutcome::ForeignSandbox,
        Outcome::ForeignRun => HostKeyOutcome::ForeignRun,
        Outcome::Expired => HostKeyOutcome::Expired,
        Outcome::WrongHost => HostKeyOutcome::WrongHost,
        Outcome::WrongPosition => HostKeyOutcome::WrongPosition,
        Outcome::NotAllowed => HostKeyOutcome::NotAllowed,
        Outcome::Unrecorded => HostKeyOutcome::Unrecorded,
        Outcome::Unavailable => HostKeyOutcome::Unavailable,
    })
}

/// The step running in the call's run, from the run's latest event with a step; none once the run has ended.
async fn step(state: &State, call: &Call<'_>) -> Option<u32> {
    let conversation = crate::conversations::uuid(call.conversation).ok()?;
    let latest = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query_scalar!(
            "SELECT step FROM events WHERE conversation_id=$1 AND run_id=$2 AND step IS NOT NULL \
             AND NOT EXISTS (SELECT 1 FROM events e WHERE e.conversation_id=$1 AND e.run_id=$2 \
             AND e.lifecycle IN ('run.completed', 'run.failed', 'run.cancelled')) ORDER BY seq DESC LIMIT 1",
            conversation,
            call.run
        )
        .fetch_optional(&state.pool)
        .await?)
    })
    .await
    .ok()
    .flatten()
    .flatten()?;
    u32::try_from(latest).ok()
}

/// The frame as `viewer` receives it: a key's name only on the run's sender's own streams (least exposure).
pub(crate) fn for_viewer(frame: ServerFrame, viewer: Uuid) -> ServerFrame {
    let viewer = viewer.to_string();
    match frame {
        ServerFrame::HostKeyUsed {
            run_id,
            step,
            sender_id,
            name,
            host,
            port,
        } => ServerFrame::HostKeyUsed {
            name: name.filter(|_| sender_id == viewer),
            run_id,
            step,
            sender_id,
            host,
            port,
        },
        ServerFrame::HostKeyRefused {
            run_id,
            step,
            sender_id,
            name,
            host,
            port,
            outcome,
        } => ServerFrame::HostKeyRefused {
            name: name.filter(|_| sender_id == viewer),
            run_id,
            step,
            sender_id,
            host,
            port,
            outcome,
        },
        frame => frame,
    }
}

#[cfg(test)]
#[path = "notices_tests.rs"]
mod tests;
