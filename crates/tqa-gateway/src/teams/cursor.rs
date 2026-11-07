//! An agent-team reader's cursor (ADR-0034, the slice S1 contract): a read answers the messages after the reader's
//! last-read seq and is recorded under its key, so a repeat answers the same; the seq advances only when that read's
//! `tool.result` is stored, in the same transaction, which makes delivery exactly once. Recorded answers are pruned
//! with the advance and when their run ends. Pure: the store keeps the same state in rows (GW-U-126).
use std::collections::BTreeMap;

/// A recorded answer: the seqs it returned, and the run that read it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Recorded {
    seqs: Vec<u64>,
    run: String,
}

/// One reader's cursor.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Cursor {
    last_read: u64,
    recorded: BTreeMap<String, Recorded>,
}

impl Cursor {
    /// A cursor whose reader has read through `last_read`.
    pub(crate) fn new(last_read: u64) -> Self {
        Self {
            last_read,
            recorded: BTreeMap::new(),
        }
    }

    /// The seq the reader has read through.
    pub(crate) const fn last_read(&self) -> u64 {
        self.last_read
    }

    /// How many answers are recorded.
    pub(crate) fn recorded(&self) -> usize {
        self.recorded.len()
    }

    /// The read under `key`: the recorded answer when there is one; otherwise the `available` seqs after the last read
    /// and after every answer still outstanding, in order, at most `limit`, recorded for `run`.
    pub(crate) fn answer(
        &mut self,
        key: &str,
        run: &str,
        available: &[u64],
        limit: usize,
    ) -> Vec<u64> {
        if let Some(recorded) = self.recorded.get(key) {
            return recorded.seqs.clone();
        }
        // After the last read and after every answer still outstanding, so two reads never answer the same seq.
        let after = self
            .recorded
            .values()
            .filter_map(|recorded| recorded.seqs.last().copied())
            .fold(self.last_read, u64::max);
        let mut seqs: Vec<u64> = available
            .iter()
            .copied()
            .filter(|seq| *seq > after)
            .collect();
        seqs.sort_unstable();
        seqs.dedup();
        seqs.truncate(limit);
        self.recorded.insert(
            key.to_owned(),
            Recorded {
                seqs: seqs.clone(),
                run: run.to_owned(),
            },
        );
        seqs
    }

    /// The read under `key` has had its result stored: the seq advances to that answer's last (never back), and every
    /// recorded answer at or below it is pruned, that one included. `false`, with nothing changed, for an unknown key.
    pub(crate) fn result_stored(&mut self, key: &str) -> bool {
        let Some(recorded) = self.recorded.remove(key) else {
            return false;
        };
        if let Some(last) = recorded.seqs.last() {
            self.last_read = self.last_read.max(*last);
        }
        let last_read = self.last_read;
        self.recorded
            .retain(|_, recorded| recorded.seqs.last().is_some_and(|last| *last > last_read));
        true
    }

    /// `run` has ended: its remaining answers go, and the seq stays where it is (their messages come with the next read).
    pub(crate) fn run_ended(&mut self, run: &str) {
        self.recorded.retain(|_, recorded| recorded.run != run);
    }
}

#[cfg(test)]
#[path = "cursor_tests.rs"]
mod tests;
