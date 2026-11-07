//! Bounded HTTP streaming with an owned task that observes cancellation during consumer pauses.

use super::{Decoder, Wire};
use crate::{
    CompletionStream, HttpConfig, LlmError, StreamChunk, TimeoutKind, collect::Accumulator,
    http::deadline,
};
use bytes::Bytes;
use futures::Stream;
use std::{
    collections::VecDeque,
    future::Future as _,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::{sync::mpsc, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use tqa_types::Sleeper;

struct State<W> {
    response: reqwest::Response,
    decoder: Decoder<W>,
    input: Bytes,
    chunks: VecDeque<StreamChunk>,
    accumulator: Option<Accumulator>,
    config: HttpConfig,
    sleeper: Arc<dyn Sleeper>,
    cancel: CancellationToken,
}

async fn send_chunk(
    sender: &mpsc::Sender<Result<StreamChunk, LlmError>>,
    chunk: Result<StreamChunk, LlmError>,
    cancel: &CancellationToken,
) -> bool {
    tokio::select! { biased;
        () = cancel.cancelled() => false,
        result = sender.send(chunk) => result.is_ok(),
    }
}

async fn pump<W: Wire>(mut state: State<W>, sender: mpsc::Sender<Result<StreamChunk, LlmError>>) {
    let cancel = state.cancel.clone();
    loop {
        let chunk = match state.next().await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => return,
            Err(error) => {
                drop(state);
                send_chunk(&sender, Err(error), &cancel).await;
                return;
            }
        };
        if !send_chunk(&sender, Ok(chunk), &cancel).await {
            return;
        }
    }
}

struct OwnedStream {
    receiver: mpsc::Receiver<Result<StreamChunk, LlmError>>,
    task: JoinHandle<()>,
    cancel: CancellationToken,
    finished: bool,
    joined: bool,
}

impl Stream for OwnedStream {
    type Item = Result<StreamChunk, LlmError>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.cancel.is_cancelled() && !self.finished {
            self.finished = true;
            self.receiver.close();
            self.task.abort();
            return Poll::Ready(Some(Err(LlmError::Cancelled)));
        }
        if !self.finished {
            match self.receiver.poll_recv(context) {
                Poll::Ready(Some(chunk)) => {
                    self.finished = chunk.is_err();
                    return Poll::Ready(Some(chunk));
                }
                Poll::Ready(None) => self.finished = true,
                Poll::Pending => return Poll::Pending,
            }
        }
        if self.joined {
            return Poll::Ready(None);
        }
        match Pin::new(&mut self.task).poll(context) {
            Poll::Ready(result) => {
                self.joined = true;
                if result.is_err_and(|error| !error.is_cancelled()) {
                    return Poll::Ready(Some(Err(LlmError::Io("stream task failed".into()))));
                }
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for OwnedStream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl<W: Wire> State<W> {
    async fn next(&mut self) -> Result<Option<StreamChunk>, LlmError> {
        loop {
            if self.cancel.is_cancelled() {
                return Err(LlmError::Cancelled);
            }
            if let Some(chunk) = self.chunks.pop_front() {
                return Ok(Some(chunk));
            }
            if self.decoder.done {
                return self.finish();
            }
            if !self.input.is_empty() {
                self.decode_pending()?;
                continue;
            }
            let bytes = deadline(
                self.response.chunk(),
                self.sleeper.as_ref(),
                self.config.idle_timeout,
                &self.cancel,
                TimeoutKind::Idle,
            )
            .await?;
            match bytes {
                Ok(Some(bytes)) => self.input = bytes,
                Ok(None) => return self.finish(),
                Err(_) => {
                    return Err(LlmError::StreamCut {
                        partial: Box::new(
                            self.accumulator
                                .as_ref()
                                .map_or_else(Default::default, Accumulator::snapshot),
                        ),
                    });
                }
            }
        }
    }

    fn decode_pending(&mut self) -> Result<(), LlmError> {
        for chunk in self.decoder.decode(&mut self.input)? {
            if let Some(accumulator) = &mut self.accumulator {
                accumulator.push(&chunk)?;
            }
            self.chunks.push_back(chunk);
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<Option<StreamChunk>, LlmError> {
        if let Some(accumulator) = self.accumulator.take() {
            accumulator.finish()?;
        }
        Ok(None)
    }
}

pub(crate) fn response_stream<W: Wire + 'static>(
    response: reqwest::Response,
    config: HttpConfig,
    wire: W,
    sleeper: Arc<dyn Sleeper>,
    cancel: CancellationToken,
) -> CompletionStream {
    let state = State {
        response,
        decoder: Decoder::with_wire(config.stream.max_sse_event_bytes, wire),
        input: Bytes::new(),
        chunks: VecDeque::new(),
        accumulator: Some(Accumulator::new(config.stream)),
        config,
        sleeper,
        cancel: cancel.clone(),
    };
    let (sender, receiver) = mpsc::channel(1);
    let task = tokio::spawn(pump(state, sender));
    Box::pin(OwnedStream {
        receiver,
        task,
        cancel,
        finished: false,
        joined: false,
    })
}
