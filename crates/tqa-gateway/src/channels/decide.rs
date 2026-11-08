//! The decision evaluator (ADR-0014 point 9): a proposal's outcome from its voting members' latest votes, the
//! rule and whether its window is still open. Pure, so the rules are tested apart from storage (GW-U-065).
use tqa_types::{DecisionRule, Outcome, VoteChoice};

/// A voting member: its identity (kind and id) and whether it is a person.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Voter {
    pub key: (String, String),
    pub human: bool,
}

/// A vote as stored: who, what, and its seq (later seqs replace earlier ones from the same voter).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Ballot {
    pub voter: (String, String),
    pub choice: VoteChoice,
    pub seq: u64,
}

/// The proposal's outcome, or `None` while it is still undecided and open.
///
/// - Only current voting members' votes count, each member's latest one.
/// - A veto by a person who votes ends it at once: `vetoed`.
/// - `human`: the first accept or reject by a person who votes decides; agents' votes are advice.
/// - `majority`: more than half of the voting members accepting, or rejecting, decides; when every one of them has
///   voted without that, `undecided`.
/// - Otherwise, once the window has closed: `timed_out`.
pub(crate) fn outcome(
    rule: DecisionRule,
    voters: &[Voter],
    ballots: &[Ballot],
    open: bool,
) -> Option<Outcome> {
    let member = |key: &(String, String)| voters.iter().find(|voter| &voter.key == key);
    let mut counted: Vec<&Ballot> = ballots
        .iter()
        .filter(|ballot| member(&ballot.voter).is_some())
        .collect();
    counted.sort_by_key(|ballot| ballot.seq);
    if counted.iter().any(|ballot| {
        ballot.choice == VoteChoice::Veto && member(&ballot.voter).is_some_and(|voter| voter.human)
    }) {
        return Some(Outcome::Vetoed);
    }
    let decided = match rule {
        DecisionRule::Human => counted
            .iter()
            .find(|ballot| {
                member(&ballot.voter).is_some_and(|voter| voter.human)
                    && ballot.choice != VoteChoice::Veto
            })
            .map(|ballot| match ballot.choice {
                VoteChoice::Accept => Outcome::Accepted,
                _ => Outcome::Rejected,
            }),
        DecisionRule::Majority => majority(voters.len(), &counted),
    };
    decided.or((!open).then_some(Outcome::TimedOut))
}

fn majority(voting: usize, counted: &[&Ballot]) -> Option<Outcome> {
    // Each member's latest vote.
    let mut latest: Vec<&Ballot> = Vec::new();
    for ballot in counted.iter().rev() {
        if !latest.iter().any(|seen| seen.voter == ballot.voter) {
            latest.push(ballot);
        }
    }
    let count = |choice| {
        latest
            .iter()
            .filter(|ballot| ballot.choice == choice)
            .count()
    };
    let (accept, reject) = (count(VoteChoice::Accept), count(VoteChoice::Reject));
    if accept * 2 > voting {
        Some(Outcome::Accepted)
    } else if reject * 2 > voting {
        Some(Outcome::Rejected)
    } else if latest.len() >= voting && voting > 0 {
        Some(Outcome::Undecided)
    } else {
        None
    }
}

#[cfg(test)]
#[path = "decide_tests.rs"]
mod tests;
