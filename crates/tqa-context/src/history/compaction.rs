//! Validation of a `compaction` event against the history it summarises.
use super::{Compacted, Entry, Envelope, History, invalid};
use crate::ProjectionError;
use tqa_types::{CompactionSummary, Seq};

impl History {
    /// Accepts a compaction only between steps of the active run, at an entry boundary that
    /// splits no step, and strictly after any earlier compaction.
    pub(super) fn compact(
        &mut self,
        through: Seq,
        summary: &CompactionSummary,
        envelope: Envelope<'_>,
    ) -> Result<(), ProjectionError> {
        let reject = |reason| Err(invalid(envelope.seq, reason));
        if self.active.is_none() || envelope.run != self.active.as_ref() {
            return reject("compaction outside its active run");
        }
        // Between attempts (no model response yet) is allowed: a route switch re-checks the window
        // there. Once a response is recorded, its tool phase must finish first.
        if self.current.is_some_and(|index| {
            let step = &self.steps[index];
            !step.completed && step.attempts.last().is_some_and(|attempt| attempt.complete)
        }) {
            return reject("compaction inside an unfinished step");
        }
        let previous = self
            .compacted
            .as_ref()
            .map_or(Seq::ZERO, |done| done.through);
        if through >= envelope.seq || through <= previous {
            return reject("compaction boundary out of order");
        }
        if !self.starts.iter().any(|(_, before)| *before == through) {
            return reject("compaction boundary is not a turn boundary");
        }
        if self
            .steps
            .iter()
            .any(|step| step.first_seq <= through && step.last_seq > through)
        {
            return reject("compaction boundary splits a step");
        }
        if summary.validate().is_err() {
            return reject("compaction summary out of bounds");
        }
        let current_user = self
            .entries
            .iter()
            .rposition(|entry| matches!(entry, Entry::User(_)));
        self.compacted = Some(Compacted {
            through,
            summary: summary.clone(),
            current_user,
        });
        Ok(())
    }
}
