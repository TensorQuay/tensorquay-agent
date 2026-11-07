//! The Docker fixture's cleanup, without an engine: removing a fixture's volume survives a container the engine created
//! late (a create that timed out on the client can still complete, and hold the volume after the containers were
//! listed), and a failed setup reports its own error before a cleanup error. SBX-U-013: a cleanup step the engine
//! doesn't answer in time is tried again, and every step still runs after an earlier one failed (candidate 13g's
//! overloaded coverage run left 22 gateway stand-ins behind when the first timed-out call ended the cleanup).
#[allow(
    dead_code,
    reason = "This binary tests the cleanup helpers only; the rest of the shared fixture is for the Docker suites."
)]
#[path = "support/docker.rs"]
mod support;

use std::{
    cell::Cell,
    time::{Duration, Instant},
};

use support::{TestResult, every_step, retry_elapsed, retry_in_use, setup_failed, step};

fn engine(status_code: u16) -> Box<dyn std::error::Error> {
    Box::new(bollard::errors::Error::DockerResponseServerError {
        status_code,
        message: "remove tqa-test-1-2-0: volume is in use - [f459]".into(),
    })
}

/// Runs the helper with a volume removal answering `answers` in turn, counting the container sweeps.
async fn run(answers: &[Option<u16>]) -> (TestResult, usize, usize) {
    let (sweeps, removals) = (Cell::new(0), Cell::new(0));
    let result = retry_in_use(
        4,
        Duration::from_millis(1),
        || {
            sweeps.set(sweeps.get() + 1);
            async { Ok(()) }
        },
        || {
            let answer = answers.get(removals.get()).copied().flatten();
            removals.set(removals.get() + 1);
            async move { answer.map_or(Ok(()), |status| Err(engine(status))) }
        },
    )
    .await;
    (result, sweeps.get(), removals.get())
}

#[tokio::test]
async fn a_volume_removed_at_once_needs_no_second_sweep() {
    let (result, sweeps, removals) = run(&[None]).await;
    assert!(result.is_ok());
    assert_eq!((sweeps, removals), (0, 1));
}

#[tokio::test]
async fn a_volume_held_by_a_late_container_is_swept_again_and_removed() {
    let (result, sweeps, removals) = run(&[Some(409), Some(409), None]).await;
    assert!(result.is_ok(), "{result:?}");
    assert_eq!((sweeps, removals), (2, 3));
}

#[tokio::test]
async fn a_volume_already_gone_is_removed() {
    let (result, _, removals) = run(&[Some(404)]).await;
    assert!(result.is_ok());
    assert_eq!(removals, 1);
}

#[tokio::test]
async fn a_volume_still_held_after_every_attempt_names_the_attempts() {
    let started = Instant::now();
    let (result, sweeps, removals) = run(&[Some(409); 4]).await;
    let message = result
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(
        message.contains("still in use after 4 attempts"),
        "{message}"
    );
    assert!(message.contains("volume is in use"), "{message}");
    assert_eq!((sweeps, removals), (3, 4));
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn another_engine_error_is_not_retried() {
    let (result, sweeps, removals) = run(&[Some(500), None]).await;
    assert!(result.is_err());
    assert_eq!((sweeps, removals), (0, 1));
}

#[test]
fn a_failed_setup_reports_its_own_error_first() {
    let error = setup_failed("Elapsed(())".into(), Err(engine(409)));
    let message = error.to_string();
    assert!(message.starts_with("Elapsed(())"), "{message}");
    assert!(
        message.contains("cleanup also failed") && message.contains("volume is in use"),
        "{message}"
    );
    assert_eq!(
        setup_failed("Elapsed(())".into(), Ok(())).to_string(),
        "Elapsed(())"
    );
}

/// A real bound that elapsed: what `bounded` returns when the engine doesn't answer in time.
async fn timed_out() -> Box<dyn std::error::Error> {
    Box::new(
        tokio::time::timeout(Duration::ZERO, std::future::pending::<()>())
            .await
            .err()
            .unwrap_or_else(unreachable_elapsed),
    )
}
#[allow(
    clippy::panic,
    reason = "a zero timeout on a pending future always elapses"
)]
fn unreachable_elapsed() -> tokio::time::error::Elapsed {
    panic!("a zero timeout on a pending future elapsed")
}

/// Runs `retry_elapsed` with a step answering `answers` in turn (`true`: it times out), counting the tries.
async fn tries(answers: &[bool]) -> (TestResult, usize) {
    let calls = Cell::new(0);
    let result = retry_elapsed(3, Duration::from_millis(1), || {
        let late = answers.get(calls.get()).copied().unwrap_or(false);
        calls.set(calls.get() + 1);
        async move { if late { Err(timed_out().await) } else { Ok(()) } }
    })
    .await;
    (result, calls.get())
}

// TEST: SBX-U-013
#[tokio::test]
async fn sbx_u_013_a_step_that_times_out_is_tried_again() {
    let (result, calls) = tries(&[true, true, false]).await;
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(calls, 3);
}

// TEST: SBX-U-013
#[tokio::test]
async fn sbx_u_013_a_step_that_keeps_timing_out_names_its_attempts() {
    let (result, calls) = tries(&[true; 5]).await;
    let message = result
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(
        message.contains("deadline has elapsed") && message.contains("3 attempts"),
        "{message}"
    );
    assert_eq!(calls, 3);
}

// TEST: SBX-U-013
#[tokio::test]
async fn sbx_u_013_another_error_is_not_tried_again() {
    let calls = Cell::new(0);
    let result = retry_elapsed(3, Duration::from_millis(1), || {
        calls.set(calls.get() + 1);
        async { Err(engine(500)) }
    })
    .await;
    assert!(result.is_err());
    assert_eq!(calls.get(), 1);
}

// TEST: SBX-U-013
#[tokio::test]
async fn sbx_u_013_every_step_runs_after_one_times_out_and_the_failures_are_named() {
    // The 13g shape: the first step (listing the containers) never answers in time. The steps after it still run,
    // and the error names the step that failed.
    let ran = Cell::new(Vec::<&str>::new());
    let mark = |name: &'static str| {
        let mut seen = ran.take();
        seen.push(name);
        ran.set(seen);
    };
    let result = every_step(
        3,
        Duration::from_millis(1),
        vec![
            step("containers", || {
                mark("containers");
                async { Err(timed_out().await) }
            }),
            step("networks", || {
                mark("networks");
                async { Ok(()) }
            }),
            step("volume", || {
                mark("volume");
                async { Err(engine(500)) }
            }),
            step("late containers", || {
                mark("late containers");
                async { Ok(()) }
            }),
        ],
    )
    .await;
    let message = result
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(
        message.contains("containers: deadline has elapsed after 3 attempts"),
        "{message}"
    );
    assert!(message.contains("volume: "), "{message}");
    assert!(!message.contains("networks"), "{message}");
    // The timed-out step tried three times, then every later step ran once.
    assert_eq!(
        ran.take(),
        [
            "containers",
            "containers",
            "containers",
            "networks",
            "volume",
            "late containers"
        ]
    );
}
