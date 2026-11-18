//! The role a run gets at its start (`GET /runtime/teams/role`, ADR-0034 point 2 and the rounds amendment's A2 and
//! A7), worked out from what the store says, without the store: what the route reads, then this.

/// What the role route read about the caller's task.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Facts {
    /// The install's `[agent_teams] enabled`.
    pub teams_on: bool,
    /// The task's place in a team now: its lead, a helper, or none.
    pub place: Place,
    /// The task was ever a helper (depth 1: it never leads).
    pub was_member: bool,
    /// The last team it led, when that team has ended.
    pub ended: Option<Ended>,
    /// Its runtime is linked at the wake's revision or above.
    pub wakeable: bool,
}

/// The task's place in a team that hasn't ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Place {
    Lead,
    Member,
    None,
}

/// A team the task led that has ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Ended {
    /// It ended for good (its lead task deleted, its person disabled), not for one of a round's reasons.
    pub for_good: bool,
    /// The lead hasn't read its end note yet.
    pub note_unread: bool,
}

/// The route's answer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Answer {
    pub role: &'static str,
    pub reason: Option<&'static str>,
}

/// The role a run with `facts` gets.
pub(crate) fn answer(facts: Facts) -> Answer {
    let role = |role| Answer { role, reason: None };
    if !facts.teams_on {
        return role("none");
    }
    match facts.place {
        Place::Lead => return role("lead"),
        Place::Member => return role("member"),
        Place::None => {}
    }
    if facts.was_member {
        return role("none");
    }
    match facts.ended {
        Some(ended) if ended.note_unread => role("ended"),
        None if facts.wakeable => role("may_lead"),
        Some(_) | None => role("none"),
    }
}

#[cfg(test)]
#[path = "role_tests.rs"]
mod tests;
