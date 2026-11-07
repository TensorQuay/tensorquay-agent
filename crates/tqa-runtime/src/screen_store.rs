//! The conversation's screenshot store, over this runtime's link (ADR-0039 section 9). A capture goes as `screen.upload`
//! chunks of at most 96 KiB, base64, keyed by its SHA-256, and the gateway answers its last chunk with
//! `screen.stored`. One upload is in flight per sandbox, its chunks in order, each written before the next is sent.
//! The gateway starts an upload afresh at another id's chunk (or any chunk 0), so a chunk an upload left behind when its
//! wait ran out is waited for before the next upload sends anything: it can never cut into the next one. A late or
//! unknown answer is ignored.
use crate::{RuntimeError, link::Control, time::Time};
use async_trait::async_trait;
use base64::Engine as _;
use std::{collections::HashMap, sync::Mutex as StdMutex, time::Duration};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tqa_core::{ScreenStore, Unstored};
use tqa_types::{MAX_UPLOAD_CHUNK_BYTES, ServerFrame, StoreRefusal};

/// A chunk's write on the link: `Ok` once the link task has written it.
type Written = oneshot::Receiver<Result<(), RuntimeError>>;

/// The runtime's way to the screenshot store; shared by every run of the session.
pub(crate) struct LinkScreenStore {
    controls: mpsc::Sender<Control>,
    time: Time,
    /// The one upload in flight; between uploads, the write of a chunk one left behind, if any.
    flight: Mutex<Option<Written>>,
    /// The upload waiting for its answer, by id.
    answers: StdMutex<HashMap<String, oneshot::Sender<Result<(), StoreRefusal>>>>,
}

impl LinkScreenStore {
    pub fn new(controls: mpsc::Sender<Control>, time: Time) -> Self {
        Self {
            controls,
            time,
            flight: Mutex::new(None),
            answers: StdMutex::new(HashMap::new()),
        }
    }

    /// The gateway's `screen.stored`, for the upload waiting for it; one nobody waits for any more is ignored.
    pub fn answered(&self, id: &str, ok: bool, reason: Option<StoreRefusal>) {
        let waiting = self
            .answers
            .lock()
            .ok()
            .and_then(|mut answers| answers.remove(id));
        if let Some(waiting) = waiting {
            let answer = if ok {
                Ok(())
            } else {
                Err(reason.unwrap_or(StoreRefusal::StoreFailed))
            };
            let _ = waiting.send(answer);
        }
    }

    /// One upload, start to answer; dropped when its wait runs out, at any await.
    async fn upload(&self, id: &str, png: &[u8]) -> Result<(), Unstored> {
        let mut flight = self.flight.lock().await;
        // A chunk an earlier upload left behind is written first, so the gateway reads it before this upload's. Awaited
        // in place: if this upload's own wait runs out meanwhile, the next one still waits for it.
        if let Some(left) = flight.as_mut() {
            let _ = left.await;
        }
        *flight = None;
        let (answer, answered) = oneshot::channel();
        if let Ok(mut answers) = self.answers.lock() {
            answers.insert(id.to_owned(), answer);
        }
        // However this upload ends, a late answer for it finds nobody waiting.
        let _forget = Forget { store: self, id };
        let count = png.len().div_ceil(MAX_UPLOAD_CHUNK_BYTES);
        for (index, chunk) in png.chunks(MAX_UPLOAD_CHUNK_BYTES).enumerate() {
            let frame = ServerFrame::ScreenUpload {
                id: id.to_owned(),
                chunk: u32::try_from(index).map_err(|_| Unstored::NoAnswer)?,
                last: index + 1 == count,
                data: base64::engine::general_purpose::STANDARD.encode(chunk),
            };
            let (sent, written) = oneshot::channel();
            self.controls
                .send(Control {
                    frame,
                    sent: Some(sent),
                })
                .await
                .map_err(|_| Unstored::NoAnswer)?;
            let outcome = flight.insert(written).await;
            *flight = None;
            if !matches!(outcome, Ok(Ok(()))) {
                return Err(Unstored::NoAnswer);
            }
        }
        match answered.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(reason)) => Err(Unstored::Refused(reason)),
            Err(_) => Err(Unstored::NoAnswer),
        }
    }
}

#[async_trait]
impl ScreenStore for LinkScreenStore {
    async fn store(&self, id: &str, png: &[u8], wait: Duration) -> Result<(), Unstored> {
        if png.is_empty() {
            return Err(Unstored::NoAnswer);
        }
        // The wait covers everything: a chunk left behind, the link's queue, the writes and the answer.
        self.time
            .wait(wait, &CancellationToken::new(), self.upload(id, png))
            .await
            .unwrap_or(Err(Unstored::NoAnswer))
    }
}

/// Takes an upload's entry out of the answers when the upload ends.
struct Forget<'a> {
    store: &'a LinkScreenStore,
    id: &'a str,
}

impl Drop for Forget<'_> {
    fn drop(&mut self) {
        if let Ok(mut answers) = self.store.answers.lock() {
            answers.remove(self.id);
        }
    }
}

#[cfg(test)]
#[path = "screen_store_tests.rs"]
mod tests;
