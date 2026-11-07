//! The reconciler's decisions, from one team's stored state at one instant (ADR-0034 points 9-11): pure, so every rule
//! is tested without a database (GW-U-130). Times are Unix seconds. Repeats are the state's to rule out: a member's
//! `told` flags are true when that notice was already sent since the member's last progress or
//! assignment, so a notice goes once per episode.
use tqa_types::NoticeKind;

/// The thresholds from `[agent_teams]`, in seconds.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Thresholds {
    pub member_idle: i64,
    pub member_stuck: i64,
    pub idle_stop: i64,
}

/// A team's state, as the reconciler sees it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TeamState {
    Running,
    Stopping,
    Ended,
}

/// One team at one instant.
#[derive(Clone, Debug)]
pub(crate) struct TeamView {
    pub state: TeamState,
    pub deadline_at: i64,
    pub deadline_minutes: u32,
    pub deadline_notified: bool,
    /// The latest of: a message stored in the team's log, a run of the lead or a member starting or ending.
    pub last_activity_at: i64,
    /// Whether any run of the lead or a member is active or queued now.
    pub any_run_active: bool,
    pub members: Vec<MemberView>,
}

/// A member's current run, as far as liveness cares.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Run {
    /// No run yet, or its state is the member's own (`starting`, `queued`).
    None,
    /// Active; `last_progress_at` is when a model call or a tool call last finished (or the run started).
    Active { last_progress_at: i64 },
    /// Ended normally at `at`.
    Ended { at: i64 },
    /// Failed, with the run's own reason (the runtime's words, never an agent's); its code stays in the transcript.
    Failed { reason: String },
}

/// One member at one instant.
#[derive(Clone, Debug)]
pub(crate) struct MemberView {
    pub name: String,
    /// The stored state: `starting`, `queued`, `working`, `waiting`, `stuck`, `done`, `stopped` or `failed`.
    pub state: String,
    pub run: Run,
    /// A report from it since its last assignment.
    pub reported_since_assignment: bool,
    /// Something it hasn't read is waiting for it (an assignment, a decision, a wake).
    pub waiting_for_it: bool,
    /// What the lead was already told in this episode.
    pub told: Told,
}

/// The notices about a member the lead already has since its last progress or assignment.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Told {
    pub idle: bool,
    pub stuck: bool,
    pub failed: bool,
}

/// Why a team stops (`agent_teams.stop_reason`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StopReason {
    Deadline,
    Idle,
}

impl StopReason {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Deadline => "deadline",
            Self::Idle => "idle",
        }
    }
}

/// What the reconciler does for one team now; each is idempotent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Action {
    /// A notice to the lead: about a member, or about the whole team when `member` is `None`.
    Notice {
        kind: NoticeKind,
        member: Option<String>,
        text: String,
    },
    /// The member's state becomes `stuck`, or back to `working` once it makes progress.
    MarkStuck(String),
    MarkWorking(String),
    /// The team starts stopping (`running` → `stopping`) for `reason`.
    StartStop(StopReason),
    /// A stopping team is carried to `ended` (`teams::stop`).
    FinishStop,
}

/// How long before the deadline the lead is told: 10 minutes, or 20 % of the deadline when that is shorter (the
/// lead's decision of 2 Oct 2026: 60 minutes warns at 10 left, 20 at 4, 5 at 1).
pub(crate) fn deadline_warning_secs(deadline_minutes: u32) -> i64 {
    (i64::from(deadline_minutes) * 60 / 5).min(600)
}

/// Whole minutes, at least one, for the notices' words.
fn minutes(secs: i64) -> i64 {
    ((secs + 59) / 60).max(1)
}

/// The team's next actions at `now`.
pub(crate) fn decide(team: &TeamView, now: i64, thresholds: &Thresholds) -> Vec<Action> {
    match team.state {
        TeamState::Ended => return Vec::new(),
        TeamState::Stopping => return vec![Action::FinishStop],
        TeamState::Running => {}
    }
    if now >= team.deadline_at {
        return vec![Action::StartStop(StopReason::Deadline)];
    }
    if !team.any_run_active && now - team.last_activity_at >= thresholds.idle_stop {
        // The members' notices due in this pass go first (a helper that ended without reporting, one that failed): with
        // equal limits they fall due with the idle stop, and the lead would otherwise never be told (E2E-AGENTS-029).
        let mut actions: Vec<Action> = team
            .members
            .iter()
            .filter(|member| !matches!(member.state.as_str(), "stopped" | "done"))
            .flat_map(|member| member_actions(member, now, thresholds))
            .filter(|action| matches!(action, Action::Notice { .. }))
            .collect();
        actions.push(Action::StartStop(StopReason::Idle));
        return actions;
    }
    let mut actions = Vec::new();
    let left = team.deadline_at - now;
    if !team.deadline_notified && left <= deadline_warning_secs(team.deadline_minutes) {
        actions.push(Action::Notice {
            kind: NoticeKind::DeadlineNear,
            member: None,
            text: format!("The team's time is nearly up: {} min left.", minutes(left)),
        });
    }
    for member in &team.members {
        if matches!(member.state.as_str(), "stopped" | "done") {
            continue;
        }
        actions.extend(member_actions(member, now, thresholds));
    }
    actions
}

fn member_actions(member: &MemberView, now: i64, thresholds: &Thresholds) -> Vec<Action> {
    let name = &member.name;
    // A failed member's run may still read as unfinished (a sandbox given up after repeated loss, GW-I-254): the lead
    // was told it failed, so it is neither stuck nor idle until its next run makes it working (GW-I-260).
    let failed = member.state == "failed";
    match &member.run {
        Run::Failed { reason } if !member.told.failed => vec![Action::Notice {
            kind: NoticeKind::MemberFailed,
            member: Some(name.clone()),
            text: format!("{name} failed: {}.", reason.trim_end_matches('.')),
        }],
        Run::Active { .. } | Run::Ended { .. } if failed => Vec::new(),
        Run::Active { last_progress_at } => {
            let quiet = now - last_progress_at;
            if quiet >= thresholds.member_stuck {
                let mut actions = Vec::new();
                if member.state != "stuck" {
                    actions.push(Action::MarkStuck(name.clone()));
                }
                if !member.told.stuck {
                    actions.push(Action::Notice {
                        kind: NoticeKind::MemberStuck,
                        member: Some(name.clone()),
                        text: format!(
                            "{name} seems stuck: nothing has moved for {} minutes.",
                            minutes(quiet)
                        ),
                    });
                }
                actions
            } else if member.state == "stuck" {
                vec![Action::MarkWorking(name.clone())]
            } else {
                Vec::new()
            }
        }
        Run::Ended { at }
            if !member.reported_since_assignment
                && !member.waiting_for_it
                && !member.told.idle
                && now - at >= thresholds.member_idle =>
        {
            vec![Action::Notice {
                kind: NoticeKind::MemberIdle,
                member: Some(name.clone()),
                text: format!(
                    "{name} stopped {} minutes ago without reporting.",
                    minutes(now - at)
                ),
            }]
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
#[path = "decide_tests.rs"]
mod tests;
