//! TK-U-024: `--concurrency` scheduling. At most N attempts run at once and start in task order; an attempt is numbered
//! when it starts and a `harness.provider` rerun goes first, so N=1 numbers as before; with N>1 a harness failure stops
//! new starts; the token limit stops new starts once reached.
//! TK-U-033 (E2E-DECISION-008): arms interleaved per task, reruns keeping their arm, and the engine-call and
//! wall-time caps stopping new starts for both arms.
use super::schedule::{Job, Schedule, Skip};
use crate::e2e::classify::Class;
use crate::e2e::result::Arm;

/// Starts everything that may start now, as `(number, task, round, rerun_of)`.
fn start_all(schedule: &mut Schedule) -> Vec<(u32, usize, u32, Option<u32>)> {
    let mut started = Vec::new();
    while let Some(next) = schedule.start() {
        started.push((
            next.number,
            next.job.task,
            next.job.round,
            next.job.rerun_of,
        ));
    }
    started
}

#[test]
fn tk_u_024_at_most_n_run_at_once_in_task_order() {
    let mut schedule = Schedule::new(3, 2, 4, None);
    assert_eq!(
        start_all(&mut schedule),
        vec![
            (1, 0, 1, None),
            (2, 0, 2, None),
            (3, 1, 1, None),
            (4, 1, 2, None)
        ],
        "four at once, in task and round order"
    );
    schedule.finish(
        1,
        &Job {
            task: 0,
            round: 1,
            rerun_of: None,
            arm: Arm::Model,
            position: 1,
        },
        Class::Pass,
        10,
    );
    assert_eq!(
        start_all(&mut schedule),
        vec![(5, 2, 1, None)],
        "one finished, one more starts"
    );
    for (number, task, round) in [(2, 0, 2), (3, 1, 1), (4, 1, 2), (5, 2, 1)] {
        schedule.finish(
            number,
            &Job {
                task,
                round,
                rerun_of: None,
                arm: Arm::Model,
                position: 1,
            },
            Class::Pass,
            10,
        );
    }
    assert_eq!(start_all(&mut schedule), vec![(6, 2, 2, None)]);
    schedule.finish(
        6,
        &Job {
            task: 2,
            round: 2,
            rerun_of: None,
            arm: Arm::Model,
            position: 1,
        },
        Class::Pass,
        10,
    );
    assert!(schedule.is_done());
    assert!(schedule.skipped().is_empty());
}

#[test]
fn tk_u_024_n1_numbers_a_provider_rerun_right_after_its_attempt() {
    let mut schedule = Schedule::new(2, 1, 1, None);
    assert_eq!(
        start_all(&mut schedule),
        vec![(1, 0, 1, None)],
        "one at a time"
    );
    schedule.finish(
        1,
        &Job {
            task: 0,
            round: 1,
            rerun_of: None,
            arm: Arm::Model,
            position: 1,
        },
        Class::HarnessProvider,
        5,
    );
    assert_eq!(
        start_all(&mut schedule),
        vec![(2, 0, 1, Some(1))],
        "the rerun first, numbered next"
    );
    schedule.finish(
        2,
        &Job {
            task: 0,
            round: 1,
            rerun_of: Some(1),
            arm: Arm::Model,
            position: 1,
        },
        Class::Pass,
        5,
    );
    assert_eq!(start_all(&mut schedule), vec![(3, 1, 1, None)]);
    schedule.finish(
        3,
        &Job {
            task: 1,
            round: 1,
            rerun_of: None,
            arm: Arm::Model,
            position: 1,
        },
        Class::Pass,
        5,
    );
    assert!(schedule.is_done());
}

#[test]
fn tk_u_024_with_n_above_1_a_harness_failure_stops_new_starts() {
    let mut schedule = Schedule::new(4, 1, 2, None);
    assert_eq!(
        start_all(&mut schedule),
        vec![(1, 0, 1, None), (2, 1, 1, None)]
    );
    // A first provider failure is not a pause: it gets its rerun.
    schedule.finish(
        1,
        &Job {
            task: 0,
            round: 1,
            rerun_of: None,
            arm: Arm::Model,
            position: 1,
        },
        Class::HarnessProvider,
        1,
    );
    assert_eq!(start_all(&mut schedule), vec![(3, 0, 1, Some(1))]);
    // Another harness failure pauses: nothing new starts, the running attempt finishes.
    schedule.finish(
        2,
        &Job {
            task: 1,
            round: 1,
            rerun_of: None,
            arm: Arm::Model,
            position: 1,
        },
        Class::HarnessCheck,
        1,
    );
    assert_eq!(start_all(&mut schedule), vec![], "paused");
    assert_eq!(schedule.paused_by(), Some(2));
    assert!(!schedule.is_done(), "attempt 3 still runs");
    schedule.finish(
        3,
        &Job {
            task: 0,
            round: 1,
            rerun_of: Some(1),
            arm: Arm::Model,
            position: 1,
        },
        Class::Pass,
        1,
    );
    assert!(schedule.is_done());
    let skipped: Vec<_> = schedule
        .skipped()
        .iter()
        .map(|(job, skip)| (job.task, *skip))
        .collect();
    assert_eq!(
        skipped,
        vec![(2, Skip::Paused { by: 2 }), (3, Skip::Paused { by: 2 })]
    );
}

