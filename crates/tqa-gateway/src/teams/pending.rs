//! The agent-team reads still waiting for their `tool.result`, by reader and call key, kept in memory so that storing
//! an event costs nothing unless it ends one of them (PERF-002). It mirrors `agent_team_reads`: loaded at
//! start, added to by the read route after its commit, and taken from by the event store after the commit that
//! advances the reader's seq. Absent when agent teams are off, so the event path does no work at all then.
use crate::GatewayError;
use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};
use uuid::Uuid;

/// Outstanding reads: (reader, call key) to the run that made it.
#[derive(Debug, Default)]
pub(crate) struct Pending {
    reads: Mutex<HashMap<(Uuid, String), String>>,
}

impl Pending {
    /// Loads the outstanding reads stored before a restart, after dropping those of runs that are no longer current.
    ///
    /// # Errors
    /// Storage failure.
    pub(crate) async fn load(&self, pool: &sqlx::PgPool) -> Result<(), GatewayError> {
        // A read whose run is no longer its reader's current run will never get its result, and its run's end may
        // have passed while agent teams were off: it goes now (GW-I-109).
        sqlx::query!(
            "DELETE FROM agent_team_reads r WHERE r.run_id IS DISTINCT FROM ( \
               SELECT CASE WHEN e.lifecycle IN ('run.started', 'run.resumed') THEN e.run_id END FROM events e \
               WHERE e.conversation_id = r.reader AND e.lifecycle IS NOT NULL ORDER BY e.seq DESC LIMIT 1)"
        )
        .execute(pool)
        .await?;
        let rows = sqlx::query!("SELECT reader, call_key, run_id FROM agent_team_reads")
            .fetch_all(pool)
            .await?;
        let mut reads = self.reads.lock().unwrap_or_else(PoisonError::into_inner);
        for row in rows {
            reads.insert((row.reader, row.call_key), row.run_id);
        }
        Ok(())
    }

    /// A read recorded for `reader` under `key`, by `run`.
    pub(crate) fn insert(&self, reader: Uuid, key: &str, run: &str) {
        self.reads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert((reader, key.to_owned()), run.to_owned());
    }

    /// Of `keys`, those with a read outstanding for `reader`.
    pub(crate) fn outstanding(&self, reader: Uuid, keys: Vec<String>) -> Vec<String> {
        let reads = self.reads.lock().unwrap_or_else(PoisonError::into_inner);
        keys.into_iter()
            .filter(|key| reads.contains_key(&(reader, key.clone())))
            .collect()
    }

    /// Whether `run` has a read outstanding for `reader`.
    pub(crate) fn has_run(&self, reader: Uuid, run: &str) -> bool {
        self.reads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .any(|((who, _), by)| *who == reader && by == run)
    }

    /// `reader`'s reads under `keys` are done: their results are stored.
    pub(crate) fn remove(&self, reader: Uuid, keys: &[String]) {
        let mut reads = self.reads.lock().unwrap_or_else(PoisonError::into_inner);
        for key in keys {
            reads.remove(&(reader, key.clone()));
        }
    }

    /// `reader`'s `run` has ended: its outstanding reads go.
    pub(crate) fn run_ended(&self, reader: Uuid, run: &str) {
        self.reads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|(who, _), by| !(*who == reader && by == run));
    }

    /// How many reads are outstanding.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.reads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

#[cfg(test)]
mod tests {
    use super::Pending;
    use uuid::Uuid;

    #[test]
    fn gw_u_126_the_outstanding_reads_index_follows_records_results_and_runs() {
        let (pending, me, other) = (Pending::default(), Uuid::now_v7(), Uuid::now_v7());
        pending.insert(me, "r-1:c-1", "r-1");
        pending.insert(me, "r-1:c-2", "r-1");
        pending.insert(other, "r-1:c-1", "r-9");
        let keys = vec!["r-1:c-1".to_owned(), "r-1:c-3".to_owned()];
        assert_eq!(
            pending.outstanding(me, keys),
            ["r-1:c-1"],
            "only the recorded key, only for its reader"
        );
        pending.remove(me, &["r-1:c-1".to_owned()]);
        assert!(pending.has_run(me, "r-1"));
        pending.run_ended(me, "r-1");
        assert!(!pending.has_run(me, "r-1"));
        assert_eq!(pending.len(), 1, "another reader's read stays");
    }
}
