//! Activeness follows durable lifecycle events; idle time uses a monotonic clock.

use crate::GatewayError;
use tqa_types::{Event, RunId, Seq, ServerFrame};

/// How long a busy sandbox at its max lifetime may finish its run before it is retired busy: long
/// enough for a typical tool call and one model step, so little work is redone (fixed by design, not a setting).
pub(super) const LIFETIME_GRACE: std::time::Duration = std::time::Duration::from_mins(2);

/// A sandbox against its max lifetime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Lifetime {
    /// Younger than its lifetime.
    Within,
    /// Past its lifetime with no active run: retire it.
    Retire,
    /// Past its lifetime and busy, within the grace: take no new work, let the run finish.
    Grace,
    /// Past its lifetime and the grace, still busy: retire it; the run resumes in a new sandbox.
    RetireBusy,
}

pub(super) struct Idle {
    pub sequence: Seq,
    pub action: Option<time::OffsetDateTime>,
    active: Option<RunId>,
    since: std::time::Duration,
    started: std::time::Duration,
    /// The lifetime the sandbox was admitted with, once a pass has read it.
    cap: Option<std::time::Duration>,
    /// This worker's replacements of a sandbox that stopped unexpectedly.
    pub losses: super::losses::Losses,
    clock: std::sync::Arc<dyn super::IdleClock>,
}

impl Idle {
    pub fn new(clock: std::sync::Arc<dyn super::IdleClock>) -> Self {
        Self {
            sequence: Seq::ZERO,
            action: None,
            active: None,
            since: clock.now(),
            started: clock.now(),
            cap: None,
            losses: super::losses::Losses::default(),
            clock,
        }
    }

    /// The manager clock's current reading.
    pub fn now(&self) -> std::time::Duration {
        self.clock.now()
    }

    /// The run in progress, when this worker has seen it start and not end.
    pub const fn active(&self) -> Option<&RunId> {
        self.active.as_ref()
    }

    pub fn action(&mut self, value: time::OffsetDateTime) {
        if self.action != Some(value) {
            self.action = Some(value);
            self.since = self.clock.now();
        }
    }

    pub fn observe(&mut self, frame: &ServerFrame) -> Result<(), GatewayError> {
        if let Some(sequence) = crate::wire::sequence(frame) {
            self.sequence = sequence;
        }
        let ServerFrame::Event { event, .. } = frame else {
            return Ok(());
        };
        match event {
            Event::RunStarted { run_id } | Event::RunResumed { run_id, .. } => {
                if self.active.as_ref().is_some_and(|active| active != run_id) {
                    return Err(GatewayError::StoreUnavailable);
                }
                self.active = Some(run_id.clone());
                self.since = self.clock.now();
            }
            Event::RunCompleted { run_id }
            | Event::RunCancelled { run_id }
            | Event::RunFailed { run_id, .. } => {
                if self.active.as_ref() == Some(run_id) {
                    self.active = None;
                }
                self.since = self.clock.now();
            }
            _ => {}
        }
        Ok(())
    }

    /// The sandbox was replaced (a lost instance recreated): its lifetime starts again.
    pub fn started_now(&mut self) {
        self.started = self.clock.now();
        self.cap = None;
    }

    /// Whether the last lifetime check found the sandbox past its cap (it then takes no new work).
    pub fn past_lifetime(&self) -> bool {
        self.cap
            .is_some_and(|cap| self.clock.now().saturating_sub(self.started) >= cap)
    }

    /// Where the sandbox stands against its max lifetime (ARCHITECTURE §4.4, reaper rule 1), measured from its start.
    /// An idle sandbox is retired; a busy one gets [`LIFETIME_GRACE`] to finish and is then retired while busy, its run
    /// resuming from the journal in a new sandbox.
    pub fn lifetime(&mut self, lifetime: std::time::Duration) -> Lifetime {
        self.cap = Some(lifetime);
        let age = self.clock.now().saturating_sub(self.started);
        if age < lifetime {
            return Lifetime::Within;
        }
        if self.active.is_none() {
            return Lifetime::Retire;
        }
        if age < lifetime.saturating_add(LIFETIME_GRACE) {
            Lifetime::Grace
        } else {
            Lifetime::RetireBusy
        }
    }

    pub fn expired(&self, window: std::time::Duration) -> bool {
        let elapsed = self.clock.now().saturating_sub(self.since);
        self.active.is_none() && elapsed >= window
    }
}

#[cfg(test)]
#[path = "idle_tests.rs"]
mod tests;
