//! Hold points in a stream's open, kept by the event store that owns its subscription, for tests only: after its
//! checks (before it subscribes) and after it subscribed and read the task's state again. They make a delete landing on
//! either side reproducible (the purge review's race). Nothing holds them unless a test asks; there is no endpoint.
use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::Notify;

/// Where in a stream's open a hold waits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenPoint {
    /// After the endpoint's checks, before the stream subscribes.
    Checked,
    /// After the stream subscribed and read the task's state again.
    Subscribed,
}

/// While held, the next stream open waits at the held point.
#[derive(Default)]
pub(crate) struct OpenGate {
    held: Mutex<Option<(OpenPoint, StreamOpenHold)>>,
}

/// A held `OpenGate` (crate-private): a test waits until a stream open reaches it, does what it needs, then releases
/// it.
#[derive(Clone)]
pub struct StreamOpenHold {
    reached: Arc<Notify>,
    release: Arc<Notify>,
}

impl StreamOpenHold {
    /// Waits until a stream open has reached the held point.
    pub async fn reached(&self) {
        self.reached.notified().await;
    }

    /// Lets the waiting stream open go on.
    pub fn release(&self) {
        self.release.notify_one();
    }
}

impl OpenGate {
    /// Holds the next stream open at `point` until the returned hold is released.
    pub(crate) fn hold(&self, point: OpenPoint) -> StreamOpenHold {
        let hold = StreamOpenHold {
            reached: Arc::new(Notify::new()),
            release: Arc::new(Notify::new()),
        };
        *self.held.lock().unwrap_or_else(PoisonError::into_inner) = Some((point, hold.clone()));
        hold
    }

    /// Passes at once unless held at `point`; when held there, says so and waits for the release (once).
    pub(crate) async fn pass(&self, point: OpenPoint) {
        let held = {
            let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
            if held.as_ref().is_some_and(|(at, _)| *at == point) {
                held.take().map(|(_, hold)| hold)
            } else {
                None
            }
        };
        if let Some(hold) = held {
            hold.reached.notify_one();
            hold.release.notified().await;
        }
    }
}
