//! GW-U-075: how long the link waits on the journal before it gives up on a runtime. One append attempt is bounded by
//! `io_timeout`, and all of a frame's attempts by three of them, so a store that is slow under host I/O pressure
//! (commits of several seconds) keeps its links, while a store that stays unavailable still closes them. A workspace
//! lease renewal that fails for the store is skipped while at least half the lease remains since this link's last
//! successful one.
use super::{FrameSink, StoreBounds, receive, renewals::Renewals};
use crate::GatewayError;
use async_trait::async_trait;
use std::{collections::VecDeque, sync::Mutex, time::Duration};
use tokio::time::Instant;
use tqa_types::{ConversationId, ServerFrame};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const IO_TIMEOUT: Duration = Duration::from_secs(10);
const BOUNDS: StoreBounds = StoreBounds::new(IO_TIMEOUT);

/// What one append attempt does.
#[derive(Clone, Copy)]
enum Attempt {
    /// Commits after this long, and answers.
    Commit(Duration),
    /// Commits at once, then never answers within any bound (the response is lost).
    CommitThenHang,
    /// Doesn't answer for this long, commits nothing.
    Stall(Duration),
}

/// A journal whose attempts follow a script; once it runs out, attempts commit at once.
struct Scripted {
    script: Mutex<VecDeque<Attempt>>,
    frames: Mutex<Vec<ServerFrame>>,
    attempts: Mutex<u32>,
}

impl Scripted {
    fn new(script: impl IntoIterator<Item = Attempt>) -> Self {
        Self {
            script: Mutex::new(script.into_iter().collect()),
            frames: Mutex::new(vec![]),
            attempts: Mutex::new(0),
        }
    }

    fn commit(&self, frame: ServerFrame) -> Result<(), GatewayError> {
        let mut frames = self.frames.lock().map_err(|_| GatewayError::Io)?;
        tqa_types::SeqChecker::new((frames.len() as u64).into())
            .accept(crate::wire::sequence(&frame).ok_or(GatewayError::Io)?)?;
        frames.push(frame);
        Ok(())
    }

    fn stored(&self) -> Result<Vec<ServerFrame>, Box<dyn std::error::Error>> {
        Ok(self.frames.lock().map_err(|_| "frames")?.clone())
    }

    fn attempts(&self) -> Result<u32, Box<dyn std::error::Error>> {
        Ok(*self.attempts.lock().map_err(|_| "attempts")?)
    }
}

#[async_trait]
impl FrameSink for Scripted {
    async fn append(&self, _: &ConversationId, frame: ServerFrame) -> Result<(), GatewayError> {
        *self.attempts.lock().map_err(|_| GatewayError::Io)? += 1;
        let next = self
            .script
            .lock()
            .map_err(|_| GatewayError::Io)?
            .pop_front();
        match next {
            None => self.commit(frame),
            Some(Attempt::Commit(after)) => {
                tokio::time::sleep(after).await;
                self.commit(frame)
            }
            Some(Attempt::CommitThenHang) => {
                self.commit(frame)?;
                std::future::pending().await
            }
            Some(Attempt::Stall(after)) => {
                tokio::time::sleep(after).await;
                Err(GatewayError::Timeout)
            }
        }
    }

    async fn recover_committed(
        &self,
        _: &ConversationId,
        frame: &ServerFrame,
    ) -> Result<bool, GatewayError> {
        Ok(self.frames.lock().map_err(|_| GatewayError::Io)?.last() == Some(frame))
    }

