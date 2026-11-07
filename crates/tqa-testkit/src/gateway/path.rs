//! The runtime's network path to the fake gateway, which a test can cut the way a link outage does: while
//! it is down, every request fails before response headers (a connection closed, whatever connection it arrives on,
//! pooled or new, the link's redials included); a response already streaming is cut mid-body; and the registered link
//! goes silent (`socket.rs`). Restoring it lets all of that work again.
use super::Shared;
use axum::{
    body::{Body, Bytes},
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use futures::Stream;
use std::{
    future::Future,
    io,
    pin::Pin,
    sync::{Arc, PoisonError},
    task::{Context, Poll},
};
use tokio_util::sync::CancellationToken;

/// The current path: cancelled while down; `set_path_down(false)` installs a fresh one.
pub(super) fn current(shared: &Shared) -> CancellationToken {
    shared
        .path
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// Fails a request before headers while the path is down, and makes a response's body fail if the path goes down
/// while it streams. The link's upgrade response is left alone: its socket handles the cut itself.
pub(super) async fn guard(
    State(shared): State<Arc<Shared>>,
    request: Request,
    next: Next,
) -> Response {
    let path = current(&shared);
    if path.is_cancelled() {
        return severed();
    }
    let link = request.uri().path() == "/runtime/link";
    let response = next.run(request).await;
    if link {
        return response;
    }
    let (parts, body) = response.into_parts();
    let body = Severable {
        inner: body.into_data_stream(),
        cut: Box::pin(path.cancelled_owned()),
        done: false,
    };
    Response::from_parts(parts, Body::from_stream(body))
}

/// A response whose body fails before anything is written, so the connection closes before its headers: hyper writes
/// the head only with the first body data, which never comes (TK-U-028 pins this).
fn severed() -> Response {
    Response::new(Body::from_stream(futures::stream::once(async {
        Err::<Bytes, io::Error>(io::Error::from(io::ErrorKind::ConnectionAborted))
    })))
}

/// A body that fails once the path is cut, even mid-stream.
struct Severable<S> {
    inner: S,
    cut: Pin<Box<dyn Future<Output = ()> + Send>>,
    done: bool,
}

impl<S, E> Stream for Severable<S>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
    E: std::error::Error + Send + Sync + 'static,
{
    type Item = Result<Bytes, io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.done {
            return Poll::Ready(None);
        }
        if self.cut.as_mut().poll(context).is_ready() {
            self.done = true;
            return Poll::Ready(Some(Err(io::Error::from(io::ErrorKind::ConnectionReset))));
        }
        Pin::new(&mut self.inner)
            .poll_next(context)
            .map(|item| item.map(|chunk| chunk.map_err(io::Error::other)))
    }
}
