//! GW-U-130: the reconciler's decisions from one team's state (ADR-0034 points 9-11).
use super::{
    Action, MemberView, Run, StopReason, TeamState, TeamView, Thresholds, Told,
    deadline_warning_secs, decide,
};
use tqa_types::NoticeKind;

const MIN: i64 = 60;
const NOW: i64 = 1_000_000;
const LIMITS: Thresholds = Thresholds {
    member_idle: 10 * MIN,
    member_stuck: 15 * MIN,
    idle_stop: 30 * MIN,
};

fn team(members: Vec<MemberView>) -> TeamView {
    TeamView {
        state: TeamState::Running,
        deadline_at: NOW + 60 * MIN,
        deadline_minutes: 120,
        deadline_notified: false,
        last_activity_at: NOW,
        any_run_active: true,
        members,
    }
}

fn member(name: &str, run: Run) -> MemberView {
    MemberView {
        name: name.into(),
        state: "working".into(),
        run,
        reported_since_assignment: false,
        waiting_for_it: false,
        told: Told::default(),
    }
}

/// The first notice's sentence.
fn first_text(actions: &[Action]) -> Option<&str> {
    actions.iter().find_map(|action| match action {
        Action::Notice { text, .. } => Some(text.as_str()),
        _ => None,
    })
}

fn kinds(actions: &[Action]) -> Vec<(NoticeKind, Option<String>)> {
    actions
        .iter()
        .filter_map(|action| match action {
            Action::Notice { kind, member, .. } => Some((*kind, member.clone())),
            _ => None,
        })
        .collect()
}

// TEST: GW-U-130
#[test]
fn a_member_ended_without_a_report_is_idle_at_its_threshold_not_before() {
    let quiet = |secs| team(vec![member("researcher", Run::Ended { at: NOW - secs })]);
    assert!(decide(&quiet(10 * MIN - 1), NOW, &LIMITS).is_empty());
    let actions = decide(&quiet(10 * MIN), NOW, &LIMITS);
    assert_eq!(
        kinds(&actions),
        [(NoticeKind::MemberIdle, Some("researcher".into()))]
    );
    assert_eq!(
        first_text(&actions),
        Some("researcher stopped 10 minutes ago without reporting.")
    );
}

// TEST: GW-U-130
#[test]
fn a_member_that_reported_or_has_work_waiting_or_was_told_already_is_not_idle() {
    let ended = Run::Ended { at: NOW - 20 * MIN };
    for change in [
        (|m: &mut MemberView| m.reported_since_assignment = true) as fn(&mut MemberView),
        |m| m.waiting_for_it = true,
        |m| m.told.idle = true,
    ] {
        let mut one = member("researcher", ended.clone());
        change(&mut one);
        assert!(decide(&team(vec![one]), NOW, &LIMITS).is_empty());
    }
}

// TEST: GW-U-130
#[test]
fn an_active_run_without_progress_is_stuck_at_its_threshold_marked_and_told_once() {
    let quiet = |secs| {
        member(
            "coder",
            Run::Active {
                last_progress_at: NOW - secs,
            },
        )
    };
    assert!(decide(&team(vec![quiet(15 * MIN - 1)]), NOW, &LIMITS).is_empty());
    let actions = decide(&team(vec![quiet(15 * MIN)]), NOW, &LIMITS);
    assert_eq!(actions[0], Action::MarkStuck("coder".into()));
    assert_eq!(
        kinds(&actions),
        [(NoticeKind::MemberStuck, Some("coder".into()))]
    );
    let mut told = quiet(40 * MIN);
    told.state = "stuck".into();
    told.told.stuck = true;
    assert!(
        decide(&team(vec![told]), NOW, &LIMITS).is_empty(),
        "once per episode"
    );
}

// TEST: GW-U-130
#[test]
fn a_stuck_member_that_makes_progress_is_working_again() {
    let mut back = member(
        "coder",
        Run::Active {
            last_progress_at: NOW - MIN,
        },
    );
    back.state = "stuck".into();
    assert_eq!(
        decide(&team(vec![back]), NOW, &LIMITS),
        [Action::MarkWorking("coder".into())]
    );
}

// TEST: GW-U-130
#[test]
fn a_failed_run_is_told_once_with_its_code_and_reason() {
    let failed = Run::Failed {
        reason: "the run's step budget was spent".into(),
    };
    let actions = decide(&team(vec![member("tester", failed.clone())]), NOW, &LIMITS);
    assert_eq!(
        kinds(&actions),
        [(NoticeKind::MemberFailed, Some("tester".into()))]
    );
    assert_eq!(
        first_text(&actions),
        Some("tester failed: the run's step budget was spent.")
    );
    let mut told = member("tester", failed);
    told.told.failed = true;
    assert!(decide(&team(vec![told]), NOW, &LIMITS).is_empty());
}

// TEST: GW-U-130
#[test]
fn stopped_and_done_members_are_left_alone() {
    for state in ["stopped", "done"] {
        let mut gone = member("researcher", Run::Ended { at: NOW - 60 * MIN });
        gone.state = state.into();
        assert!(
            decide(&team(vec![gone]), NOW, &LIMITS).is_empty(),
            "{state}"
        );
    }
}

