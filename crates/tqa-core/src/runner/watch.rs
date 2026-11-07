//! Await guards are independent of journal mutation and never surround a sink append.
use super::session::Stop;
use std::{future::Future, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use tqa_types::Sleeper;

pub(super) struct Watch {
    pub cancel: CancellationToken,
    pub shutdown: CancellationToken,
    pub remaining: Duration,
    pub sleeper: Arc<dyn Sleeper>,
}
impl Watch {
    pub async fn wait<F: Future>(&self, future: F) -> Result<F::Output, Stop> {
        tokio::select! { biased;
            () = self.shutdown.cancelled() => Err(Stop::Shutdown),
            () = self.cancel.cancelled() => Err(Stop::Cancel),
            () = self.sleeper.sleep(self.remaining) => Err(Stop::wall_clock()),
            value = future => Ok(value),
        }
    }
}
