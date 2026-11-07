//! GW-E-020: a failed append never consumes the runtime's unacknowledged WAL tail.
use super::{FrameSink, StoreBounds, receive};
use crate::GatewayError;
use async_trait::async_trait;
use std::{sync::Mutex, time::Duration};
use tokio::time::Instant;
use tqa_types::{ConversationId, ServerFrame};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// The shipped bounds at the default `io_timeout` (10 s).
const BOUNDS: StoreBounds = StoreBounds::new(Duration::from_secs(10));

struct Store {
    available_at: Instant,
    frames: Mutex<Vec<ServerFrame>>,
    ambiguous_commit: bool,
}

#[async_trait]
impl FrameSink for Store {
    async fn recover_committed(
        &self,
        _: &ConversationId,
        frame: &ServerFrame,
    ) -> Result<bool, GatewayError> {
        Ok(self.frames.lock().map_err(|_| GatewayError::Io)?.last() == Some(frame))
    }
    async fn append(&self, _: &ConversationId, frame: ServerFrame) -> Result<(), GatewayError> {
        if Instant::now() < self.available_at {
            return Err(GatewayError::StoreUnavailable);
        }
        let mut frames = self.frames.lock().map_err(|_| GatewayError::Io)?;
        tqa_types::SeqChecker::new((frames.len() as u64).into())
            .accept(crate::wire::sequence(&frame).ok_or(GatewayError::Io)?)?;
        frames.push(frame);
        if self.ambiguous_commit {
            Err(GatewayError::StoreUnavailable)
        } else {
            Ok(())
        }
    }

    fn control(&self, _: &ConversationId, _: ServerFrame) -> Result<(), GatewayError> {
        Err(GatewayError::Protocol(tqa_types::ErrorCode::Internal))
    }
}

fn frame(seq: u64) -> ServerFrame {
    ServerFrame::Event {
        seq: seq.into(),
        run_id: None,
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event: tqa_types::Event::TextDelta {
            text: format!("frame {seq}"),
        },
    }
}

#[tokio::test(start_paused = true)]
async fn gw_e_020_retry_recovers_the_head_within_the_budget() -> TestResult {
    let start = Instant::now();
    let store = Store {
        available_at: start + Duration::from_secs(4),
        frames: Mutex::new(vec![]),
        ambiguous_commit: false,
    };
    receive(
        &store,
        &"outage".parse()?,
        &serde_json::to_vec(&frame(1))?,
        BOUNDS,
    )
    .await?;
    assert!(start.elapsed() >= Duration::from_secs(4));
    assert!(start.elapsed() < Duration::from_secs(5));
    assert_eq!(
        *store.frames.lock().map_err(|_| "store lock")?,
        vec![frame(1)]
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn gw_e_020_a_one_minute_outage_closes_at_the_budget_and_replays_every_unacknowledged_frame()
-> TestResult {
    let start = Instant::now();
    let id = "outage".parse()?;
    let store = Store {
        available_at: start + Duration::from_mins(1),
        frames: Mutex::new(vec![]),
        ambiguous_commit: false,
    };
    let wal: Vec<_> = (1..=32).map(frame).collect();
    assert_eq!(
        receive(&store, &id, &serde_json::to_vec(&wal[0])?, BOUNDS).await,
        Err(GatewayError::StoreUnavailable)
    );
    assert_eq!(start.elapsed(), BOUNDS.budget);
    assert!(store.frames.lock().map_err(|_| "store lock")?.is_empty());
    tokio::time::advance(Duration::from_mins(1).saturating_sub(BOUNDS.budget)).await;
    for pending in &wal {
        receive(&store, &id, &serde_json::to_vec(pending)?, BOUNDS).await?;
    }
    assert_eq!(*store.frames.lock().map_err(|_| "store lock")?, wal);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn gw_e_020_a_commit_with_a_lost_response_is_recovered_without_a_duplicate() -> TestResult {
    let store = Store {
        available_at: Instant::now(),
        frames: Mutex::new(vec![]),
        ambiguous_commit: true,
    };
    receive(
        &store,
        &"outage".parse()?,
        &serde_json::to_vec(&frame(1))?,
        BOUNDS,
    )
    .await?;
    assert_eq!(
        *store.frames.lock().map_err(|_| "store lock")?,
        vec![frame(1)]
    );
    Ok(())
}
