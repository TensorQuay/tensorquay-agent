//! GW-U-222: the role a run gets and why (ADR-0034's rounds amendment, A2 and A7): every answer and its reason, from
//! the facts the route reads.
use super::{Answer, Ended, Facts, Place, answer};

const IN_NO_TEAM: Facts = Facts {
    teams_on: true,
    place: Place::None,
    was_member: false,
    ended: None,
    wakeable: true,
};

fn says(role: &'static str, reason: Option<&'static str>) -> Answer {
    Answer { role, reason }
}

// TEST: GW-U-222
#[test]
fn gw_u_222_a_running_team_and_a_task_in_none_answer_as_before_without_a_reason() {
    assert_eq!(
        answer(Facts {
            place: Place::Lead,
            ..IN_NO_TEAM
        }),
        says("lead", None)
    );
    assert_eq!(
        answer(Facts {
            place: Place::Member,
            ..IN_NO_TEAM
        }),
        says("member", None)
    );
    assert_eq!(answer(IN_NO_TEAM), says("may_lead", None));
}

// TEST: GW-U-222
#[test]
fn gw_u_222_none_says_why() {
    assert_eq!(
        answer(Facts {
            teams_on: false,
            place: Place::Lead,
            ..IN_NO_TEAM
        }),
        says("none", Some("teams_disabled"))
    );
    assert_eq!(
        answer(Facts {
            was_member: true,
            ..IN_NO_TEAM
        }),
        says("none", Some("was_member"))
    );
    assert_eq!(
        answer(Facts {
            wakeable: false,
            ..IN_NO_TEAM
        }),
        says("none", Some("runtime_too_old"))
    );
    for note_unread in [false, true] {
        assert_eq!(
            answer(Facts {
                ended: Some(Ended {
                    for_good: true,
                    note_unread
                }),
                ..IN_NO_TEAM
            }),
            says("none", Some("team_ended")),
            "a team ended for good, its note unread: {note_unread}"
        );
    }
}

// TEST: GW-U-222
#[test]
fn gw_u_222_a_team_ended_for_a_rounds_reason_is_between_rounds_then_one_team_until_rounds_exist() {
    assert_eq!(
        answer(Facts {
            ended: Some(Ended {
                for_good: false,
                note_unread: true
            }),
            ..IN_NO_TEAM
        }),
        says("ended", Some("between_rounds"))
    );
    // Step 1 (A7): once the note is read, a task still leads one team; step 2 makes this `ended` with start_member.
    for wakeable in [true, false] {
        assert_eq!(
            answer(Facts {
                ended: Some(Ended {
                    for_good: false,
                    note_unread: false
                }),
                wakeable,
                ..IN_NO_TEAM
            }),
            says("none", Some("one_team"))
        );
    }
}

// TEST: GW-U-222
#[test]
fn gw_u_222_was_member_wins_over_an_old_runtime_and_teams_off_over_everything() {
    assert_eq!(
        answer(Facts {
            was_member: true,
            wakeable: false,
            ..IN_NO_TEAM
        }),
        says("none", Some("was_member"))
    );
    assert_eq!(
        answer(Facts {
            teams_on: false,
            was_member: true,
            ended: Some(Ended {
                for_good: true,
                note_unread: true
            }),
            ..IN_NO_TEAM
        }),
        says("none", Some("teams_disabled"))
    );
}
