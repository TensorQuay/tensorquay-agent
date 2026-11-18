//! What `start_member` does with a team in each state (ADR-0034's rounds amendment, A1 and A3): open a round, add to
//! the running one, bring a finished helper back or start a new one, or refuse. Pure, so every cell is tested
//! (GW-U-221); step 1 builds it and nothing uses it yet: step 2 wires it into `start_member`.
use super::validate::unique_name;

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
    let names: Vec<&str> = helpers.iter().map(|helper| helper.name).collect();
    let new = || Start::New(unique_name(call.role, &names));
    match team {
        Team::None => Outcome::OpenRound {
            round: 1,
            start: new(),
        },
        Team::Running { .. } if call.seats.0 >= call.seats.1 => Outcome::Refused("member_limit"),
        Team::Running { .. } => Outcome::AddToRound(new()),
        Team::Stopping { .. } => Outcome::Refused("team_stopped"),
        Team::Ended { .. } => Outcome::Refused("team_ended"),
    }
}

#[cfg(test)]
#[path = "rounds_tests.rs"]
mod tests;
