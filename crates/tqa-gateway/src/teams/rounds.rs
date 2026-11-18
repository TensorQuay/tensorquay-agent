//! What `start_member` does with a team in each state (ADR-0034's rounds amendment, A1 and A3): open a round, add to
//! the running one, bring a finished helper back or start a new one, or refuse. Pure, so every cell is tested
//! (GW-U-221); step 1 builds it and nothing uses it yet: step 2 wires it into `start_member`.

/// The lead's team as `start_member` finds it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Team {
    /// The task leads no team yet: its first helper opens round 1.
    None,
    Running {
        round: u32,
    },
    Stopping {
        round: u32,
    },
    /// Ended, for good (its lead task deleted, its person disabled) or for one of a round's reasons.
    Ended {
        round: u32,
        for_good: bool,
    },
}

/// A helper's state, as `agent_team_members.state` holds it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HelperState {
    Starting,
    Queued,
    Working,
    Waiting,
    Stuck,
    Done,
    Stopped,
    Failed,
}

/// One of the team's helpers, of any round.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Helper<'a> {
    pub name: &'a str,
    pub state: HelperState,
    /// A failed helper whose task still runs (its run failed, not its start).
    pub failed_run: bool,
    /// Its task was deleted.
    pub gone: bool,
}

/// The `start_member` call, and what the gateway knows of the run that makes it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Call<'a> {
    pub role: &'a str,
    /// The run may open a round: a person's message started it and it hasn't opened one (A1).
    pub may_open_round: bool,
    /// Seats taken in the current round, and the most a round may have.
    pub seats: (u32, u32),
}

/// Which helper the call starts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Start {
    New(String),
    BringBack(String),
}

/// What `start_member` does.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Outcome {
    /// Opens round `round` (budget and time fresh) with this helper.
    OpenRound { round: u32, start: Start },
    /// Adds this helper to the running round.
    AddToRound(Start),
    /// Refused with this code.
    Refused(&'static str),
}

/// What `start_member` does for `call` on `team` with `helpers`.
pub(crate) fn start_member(team: Team, helpers: &[Helper<'_>], call: Call<'_>) -> Outcome {
    let start = || chosen(helpers, call.role);
    match team {
        Team::None => Outcome::OpenRound {
            round: 1,
            start: start(),
        },
        Team::Stopping { .. } => Outcome::Refused("team_stopped"),
        Team::Ended { for_good: true, .. } => Outcome::Refused("team_ended"),
        // Only a person's follow-up opens a round, once per run (A1, at review).
        Team::Ended { .. } if !call.may_open_round => Outcome::Refused("round_needs_person"),
        // Earlier rounds' helpers hold no seat: the new round starts with none taken.
        Team::Ended { round, .. } => Outcome::OpenRound {
            round: round + 1,
            start: start(),
        },
        Team::Running { .. } if call.seats.0 >= call.seats.1 => Outcome::Refused("member_limit"),
        Team::Running { .. } => Outcome::AddToRound(start()),
    }
}

/// The helper a `role` names (A3): a finished one, matched in any letter case, comes back; otherwise a new one, named
/// as today (`-2`, `-3`) against every helper the team has had.
fn chosen(helpers: &[Helper<'_>], role: &str) -> Start {
    let finished = helpers.iter().find(|helper| {
        helper.name.eq_ignore_ascii_case(role)
            && !helper.gone
            && match helper.state {
                HelperState::Done | HelperState::Stopped | HelperState::Waiting => true,
                HelperState::Failed => helper.failed_run,
                HelperState::Starting
                | HelperState::Queued
                | HelperState::Working
                | HelperState::Stuck => false,
            }
    });
    if let Some(helper) = finished {
        Start::BringBack(helper.name.to_owned())
    } else {
        let names: Vec<&str> = helpers.iter().map(|helper| helper.name).collect();
        Start::New(super::validate::unique_name(role, &names))
    }
}

#[cfg(test)]
#[path = "rounds_tests.rs"]
mod tests;
