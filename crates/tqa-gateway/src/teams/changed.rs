//! `team.changed` (ADR-0034, the slice S6 contract): each agent team's version and the browser-only frame that says it
//! moved. Migration 0079's triggers NOTIFY `agent_team` with `t:<team>` when the team or a helper changes as people see
//! it (`spent_tokens` included, so once per model call on a team: the panel shows spend, and the flush below makes it
//! at most one frame a second), and `c:<channel>` when the team's log gains a message; NOTIFY takes no row lock and is
//! delivered on commit, so the writers are unchanged. One listener marks what changed; once a second the marks are
//! flushed: a team's version rises for a change to it (not for a message alone), and the frame goes to the lead's
//! stream and each member's, at most once a second per team, the latest winning. NOTIFYs sent while the listener is
//! away are lost, so on every (re)connect every open team counts as changed. Versions start at the gateway's start time
//! in milliseconds, so they never go down across a restart (a page reads the team again after a reconnect anyway).
use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, PoisonError},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use sqlx::postgres::PgListener;
use tqa_types::{ConversationId, ServerFrame};
use uuid::Uuid;

use crate::{GatewayError, app::State};

/// The NOTIFY channel migration 0079's triggers use.
const CHANNEL: &str = "agent_team";
/// How often marks are flushed: at most one frame a second per team.
const EVERY: Duration = Duration::from_secs(1);

/// Each team's version, and what changed since the last flush.
pub(crate) struct Changes {
    base: u64,
    versions: Mutex<HashMap<Uuid, u64>>,
    marked: Mutex<Marked>,
}

/// What changed since the last flush.
#[derive(Debug, Default)]
struct Marked {
    teams: HashSet<Uuid>,
    channels: HashSet<Uuid>,
    /// Every open team: the listener (re)connected, so changes may have gone unannounced.
    all: bool,
}

impl Marked {
    fn is_empty(&self) -> bool {
        !self.all && self.teams.is_empty() && self.channels.is_empty()
    }
}

impl Default for Changes {
    fn default() -> Self {
        let base = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| {
                u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
            });
        Self {
            base,
            versions: Mutex::default(),
            marked: Mutex::default(),
        }
    }
}

impl Changes {
    /// The team's version now.
    pub(crate) fn version(&self, team: Uuid) -> u64 {
        self.versions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&team)
            .copied()
            .unwrap_or(self.base)
    }

    /// Raises the team's version; answers the new one.
    fn bump(&self, team: Uuid) -> u64 {
        let mut versions = self.versions.lock().unwrap_or_else(PoisonError::into_inner);
        let version = versions.entry(team).or_insert(self.base);
        *version += 1;
        *version
    }

    /// Marks what a NOTIFY's payload names (`t:<team>` or `c:<channel>`); anything else is ignored.
    fn mark(&self, payload: &str) {
        let mut marked = self.marked.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(team) = payload
            .strip_prefix("t:")
            .and_then(|id| Uuid::parse_str(id).ok())
        {
            marked.teams.insert(team);
        } else if let Some(channel) = payload
            .strip_prefix("c:")
            .and_then(|id| Uuid::parse_str(id).ok())
        {
            marked.channels.insert(channel);
        }
    }

    /// Marks every open team.
    fn mark_all(&self) {
        self.marked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .all = true;
    }

    /// What changed since the last flush, leaving nothing marked.
    fn take(&self) -> Marked {
        std::mem::take(&mut *self.marked.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// Starts the listener and the flush, while agent teams are on; both stop with the gateway.
pub(crate) fn start(state: &std::sync::Arc<State>) {
    if !state.config.agent_teams.enabled {
        return;
    }
    let listening = state.clone();
    state.tasks.spawn(async move { listen(&listening).await });
    let flushing = state.clone();
    state
        .tasks
        .spawn(async move { flush_every_second(&flushing).await });
}

/// Listens for the triggers' NOTIFYs, reconnecting whenever the connection goes.
async fn listen(state: &State) {
    loop {
        let connected = tokio::select! {
            () = state.cancel.cancelled() => return,
            connected = connect(state) => connected,
        };
        let mut listener = match connected {
            Ok(listener) => listener,
            Err(error) => {
                tracing::warn!(%error, "the agent teams' change listener couldn't connect; retrying");
                tokio::select! {
                    () = state.cancel.cancelled() => return,
                    () = tokio::time::sleep(EVERY) => continue,
                }
            }
        };
        // Anything announced while no one listened is lost.
        state.team_changes.mark_all();
        loop {
            let next = tokio::select! {
                () = state.cancel.cancelled() => return,
                next = listener.try_recv() => next,
            };
            match next {
                Ok(Some(notification)) => state.team_changes.mark(notification.payload()),
                // The connection dropped and the listener reconnected: the gap's NOTIFYs are lost.
                Ok(None) => state.team_changes.mark_all(),
                Err(error) => {
                    tracing::warn!(%error, "the agent teams' change listener stopped; reconnecting");
                    break;
                }
            }
        }
    }
}

async fn connect(state: &State) -> Result<PgListener, sqlx::Error> {
    let mut listener = PgListener::connect_with(&state.pool).await?;
    listener.listen(CHANNEL).await?;
    Ok(listener)
}

/// Flushes the marks once a second.
async fn flush_every_second(state: &State) {
    let mut timer = tokio::time::interval(EVERY);
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            () = state.cancel.cancelled() => return,
            _ = timer.tick() => {}
        }
        let marked = state.team_changes.take();
        if marked.is_empty() {
            continue;
        }
        if let Err(error) = flush(state, &marked).await {
            // Tried again at the next tick, with every open team.
            tracing::warn!(code = error.code(), "agent team changes weren't announced");
            state.team_changes.mark_all();
        }
    }
}

/// Raises the changed teams' versions and sends each marked team's frame to its lead's and members' streams.
async fn flush(state: &State, marked: &Marked) -> Result<(), GatewayError> {
    let teams: Vec<Uuid> = marked.teams.iter().copied().collect();
    let channels: Vec<Uuid> = marked.channels.iter().copied().collect();
    let rows = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query!(
            "SELECT t.id, t.lead_conversation, c.last_seq, \
             ARRAY(SELECT m.conversation_id FROM agent_team_members m WHERE m.team_id = t.id) AS \"members!\" \
             FROM agent_teams t JOIN channels c ON c.id = t.channel_id \
             WHERE ($3 AND t.state <> 'ended') OR t.id = ANY($1) OR t.channel_id = ANY($2)",
            &teams,
            &channels,
            marked.all
        )
        .fetch_all(&state.pool)
        .await?)
    })
    .await?;
    for row in rows {
        let version = if marked.all || marked.teams.contains(&row.id) {
            state.team_changes.bump(row.id)
        } else {
            state.team_changes.version(row.id)
        };
        let frame = ServerFrame::TeamChanged {
            team: row.id.to_string(),
            version,
            last_seq: u64::try_from(row.last_seq).unwrap_or(0),
        };
        for task in std::iter::once(row.lead_conversation).chain(row.members) {
            let Ok(id) = task.to_string().parse::<ConversationId>() else {
                continue;
            };
            if let Err(error) = state.events.control(&id, frame.clone()) {
                tracing::debug!(code = error.code(), "a team.changed frame wasn't delivered");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "changed_tests.rs"]
mod tests;