#[test]
fn tk_u_024_at_n1_a_harness_failure_does_not_pause() {
    let mut schedule = Schedule::new(2, 1, 1, None);
    start_all(&mut schedule);
    schedule.finish(
        1,
        &Job {
            task: 0,
            round: 1,
            rerun_of: None,
            arm: Arm::Model,
            position: 1,
        },
        Class::HarnessCheck,
        1,
    );
    assert_eq!(
        start_all(&mut schedule),
        vec![(2, 1, 1, None)],
        "N=1 goes on, as before"
    );
    assert_eq!(schedule.paused_by(), None);
}

#[test]
fn tk_u_024_a_failed_provider_rerun_pauses() {
    let mut schedule = Schedule::new(3, 1, 2, None);
    start_all(&mut schedule);
    schedule.finish(
        1,
        &Job {
            task: 0,
            round: 1,
            rerun_of: None,
            arm: Arm::Model,
            position: 1,
        },
        Class::HarnessProvider,
        1,
    );
    let rerun = start_all(&mut schedule);
    assert_eq!(rerun, vec![(3, 0, 1, Some(1))]);
    schedule.finish(
        3,
        &Job {
            task: 0,
            round: 1,
            rerun_of: Some(1),
            arm: Arm::Model,
            position: 1,
        },
        Class::HarnessProvider,
        1,
    );
    assert_eq!(schedule.paused_by(), Some(3));
}

#[test]
fn tk_u_024_the_token_limit_stops_new_starts_counting_every_finished_attempt() {
    let mut schedule = Schedule::new(4, 1, 2, Some(100));
    assert_eq!(
        start_all(&mut schedule),
        vec![(1, 0, 1, None), (2, 1, 1, None)]
    );
    schedule.finish(
        1,
        &Job {
            task: 0,
            round: 1,
            rerun_of: None,
            arm: Arm::Model,
            position: 1,
        },
        Class::Pass,
        60,
    );
    assert_eq!(
        start_all(&mut schedule),
        vec![(3, 2, 1, None)],
        "60 < 100: go on"
    );
    schedule.finish(
        2,
        &Job {
            task: 1,
            round: 1,
            rerun_of: None,
            arm: Arm::Model,
            position: 1,
        },
        Class::Pass,
        50,
    );
    // 110 >= 100: the rest are skipped for the budget; attempt 3 still finishes.
    assert_eq!(start_all(&mut schedule), vec![]);
    schedule.finish(
        3,
        &Job {
            task: 2,
            round: 1,
            rerun_of: None,
            arm: Arm::Model,
            position: 1,
        },
        Class::Pass,
        5,
    );
    assert!(schedule.is_done());
    let skipped: Vec<_> = schedule
        .skipped()
        .iter()
        .map(|(job, skip)| (job.task, *skip))
        .collect();
    assert_eq!(skipped, vec![(3, Skip::Budget)]);
}

/// Starts everything that may start at `elapsed`, as `(number, task, round, arm)`.
fn start_arms(
    schedule: &mut Schedule,
    elapsed: std::time::Duration,
) -> Vec<(u32, usize, u32, Arm)> {
    let mut started = Vec::new();
    while let Some(next) = schedule.start_at(elapsed) {
        started.push((next.number, next.job.task, next.job.round, next.job.arm));
    }
    started
}

