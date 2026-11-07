//! Shared injected monotonic deadlines for transport and supervisor work.
use crate::RuntimeError;
use std::{future::Future, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use tqa_types::{Clock, Sleeper};
#[derive(Clone)]
pub(crate) struct Time {
    pub clock: Arc<dyn Clock>,
    pub sleeper: Arc<dyn Sleeper>,
}
impl Time {
    pub async fn wait<T>(
        &self,
        duration: Duration,
        cancel: &CancellationToken,
        future: impl Future<Output = T>,
    ) -> Result<T, RuntimeError> {
        tokio::select! {biased;
            ()=cancel.cancelled()=>Err(RuntimeError::Stopped),
            result=future=>Ok(result),
            ()=self.sleeper.sleep(duration)=>Err(RuntimeError::Timeout),
        }
    }
    /// As [`Self::wait`], with its bound named for a test's fake clock (`Sleeper::sleep_named`).
    pub async fn wait_named<T>(
        &self,
        duration: Duration,
        name: &'static str,
        future: impl Future<Output = T>,
    ) -> Result<T, RuntimeError> {
        tokio::select! {biased;
            result=future=>Ok(result),
            ()=self.sleeper.sleep_named(duration,name)=>Err(RuntimeError::Timeout),
        }
    }
}
