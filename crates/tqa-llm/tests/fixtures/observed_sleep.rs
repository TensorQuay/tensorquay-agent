//! Observe server-body cancellation through the lifetime of its pending delay.

use std::{future::Future, pin::Pin, time::Duration};
use tokio::sync::watch;
use tqa_testkit::{FakeClock, FakeSleeper};
use tqa_types::Sleeper;

pub struct ObservedSleep {
    inner: FakeSleeper,
    active: watch::Sender<bool>,
}

impl ObservedSleep {
    pub fn new(clock: FakeClock) -> Self {
        Self {
            inner: FakeSleeper::new(clock),
            active: watch::channel(false).0,
        }
    }

    pub async fn wait(&self, active: bool) -> Result<(), watch::error::RecvError> {
        let mut receiver = self.active.subscribe();
        while *receiver.borrow_and_update() != active {
            receiver.changed().await?;
        }
        Ok(())
    }
}

struct Guard<'a>(&'a watch::Sender<bool>);

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        self.0.send_replace(false);
    }
}

impl Sleeper for ObservedSleep {
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            self.active.send_replace(true);
            let _guard = Guard(&self.active);
            self.inner.sleep(duration).await;
        })
    }
}