// TEST: TK-U-033
#[test]
fn tk_u_033_each_task_runs_its_rounds_once_per_arm_interleaved_and_a_rerun_keeps_its_arm() {
    let mut schedule = Schedule::new(2, 2, 16, None).with_arms(&[Arm::Model, Arm::Engine]);
    let started = start_arms(&mut schedule, std::time::Duration::ZERO);
    assert_eq!(
        started
            .iter()
            .map(|(_, task, round, arm)| (*task, *round, *arm))
            .collect::<Vec<_>>(),
        vec![
            (0, 1, Arm::Model),
            (0, 1, Arm::Engine),
            (0, 2, Arm::Engine),
            (0, 2, Arm::Model),
            (1, 1, Arm::Model),
            (1, 1, Arm::Engine),
            (1, 2, Arm::Engine),
            (1, 2, Arm::Model),
        ],
        "paired per task and round, ABBA: the order alternates per round, so neither arm always runs second on a warm cache"
    );
    let job = Job {
        task: 0,
        round: 1,
        rerun_of: None,
        arm: Arm::Engine,
        position: 2,
    };
    schedule.finish(2, &job, Class::HarnessProvider, 0);
    let mut rerun = Vec::new();
    while let Some(next) = schedule.start_at(std::time::Duration::ZERO) {
        rerun.push((
            next.number,
            next.job.task,
            next.job.round,
            next.job.arm,
            next.job.position,
        ));
    }
    assert_eq!(
        rerun,
        vec![(9, 0, 1, Arm::Engine, 2)],
        "the rerun keeps its arm and its place in the pair"
    );
    // Without arms the order is exactly today's, all model.
    let mut plain = Schedule::new(1, 2, 16, None);
    assert_eq!(
        start_arms(&mut plain, std::time::Duration::ZERO),
        vec![(1, 0, 1, Arm::Model), (2, 0, 2, Arm::Model)]
    );
}

// TEST: TK-U-033
#[test]
fn tk_u_033_each_job_records_its_place_in_the_pair() {
    let mut schedule = Schedule::new(1, 3, 16, None).with_arms(&[Arm::Model, Arm::Engine]);
    let mut places = Vec::new();
    while let Some(next) = schedule.start_at(std::time::Duration::ZERO) {
        places.push((next.job.round, next.job.arm, next.job.position));
    }
    assert_eq!(
        places,
        vec![
            (1, Arm::Model, 1),
            (1, Arm::Engine, 2),
            (2, Arm::Engine, 1),
            (2, Arm::Model, 2),
            (3, Arm::Model, 1),
            (3, Arm::Engine, 2),
        ]
    );
    // One arm only: every job is first (and alone) in its round.
    let mut plain = Schedule::new(1, 2, 16, None);
    let first = plain
        .start_at(std::time::Duration::ZERO)
        .map(|next| next.job.position);
    assert_eq!(first, Some(1));
}

// TEST: TK-U-033
#[test]
fn tk_u_033_the_engine_call_cap_and_the_wall_cap_stop_new_starts_for_both_arms() {
    let arms = [Arm::Model, Arm::Engine];
    let mut capped = Schedule::new(3, 1, 2, None)
        .with_arms(&arms)
        .engine_limit(Some(10));
    let first = start_arms(&mut capped, std::time::Duration::ZERO);
    assert_eq!(first.len(), 2, "task 0's pair");
    let engine_job = Job {
        task: 0,
        round: 1,
        rerun_of: None,
        arm: Arm::Engine,
        position: 2,
    };
    capped.finish(2, &engine_job, Class::Pass, 0);
    capped.record_engine_calls(12);
    assert!(
        start_arms(&mut capped, std::time::Duration::ZERO).is_empty(),
        "the cap is reached"
    );
    assert_eq!(
        capped
            .skipped()
            .iter()
            .map(|(job, skip)| (job.task, job.arm, *skip))
            .collect::<Vec<_>>(),
        vec![
            (1, Arm::Model, Skip::EngineBudget),
            (1, Arm::Engine, Skip::EngineBudget),
            (2, Arm::Model, Skip::EngineBudget),
            (2, Arm::Engine, Skip::EngineBudget),
        ]
    );
    let minute = std::time::Duration::from_mins(1);
    let mut timed = Schedule::new(2, 1, 1, None).wall_limit(Some(30 * minute));
    assert_eq!(
        start_arms(&mut timed, 29 * minute).len(),
        1,
        "before the cap"
    );
    let job = Job {
        task: 0,
        round: 1,
        rerun_of: None,
        arm: Arm::Model,
        position: 1,
    };
    timed.finish(1, &job, Class::Pass, 0);
    assert!(start_arms(&mut timed, 30 * minute).is_empty(), "at the cap");
    assert_eq!(
        timed
            .skipped()
            .iter()
            .map(|(_, skip)| *skip)
            .collect::<Vec<_>>(),
        vec![Skip::WallTime]
    );
}
