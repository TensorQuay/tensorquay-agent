//! GW-U-221: the rounds state table (ADR-0034's rounds amendment, A1 and A3), every cell: the team's state, the
//! helper a name matches, whether the run may open a round, and the seats.
use super::{Call, Helper, HelperState, Outcome, Start, Team, start_member};

fn helper(name: &str, state: HelperState) -> Helper<'_> {
    Helper {
        name,
        state,
        failed_run: false,
        gone: false,
    }
}

fn call(role: &str) -> Call<'_> {
    Call {
        role,
        may_open_round: true,
        seats: (0, 3),
    }
}

const RUNNING: Team = Team::Running { round: 1 };
const ENDED: Team = Team::Ended {
    round: 1,
    for_good: false,
};

fn new(name: &str) -> Start {
    Start::New(name.to_owned())
}
fn back(name: &str) -> Start {
    Start::BringBack(name.to_owned())
}

// TEST: GW-U-221
#[test]
fn gw_u_221_a_first_helper_opens_round_one_and_a_running_round_takes_more() {
    assert_eq!(
        start_member(Team::None, &[], call("researcher")),
        Outcome::OpenRound {
            round: 1,
            start: new("researcher")
        }
    );
    assert_eq!(
        start_member(
            RUNNING,
            &[helper("writer", HelperState::Working)],
            call("researcher")
        ),
        Outcome::AddToRound(new("researcher"))
    );
    assert_eq!(
        start_member(Team::Stopping { round: 1 }, &[], call("researcher")),
        Outcome::Refused("team_stopped")
    );
}

// TEST: GW-U-221
#[test]
fn gw_u_221_a_name_brings_back_a_finished_helper_in_any_letter_case() {
    for state in [
        HelperState::Done,
        HelperState::Stopped,
        HelperState::Waiting,
    ] {
        let helpers = [helper("researcher", state)];
        assert_eq!(
            start_member(RUNNING, &helpers, call("Researcher")),
            Outcome::AddToRound(back("researcher")),
            "{state:?}"
        );
    }
    let failed_run = [Helper {
        failed_run: true,
        ..helper("researcher", HelperState::Failed)
    }];
    assert_eq!(
        start_member(RUNNING, &failed_run, call("researcher")),
        Outcome::AddToRound(back("researcher"))
    );
}

// TEST: GW-U-221
#[test]
fn gw_u_221_a_live_or_gone_helper_by_that_name_means_a_new_one() {
    for state in [
        HelperState::Starting,
        HelperState::Queued,
        HelperState::Working,
        HelperState::Stuck,
    ] {
        let helpers = [helper("researcher", state)];
        assert_eq!(
            start_member(RUNNING, &helpers, call("researcher")),
            Outcome::AddToRound(new("researcher-2")),
            "{state:?}"
        );
    }
    // Its start failed (no run), or its task was deleted.
    let failed = [helper("researcher", HelperState::Failed)];
    let gone = [Helper {
        gone: true,
        ..helper("researcher", HelperState::Done)
    }];
    for helpers in [&failed[..], &gone[..]] {
        assert_eq!(
            start_member(RUNNING, helpers, call("researcher")),
            Outcome::AddToRound(new("researcher-2"))
        );
    }
}

// TEST: GW-U-221
#[test]
fn gw_u_221_a_team_ended_for_a_rounds_reason_opens_the_next_round_on_a_persons_run_only() {
    let helpers = [helper("researcher", HelperState::Done)];
    assert_eq!(
        start_member(ENDED, &helpers, call("researcher")),
        Outcome::OpenRound {
            round: 2,
            start: back("researcher")
        }
    );
    assert_eq!(
        start_member(
            Team::Ended {
                round: 3,
                for_good: false
            },
            &helpers,
            call("reviewer")
        ),
        Outcome::OpenRound {
            round: 4,
            start: new("reviewer")
        }
    );
    // A wake, the lead's own continuation, or a run that already opened a round (A1, at review).
    assert_eq!(
        start_member(
            ENDED,
            &helpers,
            Call {
                may_open_round: false,
                ..call("researcher")
            }
        ),
        Outcome::Refused("round_needs_person")
    );
}

// TEST: GW-U-221
#[test]
fn gw_u_221_a_team_ended_for_good_never_opens_a_round() {
    for may_open_round in [true, false] {
        assert_eq!(
            start_member(
                Team::Ended {
                    round: 2,
                    for_good: true
                },
                &[helper("researcher", HelperState::Done)],
                Call {
                    may_open_round,
                    ..call("researcher")
                }
            ),
            Outcome::Refused("team_ended")
        );
    }
}

// TEST: GW-U-221
#[test]
fn gw_u_221_seats_are_per_round() {
    let full = Call {
        seats: (3, 3),
        ..call("researcher")
    };
    assert_eq!(
        start_member(RUNNING, &[], full),
        Outcome::Refused("member_limit")
    );
    let helpers = [helper("researcher", HelperState::Done)];
    assert_eq!(
        start_member(RUNNING, &helpers, full),
        Outcome::Refused("member_limit")
    );
    // Earlier rounds' helpers hold no seat: a new round starts with none taken.
    assert_eq!(
        start_member(ENDED, &helpers, full),
        Outcome::OpenRound {
            round: 2,
            start: back("researcher")
        }
    );
}
