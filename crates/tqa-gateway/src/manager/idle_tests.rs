use super::Idle;
use std::time::Duration;
use tqa_types::{Event, ServerFrame};

#[tokio::test(start_paused = true)]
async fn gw_i_010_activity_is_lifecycle_only_and_terminal_restarts_the_full_window()
-> Result<(), Box<dyn std::error::Error>> {
    let window = Duration::from_mins(10);
    let mut idle = Idle::new(std::sync::Arc::new(crate::manager::SystemClock::new()));
    tokio::time::advance(window).await;
    assert!(idle.expired(window));
    let run_id: tqa_types::RunId = "run-clock".parse()?;
    idle.observe(&frame(Event::RunStarted {
        run_id: run_id.clone(),
    }))?;
    tokio::time::advance(window * 2).await;
    idle.observe(&ServerFrame::Heartbeat {
        run_id: run_id.clone(),
    })?;
    assert!(!idle.expired(window));
    idle.observe(&frame(Event::RunCompleted { run_id }))?;
    tokio::time::advance(
        window
            .checked_sub(Duration::from_secs(1))
            .ok_or("invalid window")?,
    )
    .await;
    assert!(!idle.expired(window));
    idle.observe(&frame(Event::TextDelta {
        text: "late text".into(),
    }))?;
    tokio::time::advance(Duration::from_secs(1)).await;
    assert!(idle.expired(window));
    idle.action(time::OffsetDateTime::UNIX_EPOCH);
    assert!(!idle.expired(window));
    Ok(())
}

fn frame(event: Event) -> ServerFrame {
    ServerFrame::Event {
        seq: 1.into(),
        run_id: None,
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event,
    }
}

fn fresh() -> Idle {
    Idle::new(std::sync::Arc::new(crate::manager::SystemClock::new()))
}

#[tokio::test(start_paused = true)]
async fn gw_i_066_an_idle_sandbox_is_retired_at_its_lifetime() {
    use super::Lifetime;
    let cap = Duration::from_hours(1);
    let mut idle = fresh();
    tokio::time::advance(cap.saturating_sub(Duration::from_secs(1))).await;
    assert_eq!(idle.lifetime(cap), Lifetime::Within);
    tokio::time::advance(Duration::from_secs(1)).await;
    assert_eq!(idle.lifetime(cap), Lifetime::Retire);
}

#[tokio::test(start_paused = true)]
async fn gw_i_066_a_busy_sandbox_gets_the_grace_then_is_retired_busy()
-> Result<(), Box<dyn std::error::Error>> {
    use super::{LIFETIME_GRACE, Lifetime};
    let cap = Duration::from_hours(1);
    let mut idle = fresh();
    idle.observe(&frame(Event::RunStarted {
        run_id: "run-busy".parse()?,
    }))?;
    tokio::time::advance(cap.saturating_sub(Duration::from_secs(1))).await;
    assert_eq!(idle.lifetime(cap), Lifetime::Within, "never before the cap");
    tokio::time::advance(Duration::from_secs(1)).await;
    assert_eq!(idle.lifetime(cap), Lifetime::Grace);
    tokio::time::advance(LIFETIME_GRACE.saturating_sub(Duration::from_secs(1))).await;
    assert_eq!(idle.lifetime(cap), Lifetime::Grace);
    tokio::time::advance(Duration::from_secs(1)).await;
    assert_eq!(idle.lifetime(cap), Lifetime::RetireBusy);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn gw_i_066_a_run_that_ends_within_the_grace_retires_as_idle()
-> Result<(), Box<dyn std::error::Error>> {
    use super::Lifetime;
    let cap = Duration::from_hours(1);
    let mut idle = fresh();
    let run_id: tqa_types::RunId = "run-ends".parse()?;
    idle.observe(&frame(Event::RunStarted {
        run_id: run_id.clone(),
    }))?;
    tokio::time::advance(cap + Duration::from_secs(30)).await;
    assert_eq!(idle.lifetime(cap), Lifetime::Grace);
    idle.observe(&frame(Event::RunCompleted { run_id }))?;
    assert_eq!(idle.lifetime(cap), Lifetime::Retire);
    Ok(())
}
