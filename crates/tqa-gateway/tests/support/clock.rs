use std::{
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::Duration,
};
use tqa_gateway::IdleClock;

pub struct Clock {
    start: tokio::time::Instant,
    frozen: AtomicBool,
    nanos: AtomicU64,
    reads: AtomicU64,
    changed: tokio::sync::Notify,
}

impl Clock {
    pub fn new() -> Self {
        Self {
            start: tokio::time::Instant::now(),
            frozen: AtomicBool::new(false),
            nanos: AtomicU64::new(0),
            reads: AtomicU64::new(0),
            changed: tokio::sync::Notify::new(),
        }
    }

    pub fn freeze(&self) {
        self.nanos.store(
            u64::try_from(self.start.elapsed().as_nanos()).unwrap_or(u64::MAX),
            Ordering::SeqCst,
        );
        self.frozen.store(true, Ordering::SeqCst);
    }

    pub fn advance(&self, duration: Duration) {
        self.nanos.fetch_add(
            u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX),
            Ordering::SeqCst,
        );
        // Every task waiting on the clock wakes (the idle sweeper and a teardown's retries both do), and one about to
        // wait finds the stored permit: waking one waiter per change let the other miss it and sleep in real time.
        self.changed.notify_waiters();
        self.changed.notify_one();
    }

    pub async fn observed(&self) -> super::TestResult {
        let before = self.reads.load(Ordering::SeqCst);
        self.changed.notify_one();
        tokio::time::timeout(super::DEADLINE, async {
            while self.reads.load(Ordering::SeqCst) == before {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl IdleClock for Clock {
    async fn changed(&self) {
        self.changed.notified().await;
    }
    fn now(&self) -> Duration {
        self.reads.fetch_add(1, Ordering::SeqCst);
        if self.frozen.load(Ordering::SeqCst) {
            Duration::from_nanos(self.nanos.load(Ordering::SeqCst))
        } else {
            self.start.elapsed()
        }
    }
}