// TEST: GW-U-130
#[test]
fn the_deadline_warning_is_ten_minutes_or_a_fifth_of_a_short_deadline() {
    // The lead's decision of 2 Oct 2026: 60 minutes warns at 10 left, 20 at 4, 5 at 1.
    assert_eq!(deadline_warning_secs(120), 10 * MIN);
    assert_eq!(deadline_warning_secs(60), 10 * MIN);
    assert_eq!(deadline_warning_secs(20), 4 * MIN);
    assert_eq!(deadline_warning_secs(5), MIN);
    let near = |left| TeamView {
        deadline_at: NOW + left,
        ..team(Vec::new())
    };
    assert!(decide(&near(10 * MIN + 1), NOW, &LIMITS).is_empty());
    let actions = decide(&near(10 * MIN), NOW, &LIMITS);
    assert_eq!(kinds(&actions), [(NoticeKind::DeadlineNear, None)]);
    assert_eq!(
        first_text(&actions),
        Some("The team's time is nearly up: 10 min left.")
    );
    let told = TeamView {
        deadline_notified: true,
        ..near(5 * MIN)
    };
    assert!(decide(&told, NOW, &LIMITS).is_empty(), "once");
}

// TEST: GW-U-130
#[test]
fn the_deadline_stops_the_team_before_anything_else() {
    let late = TeamView {
        deadline_at: NOW,
        ..team(vec![member(
            "researcher",
            Run::Ended { at: NOW - 60 * MIN },
        )])
    };
    assert_eq!(
        decide(&late, NOW, &LIMITS),
        [Action::StartStop(StopReason::Deadline)]
    );
}

// TEST: GW-U-130
#[test]
fn the_idle_stop_needs_no_active_run_and_its_full_quiet_time() {
    let idle = |active, quiet| TeamView {
        any_run_active: active,
        last_activity_at: NOW - quiet,
        ..team(Vec::new())
    };
    assert!(decide(&idle(false, 30 * MIN - 1), NOW, &LIMITS).is_empty());
    assert!(
        decide(&idle(true, 60 * MIN), NOW, &LIMITS).is_empty(),
        "a run is active"
    );
    assert_eq!(
        decide(&idle(false, 30 * MIN), NOW, &LIMITS),
        [Action::StartStop(StopReason::Idle)]
    );
}

// TEST: GW-U-130
#[test]
fn notices_due_when_the_team_idles_out_are_sent_before_the_stop() {
    // E2E-AGENTS-029 on the pre-16d build: with both limits at a minute, a helper's quiet end and the team's idle stop
    // fall due in the same pass; the lead must still be told, before the team stops. (The deadline still stops the team
    // before anything else.)
    let minute = Thresholds {
        member_idle: MIN,
        member_stuck: 15 * MIN,
        idle_stop: MIN,
    };
    let idle = TeamView {
        any_run_active: false,
        last_activity_at: NOW - MIN,
        ..team(vec![
            member("quiet", Run::Ended { at: NOW - MIN }),
            member(
                "ghost",
                Run::Failed {
                    reason: "its sandbox couldn't start".into(),
                },
            ),
        ])
    };
    let actions = decide(&idle, NOW, &minute);
    assert_eq!(
        kinds(&actions),
        [
            (NoticeKind::MemberIdle, Some("quiet".into())),
            (NoticeKind::MemberFailed, Some("ghost".into())),
        ]
    );
    assert_eq!(actions.last(), Some(&Action::StartStop(StopReason::Idle)));
    assert_eq!(actions.len(), 3, "the notices and the stop, nothing else");
}

// TEST: GW-U-130
#[test]
fn a_stopping_team_is_finished_and_an_ended_one_left_alone() {
    let mut stopping = team(vec![member(
        "coder",
        Run::Active {
            last_progress_at: NOW - 60 * MIN,
        },
    )]);
    stopping.state = TeamState::Stopping;
    assert_eq!(decide(&stopping, NOW, &LIMITS), [Action::FinishStop]);
    stopping.state = TeamState::Ended;
    assert!(decide(&stopping, NOW, &LIMITS).is_empty());
}

// TEST: GW-U-130
#[test]
fn every_member_is_judged_on_its_own_in_one_pass() {
    let actions = decide(
        &team(vec![
            member("researcher", Run::Ended { at: NOW - 11 * MIN }),
            member(
                "coder",
                Run::Active {
                    last_progress_at: NOW - 16 * MIN,
                },
            ),
            member(
                "tester",
                Run::Active {
                    last_progress_at: NOW - MIN,
                },
            ),
        ]),
        NOW,
        &LIMITS,
    );
    assert_eq!(
        kinds(&actions),
        [
            (NoticeKind::MemberIdle, Some("researcher".into())),
            (NoticeKind::MemberStuck, Some("coder".into())),
        ]
    );
}

// TEST: GW-U-130
#[test]
fn a_failed_member_is_neither_stuck_nor_idle_but_its_failed_run_is_told() {
    let mut given_up = member(
        "researcher",
        Run::Active {
            last_progress_at: NOW - 40 * MIN,
        },
    );
    given_up.state = "failed".into();
    given_up.told.failed = true;
    assert!(
        decide(&team(vec![given_up]), NOW, &LIMITS).is_empty(),
        "a sandbox given up: no stuck notice after the failed one"
    );
    let mut ended = member("researcher", Run::Ended { at: NOW - 60 * MIN });
    ended.state = "failed".into();
    assert!(
        decide(&team(vec![ended]), NOW, &LIMITS).is_empty(),
        "nor idle"
    );
    let mut failed = member(
        "researcher",
        Run::Failed {
            reason: "the model provider kept failing".into(),
        },
    );
    failed.state = "failed".into();
    assert_eq!(
        kinds(&decide(&team(vec![failed]), NOW, &LIMITS)),
        [(NoticeKind::MemberFailed, Some("researcher".into()))]
    );
}
