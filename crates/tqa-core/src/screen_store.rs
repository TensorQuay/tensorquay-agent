//! Where a `computer` call's screenshots go for the person's record (ADR-0039 section 9): the conversation's screenshot
//! store, behind the gateway. The runner uploads each capture and journals its `screen.capture` only once the store
//! holds it, so an event never names a missing file. Trait only: `tqa-core` does no network I/O of its own; the
//! embedding runtime uploads over its link.
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::{fmt::Write, time::Duration};
use tqa_types::StoreRefusal;

/// The most a before capture waits for the store's answer: it is journaled ahead of the action's `tool.start`, and
/// must not stall the action.
pub const BEFORE_WAIT: Duration = Duration::from_secs(2);
/// The most an after capture, or the model's own screenshot, waits for the store's answer.
pub const AFTER_WAIT: Duration = Duration::from_secs(5);

/// Why a capture wasn't kept. Either way it is dropped: no event, and its phase reads false; the action goes on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Unstored {
    /// The store refused it.
    Refused(StoreRefusal),
    /// No answer within the wait, or the link couldn't carry the upload.
    NoAnswer,
}

/// The conversation's screenshot store, as the runtime reaches it.
#[async_trait]
pub trait ScreenStore: Send + Sync {
    /// Uploads `png` under `id`, its SHA-256 in lowercase hex, and waits at most `wait` for the store's answer:
    /// `Ok` once the store holds it. The runner also bounds the call by the run's cancellation, shutdown and wall
    /// clock.
    async fn store(&self, id: &str, png: &[u8], wait: Duration) -> Result<(), Unstored>;
}

/// A PNG's store id: its SHA-256 in lowercase hex, the same value as its `screen.capture`'s `sha256`.
#[must_use]
pub fn screenshot_id(png: &[u8]) -> String {
    Sha256::digest(png)
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}
