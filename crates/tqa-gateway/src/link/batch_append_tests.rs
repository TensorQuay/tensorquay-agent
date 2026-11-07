//! GW-U-117: a batch on a slow or uncertain journal. An attempt that times out with nothing committed is retried with
//! the batch halved, down to one frame, so a store slow only for big batches keeps the link where single frames would
//! have kept it; a store slow at every size still closes it within the budget, as before. A batch whose commit is
//! uncertain is recovered only when every stored frame equals the batch's, and a batch with a gap anywhere is refused
//! whole.
use super::{FrameSink, StoreBounds, receive_batch};
use crate::GatewayError;
use async_trait::async_trait;
use std::{sync::Mutex, time::Duration};
use tqa_types::{ConversationId, ErrorCode, ServerFrame};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const IO_TIMEOUT: Duration = Duration::from_secs(10);
const BOUNDS: StoreBounds = StoreBounds::new(IO_TIMEOUT);

/// How the scripted journal treats an attempt.
#[derive(Clone, Copy)]
enum Speed {
    /// Batches of more than this many frames stall past an attempt's bound and commit nothing; smaller ones commit.
    SlowAbove(usize),
    /// Every attempt stalls past its bound and commits nothing.
    AlwaysSlow,
    /// The first attempt commits and then never answers (its response is lost); later ones answer at once.
    LoseFirstAnswer,
    /// The first attempt never answers and commits nothing; later ones answer at once.
    StallFirst,
}

/// An atomic journal: a batch is stored whole or not at all, and a gap or duplicate refuses it whole.
struct Journal {
    speed: Speed,
    stored: Mutex<Vec<ServerFrame>>,
    attempts: Mutex<Vec<usize>>,
    published: Mutex<Vec<ServerFrame>>,
}

impl Journal {
    fn new(speed: Speed, stored: Vec<ServerFrame>) -> Self {
        Self {
            speed,
            stored: Mutex::new(stored),
            attempts: Mutex::new(vec![]),
            published: Mutex::new(vec![]),
        }
    }

    /// Stores the batch whole or refuses it whole; it is published only once its attempt answers.
    fn store(&self, frames: &[ServerFrame]) -> Result<(), GatewayError> {
        let mut stored = self.stored.lock().map_err(|_| GatewayError::Io)?;
        let mut checker = tqa_types::SeqChecker::new((stored.len() as u64).into());
        for frame in frames {
            checker.accept(crate::wire::sequence(frame).ok_or(GatewayError::Io)?)?;
        }
        stored.extend_from_slice(frames);
        Ok(())
    }

    fn commit(&self, frames: &[ServerFrame]) -> Result<(), GatewayError> {
        self.store(frames)?;
        self.published
            .lock()
            .map_err(|_| GatewayError::Io)?
            .extend_from_slice(frames);
        Ok(())
    }

    async fn attempt(&self, frames: &[ServerFrame]) -> Result<(), GatewayError> {
        let first = {
            let mut attempts = self.attempts.lock().map_err(|_| GatewayError::Io)?;
            attempts.push(frames.len());
            attempts.len() == 1
        };
        let stall = match self.speed {
            Speed::SlowAbove(size) => frames.len() > size,
            Speed::AlwaysSlow => true,
            Speed::LoseFirstAnswer => {
                if first {
                    self.store(frames)?;
                }
                first
            }
            Speed::StallFirst => first,
        };
        if stall {
            std::future::pending::<()>().await;
        }
        self.commit(frames)
    }

    fn stored(&self) -> Result<Vec<ServerFrame>, Box<dyn std::error::Error>> {
        Ok(self.stored.lock().map_err(|_| "stored")?.clone())
    }

    fn attempts(&self) -> Result<Vec<usize>, Box<dyn std::error::Error>> {
        Ok(self.attempts.lock().map_err(|_| "attempts")?.clone())
    }

    fn published(&self) -> Result<Vec<ServerFrame>, Box<dyn std::error::Error>> {
        Ok(self.published.lock().map_err(|_| "published")?.clone())
    }
}

#[async_trait]
impl FrameSink for Journal {
    async fn append(&self, _: &ConversationId, frame: ServerFrame) -> Result<(), GatewayError> {
        self.attempt(std::slice::from_ref(&frame)).await
    }

    async fn append_batch(
        &self,
        _: &ConversationId,
        frames: &[ServerFrame],
    ) -> Result<(), GatewayError> {
        self.attempt(frames).await
    }

    async fn recover_committed(
        &self,
        conversation: &ConversationId,
        frame: &ServerFrame,
    ) -> Result<bool, GatewayError> {
        self.recover_committed_batch(conversation, std::slice::from_ref(frame))
            .await
    }

