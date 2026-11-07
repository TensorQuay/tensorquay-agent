//! A hundred thousand offline events outlive the bounded notification and socket replay buffers.
#[path = "support/fixture.rs"]
mod support;
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use support::*;
use tqa_core::{EventContext, EventSink};
use tqa_runtime::{Journal, JournalOptions, Runtime, RuntimeLimits};
use tqa_types::{Event, SandboxState, Seq, ServerFrame};

const EVENTS: usize = 100_000;

#[tokio::test]
async fn rt_i_011_offline_events_coalesce_and_replay_in_bounded_pages_without_loss() -> TestResult {
    let f = Fixture::new(vec![]).await?;
    f.gateway.set_frame_limit(EVENTS + 10)?;
    let expected = offline_log(&f).await?;
    assert_eq!(expected.len(), EVENTS);
    assert_eq!(f.gateway.registrations(), 0);
    f.gateway.set_bound(true);
    let deps = f.deps()?;
    let page_peak = deps.testing.replay_page_peak.clone();
    let runtime = Runtime::new(deps)?;
    let shutdown = tokio_util::sync::CancellationToken::new();
    let task = tokio::spawn(runtime.run(shutdown.clone()));
    tokio::time::timeout(Duration::from_mins(2), async {
        while f.gateway.frame_count() < EVENTS {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    assert_eq!(f.gateway.stored(), expected);
    assert_eq!(
        f.gateway.frames(),
        expected,
        "socket replay may neither duplicate nor omit frames"
    );
    let peak = page_peak.load(Ordering::SeqCst);
    assert!(
        peak > 0 && peak <= RuntimeLimits::RECOMMENDED.queue_capacity,
        "socket replay held {peak} frames at once"
    );
    stop(shutdown, task).await?;
    f.gateway.shutdown().await?;
    Ok(())
}

async fn offline_log(f: &Fixture) -> Result<Vec<ServerFrame>, Box<dyn std::error::Error>> {
    let mut session = Journal::open(JournalOptions {
        directory: f.directory.path().join("state"),
        wall_clock: Arc::new(FixedWall),
        sleeper: Arc::new(tqa_llm::TokioSleeper),
        limits: RuntimeLimits::RECOMMENDED,
        workspace_sync: None,
        testing: tqa_runtime::testing::TestingHooks::default(),
    })
    .await?;
    session.journal.register(Seq::ZERO).await?;
    assert_eq!(
        session.frames.max_capacity(),
        RuntimeLimits::RECOMMENDED.queue_capacity
    );
    let drain = tokio::spawn(async move {
        let mut count = 0_u64;
        while let Some(frame) = session.frames.recv().await {
            count += 1;
            assert!(matches!(frame, ServerFrame::Event { seq, .. } if seq == Seq::from(count)));
        }
        count
    });
    for _ in 0..EVENTS {
        session
            .journal
            .emit(
                Event::SandboxState {
                    state: SandboxState::Idle,
                },
                EventContext::default(),
            )
            .await?;
    }
    assert!(session.changes.has_changed()?);
    assert!(
        matches!(session.changes.borrow_and_update().as_deref(), Some(ServerFrame::Event { seq, .. })
        if *seq == Seq::from(EVENTS as u64))
    );
    assert!(
        !session.changes.has_changed()?,
        "the notification retains one latest frame, not a backlog"
    );
    session.journal.shutdown().await?;
    assert_eq!(drain.await?, EVENTS as u64);
    let (wal, recovery) = tqa_context::Wal::open(&f.directory.path().join("state"))?;
    assert!(!recovery.truncated_tail);
    drop(wal);
    Ok(recovery
        .records
        .into_iter()
        .map(|(_, frame)| frame)
        .collect())
}
