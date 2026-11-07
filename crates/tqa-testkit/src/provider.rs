//! In-process provider scripts with injected time and owned, task-free streams.
use async_trait::async_trait;
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use tqa_llm::{CompletionRequest, CompletionStream, LlmError, ModelProvider, Route, StreamChunk};
use tqa_types::Sleeper;

/// One stream item released after an injected-clock delay.
#[derive(Debug)]
pub struct ProviderChunk {
    /// Delay after the previous item, measured by the supplied sleeper.
    pub delay: Duration,
    /// A chunk or a terminal stream failure.
    pub result: Result<StreamChunk, LlmError>,
}

/// One finite completion script; missing a finish chunk deliberately models a cut.
#[derive(Debug)]
pub struct ProviderReply {
    /// Delay before returning the stream or the initial failure.
    pub delay: Duration,
    /// Stream items or a failure such as an HTTP status error.
    pub result: Result<Vec<ProviderChunk>, LlmError>,
}

/// A provider fake that records requests and consumes replies in arrival order without HTTP.
pub struct FakeProvider {
    route: Route,
    replies: Mutex<VecDeque<ProviderReply>>,
    requests: Mutex<Vec<CompletionRequest>>,
    sleeper: Arc<dyn Sleeper>,
    active: Arc<AtomicUsize>,
}
impl FakeProvider {
    /// Creates a finite provider script with explicit route and time dependencies.
    #[must_use]
    pub fn new(route: Route, replies: Vec<ProviderReply>, sleeper: Arc<dyn Sleeper>) -> Self {
        Self {
            route,
            replies: Mutex::new(replies.into()),
            requests: Mutex::new(vec![]),
            sleeper,
            active: Arc::default(),
        }
    }
    /// Returns every received request in arrival order, including failed requests.
    #[must_use]
    pub fn requests(&self) -> Vec<CompletionRequest> {
        self.requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
    /// Counts streams still held by their caller; exhaustion or dropping releases their ownership.
    #[must_use]
    pub fn active_streams(&self) -> usize {
        self.active.load(Ordering::Relaxed)
    }
}

#[async_trait]
impl ModelProvider for FakeProvider {
    fn route(&self) -> &Route {
        &self.route
    }
    async fn complete(
        &self,
        request: CompletionRequest,
        cancel: CancellationToken,
    ) -> Result<CompletionStream, LlmError> {
        self.requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(request);
        if cancel.is_cancelled() {
            return Err(LlmError::Cancelled);
        }
        let reply = self
            .replies
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
            .ok_or_else(|| LlmError::Protocol("fake provider script exhausted".into()))?;
        let chunks = tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(LlmError::Cancelled),
            () = self.sleeper.sleep(reply.delay) => reply.result?,
        };
        self.active.fetch_add(1, Ordering::Relaxed);
        let state = StreamState {
            chunks: chunks.into_iter(),
            sleeper: Arc::clone(&self.sleeper),
            cancel,
            ended: false,
            _lease: StreamLease(Arc::clone(&self.active)),
        };
        Ok(Box::pin(futures::stream::unfold(state, StreamState::next)))
    }
}

struct StreamState {
    chunks: std::vec::IntoIter<ProviderChunk>,
    sleeper: Arc<dyn Sleeper>,
    cancel: CancellationToken,
    ended: bool,
    _lease: StreamLease,
}
impl StreamState {
    async fn next(mut self) -> Option<(Result<StreamChunk, LlmError>, Self)> {
        if self.ended {
            return None;
        }
        if self.cancel.is_cancelled() {
            self.ended = true;
            return Some((Err(LlmError::Cancelled), self));
        }
        let chunk = self.chunks.next()?;
        let result = tokio::select! {
            biased;
            () = self.cancel.cancelled() => Err(LlmError::Cancelled),
            () = self.sleeper.sleep(chunk.delay) => chunk.result,
        };
        self.ended = result.is_err();
        Some((result, self))
    }
}

struct StreamLease(Arc<AtomicUsize>);
impl Drop for StreamLease {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}