    fn control(&self, _: &ConversationId, _: ServerFrame) -> Result<(), GatewayError> {
        Ok(())
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

async fn send(store: &Scripted, seq: u64) -> Result<(), GatewayError> {
    let bytes = serde_json::to_vec(&frame(seq)).map_err(|_| GatewayError::Io)?;
    receive(
        store,
        &"slow".parse().map_err(|_| GatewayError::Io)?,
        &bytes,
        BOUNDS,
    )
    .await
    .map(|_| ())
}

// TEST: GW-U-075
#[test]
fn gw_u_075_an_attempt_is_bounded_by_io_timeout_and_a_frame_by_three() {
    assert_eq!(
        BOUNDS,
        StoreBounds {
            attempt: IO_TIMEOUT,
            budget: IO_TIMEOUT * 3
        }
    );
}

// TEST: GW-U-075
#[tokio::test(start_paused = true)]
async fn gw_u_075_a_commit_slower_than_five_seconds_keeps_the_link() -> TestResult {
    // 7 s is the slowest statement the 00:33 load stall logged; 5 s used to close every link.
    let store = Scripted::new([Attempt::Commit(Duration::from_secs(7))]);
    let start = Instant::now();
    send(&store, 1).await?;
    assert_eq!(start.elapsed(), Duration::from_secs(7));
    assert_eq!((store.stored()?, store.attempts()?), (vec![frame(1)], 1));
    Ok(())
}

// TEST: GW-U-075
#[tokio::test(start_paused = true)]
async fn gw_u_075_an_attempt_past_its_bound_is_retried_and_its_lost_commit_recovered() -> TestResult
{
    let store = Scripted::new([Attempt::CommitThenHang]);
    let start = Instant::now();
    send(&store, 1).await?;
    assert!(start.elapsed() >= IO_TIMEOUT && start.elapsed() < IO_TIMEOUT + Duration::from_secs(1));
    assert_eq!(store.stored()?, vec![frame(1)], "stored once, not twice");
    assert_eq!(store.attempts()?, 2);
    Ok(())
}

// TEST: GW-U-075
#[tokio::test(start_paused = true)]
async fn gw_u_075_two_slow_attempts_then_a_commit_keep_the_link() -> TestResult {
    let store = Scripted::new([
        Attempt::Stall(Duration::from_secs(9)),
        Attempt::Stall(Duration::from_secs(9)),
        Attempt::Commit(Duration::from_secs(8)),
    ]);
    send(&store, 1).await?;
    assert_eq!((store.stored()?, store.attempts()?), (vec![frame(1)], 3));
    Ok(())
}

// TEST: GW-U-075
#[tokio::test(start_paused = true)]
async fn gw_u_075_a_store_that_stays_slow_closes_the_link_when_the_budget_is_spent() -> TestResult {
    let store = Scripted::new([Attempt::Stall(Duration::from_mins(10)); 8]);
    let start = Instant::now();
    assert_eq!(send(&store, 1).await, Err(GatewayError::StoreUnavailable));
    assert_eq!(
        start.elapsed(),
        IO_TIMEOUT * 3,
        "not before the budget, and not after it"
    );
    assert!(store.stored()?.is_empty());
    assert_eq!(store.attempts()?, 3, "each attempt ends at its own bound");
    Ok(())
}

// TEST: GW-U-075
#[tokio::test(start_paused = true)]
async fn gw_u_075_a_failed_lease_renewal_is_skipped_while_half_the_lease_remains() -> TestResult {
    let ttl = Duration::from_secs(90);
    let heartbeat = Duration::from_secs(30);
    let start = Instant::now();
    let mut renewals = Renewals::default();
    // Before a renewal on this link succeeded, nothing is known about the lease: a failure closes the link.
    assert_eq!(
        renewals.outcome(start, ttl, Err(GatewayError::Timeout)),
        Err(GatewayError::Timeout)
    );
    let mut renewals = Renewals::default();
    renewals.outcome(start, ttl, Ok(()))?;
    // 30 s after the last renewal, 60 s of the lease remain: skipped, for the store and for the I/O bound alike.
    assert_eq!(
        renewals.outcome(start + heartbeat, ttl, Err(GatewayError::Timeout)),
        Ok(())
    );
    // 60 s after it, only 30 s remain: the link closes before the lease can lapse under it.
    assert_eq!(
        renewals.outcome(
            start + heartbeat * 2,
            ttl,
            Err(GatewayError::StoreUnavailable)
        ),
        Err(GatewayError::StoreUnavailable)
    );
    // A success counts from its own time again.
    let mut renewals = Renewals::default();
    renewals.outcome(start, ttl, Ok(()))?;
    renewals.outcome(start + heartbeat, ttl, Err(GatewayError::Timeout))?;
    renewals.outcome(start + heartbeat * 2, ttl, Ok(()))?;
    assert_eq!(
        renewals.outcome(start + heartbeat * 3, ttl, Err(GatewayError::Timeout)),
        Ok(())
    );
    // A failure that isn't the store's is never skipped.
    assert_eq!(
        renewals.outcome(start + heartbeat * 3, ttl, Err(GatewayError::Cancelled)),
        Err(GatewayError::Cancelled)
    );
    Ok(())
}
