//! A prolonged link outage uses bounded redials while the model response completes locally.
#[path = "support/fixture.rs"]
mod support;
use std::{sync::Mutex, time::Duration};
use support::*;
use tqa_testkit::{Script, Scripted};
use tqa_types::{Event, ServerFrame};

#[tokio::test]
async fn rt_e_006_two_minutes_offline_back_off_once_warn_and_replay_in_order() -> TestResult {
    let f = Fixture::new(vec![Script::new(vec![
        Scripted::Text("partial".into()),
        Scripted::Delay(Duration::from_secs(1)),
        Scripted::Text("answer".into()),
    ])])
    .await?;
    let log_path = f.directory.path().join("warnings");
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_max_level(tracing::Level::WARN)
        .with_writer(Mutex::new(std::fs::File::create(&log_path)?))
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(user()).await?;
    f.wait_event("text.delta").await?;
    let before = f.gateway.stored();
    f.gateway.set_link_available(false);
    let remaining = offline(&f).await?;
    assert_eq!(f.gateway.registrations(), 1);
    assert_eq!(f.gateway.stored(), before);
    let wal = std::fs::read_to_string(f.directory.path().join("state/events.jsonl"))?;
    assert!(
        wal.contains("run.completed"),
        "the run must complete without a gateway link"
    );
    assert_eq!(f.gateway.model_requests().len(), 1);
    f.gateway.set_link_available(true);
    f.clock.advance(remaining)?;
    f.gateway.wait_for_registrations(2).await?;
    f.wait_event("run.completed").await?;
    let stored = f.gateway.stored();
    let expected: Vec<ServerFrame> = wal
        .lines()
        .map(|line| {
            let record: serde_json::Value = serde_json::from_str(line)?;
            serde_json::from_value(record["frame"].clone())
        })
        .collect::<Result<_, serde_json::Error>>()?;
    assert_eq!(stored, expected);
    assert!(matches!(
        stored.last(),
        Some(ServerFrame::Event {
            event: Event::RunCompleted { .. },
            ..
        })
    ));
    stop(shutdown, task).await?;
    assert_eq!(
        std::fs::read_to_string(log_path)?
            .lines()
            .filter(|line| line.contains("runtime link disconnected"))
            .count(),
        1
    );
    f.gateway.shutdown().await?;
    Ok(())
}

async fn offline(f: &Fixture) -> Result<Duration, Box<dyn std::error::Error>> {
    let mut elapsed = Duration::ZERO;
    let mut delay = Duration::from_millis(125);
    let outage = Duration::from_mins(2);
    while elapsed < outage {
        wait_for(|| {
            f.sleeper.pending_durations().contains(&delay)
                && !f
                    .sleeper
                    .pending_durations()
                    .contains(&Duration::from_secs(5))
        })
        .await?;
        let advance = delay.min(outage.checked_sub(elapsed).ok_or("outage exceeded")?);
        let requests = f.gateway.requests().len();
        f.clock.advance(advance)?;
        elapsed += advance;
        if advance < delay {
            return Ok(delay.checked_sub(advance).ok_or("backoff exceeded")?);
        }
        wait_for(|| f.gateway.requests().len() > requests).await?;
        delay = delay.saturating_mul(2).min(Duration::from_millis(2500));
    }
    Ok(Duration::ZERO)
}
