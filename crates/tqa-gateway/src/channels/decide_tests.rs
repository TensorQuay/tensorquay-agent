//! GW-U-065: the decision evaluator (ADR-0014 point 9).
use super::{Ballot, Voter, outcome};
use tqa_types::{DecisionRule, Outcome, VoteChoice};

fn voter(id: &str, human: bool) -> Voter {
    Voter {
        key: (
            if human { "user" } else { "conversation" }.into(),
            id.into(),
        ),
        human,
    }
}

fn ballot(voter: &Voter, choice: VoteChoice, seq: u64) -> Ballot {
    Ballot {
        voter: voter.key.clone(),
        choice,
        seq,
    }
}

use VoteChoice::{Accept, Reject, Veto};

// TEST: GW-U-065
#[test]
fn gw_u_065_majority_decides_as_soon_as_more_than_half_agree() {
    let (a, b, c) = (voter("a", false), voter("b", false), voter("c", true));
    let voters = [a.clone(), b.clone(), c.clone()];
    let majority =
        |ballots: &[Ballot], open| outcome(DecisionRule::Majority, &voters, ballots, open);
    assert_eq!(
        majority(&[ballot(&a, Accept, 1)], true),
        None,
        "one of three"
    );
    assert_eq!(
        majority(&[ballot(&a, Accept, 1), ballot(&b, Accept, 2)], true),
        Some(Outcome::Accepted)
    );
    assert_eq!(
        majority(
            &[
                ballot(&a, Accept, 1),
                ballot(&b, Reject, 2),
                ballot(&c, Reject, 3)
            ],
            true
        ),
        Some(Outcome::Rejected)
    );
    // The latest vote counts: a changes its mind.
    assert_eq!(
        majority(
            &[
                ballot(&a, Accept, 1),
                ballot(&b, Reject, 2),
                ballot(&a, Reject, 3)
            ],
            true
        ),
        Some(Outcome::Rejected)
    );
    // The window closes with votes missing.
    assert_eq!(
        majority(&[ballot(&a, Accept, 1)], false),
        Some(Outcome::TimedOut)
    );
    assert_eq!(majority(&[], false), Some(Outcome::TimedOut));
}

// TEST: GW-U-065
#[test]
fn gw_u_065_a_tie_is_undecided_only_when_everyone_has_voted() {
    let voters = [
        voter("a", false),
        voter("b", false),
        voter("c", true),
        voter("d", true),
    ];
    let [a, b, c, d] = voters.clone();
    let majority = |ballots: &[Ballot]| outcome(DecisionRule::Majority, &voters, ballots, true);
    assert_eq!(
        majority(&[ballot(&a, Accept, 1), ballot(&b, Reject, 2)]),
        None,
        "two of four, tied"
    );
    assert_eq!(
        majority(&[
            ballot(&a, Accept, 1),
            ballot(&b, Reject, 2),
            ballot(&c, Accept, 3),
            ballot(&d, Reject, 4)
        ]),
        Some(Outcome::Undecided)
    );
}

// TEST: GW-U-065
#[test]
fn gw_u_065_under_human_only_a_person_decides() {
    let (agent, person) = (voter("a", false), voter("p", true));
    let voters = [agent.clone(), person.clone()];
    let human = |ballots: &[Ballot], open| outcome(DecisionRule::Human, &voters, ballots, open);
    assert_eq!(
        human(&[ballot(&agent, Accept, 1)], true),
        None,
        "an agent advises"
    );
    assert_eq!(
        human(
            &[ballot(&agent, Accept, 1), ballot(&person, Reject, 2)],
            true
        ),
        Some(Outcome::Rejected)
    );
    assert_eq!(
        human(&[ballot(&person, Accept, 1)], true),
        Some(Outcome::Accepted)
    );
    assert_eq!(
        human(&[ballot(&agent, Accept, 1)], false),
        Some(Outcome::TimedOut)
    );
}

// TEST: GW-U-065
#[test]
fn gw_u_065_a_persons_veto_ends_it_an_agents_does_not_and_non_members_dont_count() {
    let (agent, person, outsider) = (voter("a", false), voter("p", true), voter("x", true));
    let voters = [agent.clone(), person.clone()];
    for rule in [DecisionRule::Majority, DecisionRule::Human] {
        assert_eq!(
            outcome(rule, &voters, &[ballot(&person, Veto, 1)], true),
            Some(Outcome::Vetoed),
            "{rule:?}"
        );
        assert_eq!(
            outcome(rule, &voters, &[ballot(&agent, Veto, 1)], true),
            None,
            "{rule:?}: an agent's veto"
        );
        // Someone no longer a voting member: their votes are not counted.
        assert_eq!(
            outcome(rule, &voters, &[ballot(&outsider, Veto, 1)], true),
            None,
            "{rule:?}"
        );
    }
    assert_eq!(
        outcome(
            DecisionRule::Majority,
            &voters,
            &[ballot(&outsider, Accept, 1), ballot(&agent, Accept, 2)],
            true
        ),
        None,
        "one of two members"
    );
}