    async fn recover_committed_batch(
        &self,
        _: &ConversationId,
        frames: &[ServerFrame],
    ) -> Result<bool, GatewayError> {
        let stored = self.stored.lock().map_err(|_| GatewayError::Io)?.clone();
        let matches = frames.iter().all(|frame| {
            crate::wire::sequence(frame)
                .and_then(|seq| seq.to_string().parse::<usize>().ok())
                .and_then(|seq| stored.get(seq - 1))
                == Some(frame)
        });
        if matches {
            self.published
                .lock()
                .map_err(|_| GatewayError::Io)?
                .extend_from_slice(frames);
        }
        Ok(matches)
    }

    fn control(&self, _: &ConversationId, _: ServerFrame) -> Result<(), GatewayError> {
        Ok(())
    }
}

fn delta(seq: u64, text: &str) -> ServerFrame {
    ServerFrame::Event {
        seq: seq.into(),
        run_id: None,
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event: tqa_types::Event::TextDelta {
            text: text.to_owned(),
        },
    }
}

fn deltas(range: std::ops::RangeInclusive<u64>) -> Vec<ServerFrame> {
    range.map(|seq| delta(seq, "x")).collect()
}

async fn send(journal: &Journal, frames: Vec<ServerFrame>) -> Result<(), GatewayError> {
    receive_batch(
        journal,
        &"batched".parse().map_err(|_| GatewayError::Io)?,
        frames,
        BOUNDS,
    )
    .await
    .map(|_| ())
}

// TEST: GW-U-117
#[tokio::test(start_paused = true)]
async fn gw_u_117_a_store_slow_for_big_batches_gets_halved_ones_and_keeps_the_link() -> TestResult {
    let journal = Journal::new(Speed::SlowAbove(64), vec![]);
    send(&journal, deltas(1..=256)).await?;
    assert_eq!(journal.attempts()?, [256, 128, 64, 64, 64, 64]);
    assert_eq!(
        journal.stored()?,
        deltas(1..=256),
        "every frame once, in order"
    );
    assert_eq!(journal.published()?, deltas(1..=256));
    Ok(())
}

// TEST: GW-U-117
#[tokio::test(start_paused = true)]
async fn gw_u_117_a_store_slow_at_every_size_still_closes_the_link_within_the_budget() -> TestResult
{
    let journal = Journal::new(Speed::AlwaysSlow, vec![]);
    let started = tokio::time::Instant::now();
    assert_eq!(
        send(&journal, deltas(1..=8)).await,
        Err(GatewayError::StoreUnavailable)
    );
    assert!(
        started.elapsed() <= BOUNDS.budget,
        "{:?}",
        started.elapsed()
    );
    assert_eq!(journal.attempts()?.first(), Some(&8));
    assert!(journal.stored()?.is_empty() && journal.published()?.is_empty());
    Ok(())
}

// TEST: GW-U-117
#[tokio::test(start_paused = true)]
async fn gw_u_117_a_batch_whose_answer_was_lost_is_recovered_once_and_published_once() -> TestResult
{
    let journal = Journal::new(Speed::LoseFirstAnswer, vec![]);
    send(&journal, deltas(1..=4)).await?;
    // The whole batch committed on the first attempt; the halved retries find it stored and recover it.
    assert_eq!(journal.attempts()?, [4, 2, 2]);
    assert_eq!(journal.stored()?, deltas(1..=4));
    assert_eq!(
        journal.published()?,
        deltas(1..=4),
        "each frame published once"
    );
    Ok(())
}

// TEST: GW-U-117
#[tokio::test(start_paused = true)]
async fn gw_u_117_an_uncertain_batch_that_does_not_match_the_stored_frames_fails_closed()
-> TestResult {
    // The first attempt times out, so the retry is uncertain; it finds frames 1-2 stored, but not this batch's.
    let journal = Journal::new(
        Speed::StallFirst,
        vec![delta(1, "other"), delta(2, "other")],
    );
    assert_eq!(
        send(&journal, deltas(1..=2)).await,
        Err(GatewayError::Protocol(ErrorCode::SeqGap))
    );
    assert_eq!(journal.attempts()?, [2, 1]);
    assert_eq!(journal.stored()?, [delta(1, "other"), delta(2, "other")]);
    assert!(
        journal.published()?.is_empty(),
        "nothing of this batch was published"
    );
    Ok(())
}

// TEST: GW-U-117
#[tokio::test(start_paused = true)]
async fn gw_u_117_a_gap_anywhere_in_a_batch_refuses_all_of_it() -> TestResult {
    let journal = Journal::new(Speed::SlowAbove(usize::MAX), deltas(1..=4));
    let batch = vec![delta(5, "x"), delta(6, "x"), delta(8, "x")];
    assert_eq!(
        send(&journal, batch).await,
        Err(GatewayError::Protocol(ErrorCode::SeqGap))
    );
    assert_eq!(
        journal.stored()?,
        deltas(1..=4),
        "neither 5 nor 6 was stored"
    );
    assert!(journal.published()?.is_empty());
    Ok(())
}
