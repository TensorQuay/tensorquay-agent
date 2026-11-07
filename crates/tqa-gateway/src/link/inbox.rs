//! A runtime link's inbound frames while the journal is slow (GW-I-127). The frame being journaled is a future the
//! link's loop polls next to the socket, so the loop keeps reading: it answers pings and records pongs at once, and
//! the runtime's probe of the link (a ping after a failed model call) gets its pong however long the append takes
//! within its budget. Data frames read meanwhile wait here, in order, and are received one after another once it
//! finishes; the end of the link (a Close, the socket's end or a read error) waits behind them, so a frame in flight,
//! such as a run's last event, is never dropped by the Close that follows it. A heartbeat's workspace lease renewal
//! is a store write too, so it runs in the same future, never in the loop.
//!
//! Limit: past [`HELD_FRAMES`] frames or [`HELD_BYTES`] bytes waiting, the loop stops reading until the append
//! finishes, so a ping behind that many frames waits with them, as every frame did before.
//!
//! Batches (GW-U-116): the frames held while an append was in flight are journaled together when they are plain
//! journal frames, the run of them at the front of the queue in one transaction, up to [`BATCH_FRAMES`] frames and
//! [`BATCH_BYTES`] bytes. Nothing waits to fill a batch: it is whatever queued during the last commit, so a lone frame
//! is received as before. A run lifecycle frame, a frame that isn't a journal entry and the link's end go on their own.
use super::{Received, StoreBounds};
use crate::{GatewayError, app::State};
use std::{collections::VecDeque, future::Future, pin::Pin};
use tqa_types::ConversationId;

/// The most data frames that wait behind a pending append: a whole batch's worth.
pub(super) const HELD_FRAMES: usize = 256;
/// The most bytes of data frames that wait behind a pending append.
pub(super) const HELD_BYTES: usize = 1024 * 1024;
/// The most frames journaled in one transaction.
pub(super) const BATCH_FRAMES: usize = 256;
/// The most bytes journaled in one transaction, each frame counted at its full size (a spilled one's blob included).
pub(super) const BATCH_BYTES: usize = 1024 * 1024;

type Pending<'a> = Pin<Box<dyn Future<Output = Outcome> + Send + 'a>>;

/// A received frame's outcome.
pub(super) struct Outcome {
    pub received: Result<Received, GatewayError>,
    /// When it was received, before any lease renewal.
    pub at: tokio::time::Instant,
    /// For a heartbeat, its workspace lease renewal's result.
    pub renewed: Option<Result<(), GatewayError>>,
}

pub(super) enum Held {
    Frame(Vec<u8>),
    /// How the link ends once everything before it was received.
    End(Result<(), GatewayError>),
}

/// What the inbox receives next from its queue.
pub(super) enum Next {
    /// One frame, received as it always was.
    Frame(Vec<u8>),
    /// Two or more consecutive plain journal frames, journaled in one transaction.
    Batch(Vec<tqa_types::ServerFrame>),
    /// The link's end, once everything before it was received.
    End(Result<(), GatewayError>),
}

/// Takes the next thing to receive from the front of `held`, with the bytes of the frames it took.
pub(super) fn next(held: &mut VecDeque<Held>) -> Option<(Next, usize)> {
    let bytes = match held.pop_front()? {
        Held::End(end) => return Some((Next::End(end), 0)),
        Held::Frame(bytes) => bytes,
    };
    let Some(first) = super::plain_journal(&bytes) else {
        let taken = bytes.len();
        return Some((Next::Frame(bytes), taken));
    };
    let (mut batch, mut taken) = (vec![first], bytes.len());
    while batch.len() < BATCH_FRAMES {
        let Some(Held::Frame(following)) = held.front() else {
            break;
        };
        if taken.saturating_add(following.len()) > BATCH_BYTES {
            break;
        }
        let Some(frame) = super::plain_journal(following) else {
            break;
        };
        taken = taken.saturating_add(following.len());
        held.pop_front();
        batch.push(frame);
    }
    if batch.len() == 1 {
        return Some((Next::Frame(bytes), taken));
    }
    Some((Next::Batch(batch), taken))
}

