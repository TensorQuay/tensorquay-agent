//! CORE-U-077: the suites' `drive` bounds a run by fake time and by real time together. A run waiting on a real thread
//! (a tool result's cap on the blocking pool, say) finishes when that thread does, however few microseconds its polls
//! take; a run stuck on fake time still ends, with the harness's message (TEST-PLAN §19a, CORE-U-059).
mod support;
use std::time::Duration;
use support::*;
use tqa_testkit::FakeClock;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn core_u_077_a_run_waiting_on_a_slow_real_thread_finishes() -> TestResult {
    let clock = FakeClock::new();
    // Longer than the poll budget lasts in real time when every poll only waits for the thread.
    let work = tokio::task::spawn_blocking(|| {
        std::thread::sleep(Duration::from_millis(300));
        7
    });
    assert_eq!(drive(work, &clock).await??, 7);
    Ok(())
}

#[tokio::test]
async fn core_u_077_a_run_stuck_on_fake_time_still_ends() -> TestResult {
    let clock = FakeClock::new();
    let started = std::time::Instant::now();
    let stuck = drive_within(
        std::future::pending::<()>(),
        &clock,
        100,
        Duration::from_millis(200),
    )
    .await;
    let error = stuck.err().ok_or("a run that never ends must not finish")?;
    assert_eq!(
        error.to_string(),
        "script did not finish within its bounded fake time"
    );
    // It ends only after the real floor has passed too.
    assert!(started.elapsed() >= Duration::from_millis(200));
    Ok(())
}

#[tokio::test]
async fn core_u_077_a_quick_run_takes_no_real_wait() -> TestResult {
    let clock = FakeClock::new();
    let started = std::time::Instant::now();
    assert_eq!(drive(async { 3 }, &clock).await?, 3);
    // A floor that became a wait would take all of it (DRIVE_REAL_FLOOR, 10 s); 2 s tells the two apart under any load.
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the floor is a bound, never a wait"
    );
    assert!(DRIVE_REAL_FLOOR >= Duration::from_secs(10));
    Ok(())
}
