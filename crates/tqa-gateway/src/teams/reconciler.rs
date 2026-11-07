//! The agent teams' reconciler (ADR-0034 point 11): one loop derives each team's next action from stored state (idle,
//! stuck and failed notices to the lead, the deadline's warning and stop, the idle stop, finishing a stop). Every
//! action is idempotent, so a restart resumes where it was. Off with `[agent_teams] enabled = false`: no loop at all.
pub(crate) mod decide;
pub(crate) mod load;

use crate::{GatewayError, app::State};

/// The liveness thresholds in force: an administrator's overrides, else the `[agent_teams]` file settings (ADR-0034 S6),
/// read on every pass so a change applies from the next one.
pub(crate) fn thresholds(state: &State) -> decide::Thresholds {
    let limits = state.settings.current().limits;
    let secs = |minutes: u32| i64::from(minutes) * 60;
    decide::Thresholds {
        member_idle: secs(limits.member_idle_minutes),
        member_stuck: secs(limits.member_stuck_minutes),
        idle_stop: secs(limits.team_idle_stop_minutes),
    }
}

/// How often the reconciler looks at every open team: well inside the thresholds, which are minutes.
pub(crate) const EVERY: std::time::Duration = std::time::Duration::from_secs(15);

/// The reconciler's wake-up. A stop left to it (the lead's `done`, the budget, a person's stop it couldn't finish)
/// calls [`Nudge::wake`] after its commit, so the stop is carried out within a second, not at the next pass
/// (GW-I-261).
#[derive(Default)]
pub(crate) struct Nudge {
    wake: tokio::sync::Notify,
    held: std::sync::atomic::AtomicBool,
}

impl Nudge {
    /// Runs a pass soon (one wake is stored while a pass runs, so a stop made during it isn't missed).
    pub fn wake(&self) {
        self.wake.notify_one();
    }

    /// Holds the loop's passes (`true`) or lets them run, so a test can look at a team still `stopping`; a pass called
    /// directly still runs. Only with the `testing` feature, never in a release build.
    #[cfg(feature = "testing")]
    pub fn hold(&self, held: bool) {
        self.held.store(held, std::sync::atomic::Ordering::SeqCst);
        if !held {
            self.wake();
        }
    }

    fn held(&self) -> bool {
        self.held.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// Starts the reconciler, which stops with the gateway; nothing at all while agent teams are off.
pub(crate) fn start(state: &std::sync::Arc<State>) {
    if !state.config.agent_teams.enabled {
        return;
    }
    let owned = state.clone();
    state.tasks.spawn(async move {
        let mut every = tokio::time::interval(EVERY);
        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                () = owned.cancel.cancelled() => break,
                _ = every.tick() => {}
                () = owned.reconciler.wake.notified() => {}
            }
            if owned.reconciler.held() {
                continue;
            }
            if let Err(error) = Box::pin(pass(&owned)).await {
                tracing::warn!(
                    code = error.code(),
                    "the agent teams' reconciler pass failed; it runs again"
                );
            }
        }
    });
}

/// One pass: every open team's actions now, a team's failure logged and the next team taken.
///
/// # Errors
/// Storage failure reading the teams.
pub(crate) async fn pass(state: &State) -> Result<usize, GatewayError> {
    if !state.config.agent_teams.enabled {
        return Ok(0);
    }
    let thresholds = thresholds(state);
    let (now, teams) = super::stop::step(state, async {
        let mut connection = state.pool.acquire().await?;
        let now =
            sqlx::query_scalar!("SELECT EXTRACT(EPOCH FROM CURRENT_TIMESTAMP)::bigint AS \"now!\"")
                .fetch_one(&mut *connection)
                .await?;
        Ok((now, load::open_teams(&mut connection).await?))
    })
    .await?;
    let mut done = 0;
    for team in teams {
        for action in decide::decide(&team.view, now, &thresholds) {
            match apply(state, &team, action).await {
                Ok(()) => done += 1,
                Err(error) => {
                    tracing::warn!(team = %team.id, code = error.code(), "an agent team's action wasn't done; next pass");
                }
            }
        }
        // Readers with unread messages and no active run are woken: the notices just posted, and any wake a post
        // couldn't send (a restart between its commit and its wake). A team no longer running wakes no one.
        if let Err(error) =
            Box::pin(super::stop::step(state, super::wake::team(state, team.id))).await
        {
            tracing::warn!(team = %team.id, code = error.code(), "an agent team's wake wasn't sent; next pass");
        }
    }
    Ok(done)
}

/// Does one action; each is idempotent, so a pass cut short repeats it harmlessly.
async fn apply(
    state: &State,
    team: &load::OpenTeam,
    action: decide::Action,
) -> Result<(), GatewayError> {
    match action {
        decide::Action::Notice { kind, member, text } => {
            let seq = super::stop::step(state, async {
                let mut transaction = state.pool.begin().await?;
                let seq = match member {
                    Some(member) => {
                        super::store::notice(&mut transaction, team.channel, (kind, &member), &text)
                            .await?
                    }
                    None => {
                        super::store::team_notice(&mut transaction, team.channel, kind, &text)
                            .await?
                    }
                };
                transaction.commit().await?;
                Ok(seq)
            })
            .await?;
            state.channels.published(team.channel, seq);
        }
        decide::Action::MarkStuck(name) => {
            member_state(state, team.id, &name, "stuck", &["working", "waiting"]).await?;
        }
        decide::Action::MarkWorking(name) => {
            member_state(state, team.id, &name, "working", &["stuck"]).await?;
        }
        decide::Action::StartStop(reason) => {
            super::stop::step(state, async {
                sqlx::query!(
                    "UPDATE agent_teams SET state = 'stopping', stop_reason = $2 WHERE id = $1 AND state = 'running'",
                    team.id,
                    reason.as_str()
                )
                .execute(&state.pool)
                .await?;
                Ok(())
            })
            .await?;
            super::stop::finish(state, team.id).await?;
        }
        decide::Action::FinishStop => {
            super::stop::finish(state, team.id).await?;
        }
    }
    Ok(())
}

/// Sets a member's state, only from one of `from` (so a member stopped or done meanwhile is left alone).
async fn member_state(
    state: &State,
    team: uuid::Uuid,
    name: &str,
    to: &str,
    from: &[&str],
) -> Result<(), GatewayError> {
    let from: Vec<String> = from.iter().map(|state| (*state).to_owned()).collect();
    super::stop::step(state, async {
        sqlx::query!(
            "UPDATE agent_team_members SET state = $3 WHERE team_id = $1 AND name = $2 AND state = ANY($4)",
            team,
            name,
            to,
            &from
        )
        .execute(&state.pool)
        .await?;
        Ok(())
    })
    .await
}
