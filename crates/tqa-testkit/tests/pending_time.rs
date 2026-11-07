//! Pending waits are a deterministic barrier for transport tests that advance fake time.
use std::{sync::Arc, time::Duration};
use tqa_testkit::{FakeClock, FakeSleeper};
use tqa_types::Sleeper as _;
#[tokio::test]
async fn cancelled_sleep_disappears_from_pending_waits() {
    let sleeper = Arc::new(FakeSleeper::new(FakeClock::new()));
    let task_sleeper = sleeper.clone();
    let task = tokio::spawn(async move {
        task_sleeper.sleep(Duration::from_secs(5)).await;
    });
    tokio::task::yield_now().await;
    assert_eq!(sleeper.pending_durations(), vec![Duration::from_secs(5)]);
    task.abort();
    assert!(task.await.is_err());
    assert!(sleeper.pending_durations().is_empty());
}