pub(super) struct Inbox<'a> {
    state: &'a State,
    conversation: &'a ConversationId,
    pending: Option<Pending<'a>>,
    held: VecDeque<Held>,
    held_bytes: usize,
    ended: bool,
}

impl<'a> Inbox<'a> {
    pub fn new(state: &'a State, conversation: &'a ConversationId) -> Self {
        Self {
            state,
            conversation,
            pending: None,
            held: VecDeque::new(),
            held_bytes: 0,
            ended: false,
        }
    }

    /// Whether a frame is being received.
    pub fn receiving(&self) -> bool {
        self.pending.is_some()
    }

    /// Whether the loop may read the socket: not after the link's end, and not past the cap.
    pub fn reading(&self) -> bool {
        !self.ended && self.held.len() < HELD_FRAMES && self.held_bytes < HELD_BYTES
    }

    /// The pending frame's outcome; never resolves while none is pending.
    pub async fn received(&mut self) -> Outcome {
        let Some(pending) = self.pending.as_mut() else {
            return std::future::pending().await;
        };
        let result = pending.await;
        self.pending = None;
        result
    }

    /// A data frame read from the socket: received now, or after those before it.
    pub fn frame(&mut self, bytes: Vec<u8>) {
        if self.pending.is_none() && self.held.is_empty() {
            self.start(bytes);
        } else {
            self.held_bytes = self.held_bytes.saturating_add(bytes.len());
            self.held.push_back(Held::Frame(bytes));
        }
    }

    /// The link's end as read from the socket: `Some` to end it now, `None` when frames before it are still to be
    /// received; [`Self::advance`] returns it once they are.
    pub fn end(&mut self, end: Result<(), GatewayError>) -> Option<Result<(), GatewayError>> {
        self.ended = true;
        if self.pending.is_none() && self.held.is_empty() {
            return Some(end);
        }
        self.held.push_back(Held::End(end));
        None
    }

    /// After a frame was received: starts the next one waiting, or returns the link's end once it is next.
    pub fn advance(&mut self) -> Option<Result<(), GatewayError>> {
        while self.pending.is_none() {
            let (next, taken) = next(&mut self.held)?;
            self.held_bytes = self.held_bytes.saturating_sub(taken);
            match next {
                Next::Frame(bytes) => self.start(bytes),
                Next::Batch(frames) => self.start_batch(frames),
                Next::End(end) => return Some(end),
            }
        }
        None
    }

    fn start(&mut self, bytes: Vec<u8>) {
        let (state, conversation) = (self.state, self.conversation);
        // `receive` bounds its journal append itself (`StoreBounds`); the link's cancellation drops it early.
        let bounds = StoreBounds::new(state.config.io_timeout());
        self.pending = Some(Box::pin(async move {
            let received = super::receive(&state.events, conversation, &bytes, bounds).await;
            let at = tokio::time::Instant::now();
            let renewed = match &received {
                Ok(Received::Heartbeat(_)) => Some(renew(state, conversation).await),
                _ => None,
            };
            Outcome {
                received,
                at,
                renewed,
            }
        }));
    }
}

impl Inbox<'_> {
    fn start_batch(&mut self, frames: Vec<tqa_types::ServerFrame>) {
        let (state, conversation) = (self.state, self.conversation);
        let bounds = StoreBounds::new(state.config.io_timeout());
        self.pending = Some(Box::pin(async move {
            let received = super::receive_batch(&state.events, conversation, frames, bounds).await;
            Outcome {
                received,
                at: tokio::time::Instant::now(),
                renewed: None,
            }
        }));
    }
}

/// A running holder keeps its workspace lease alive (066b), bounded by `io_timeout`.
async fn renew(state: &State, conversation: &ConversationId) -> Result<(), GatewayError> {
    let id = crate::conversations::uuid(conversation)?;
    tokio::time::timeout(
        state.config.io_timeout(),
        crate::manager::lease::renew(&state.pool, id, state.config.workspaces.lease_ttl),
    )
    .await
    .map_err(|_| GatewayError::Timeout)?
}
