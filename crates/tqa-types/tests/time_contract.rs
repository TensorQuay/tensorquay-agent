//! Clock and sleeper contracts remain object-safe and independent of an async runtime.

use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
};

use tqa_types::{Clock, Sleeper};

struct FixedTime(Instant);

impl Clock for FixedTime {
    fn now(&self) -> Instant {
        self.0
    }
}

impl Sleeper for FixedTime {
    fn sleep(&self, _: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(std::future::ready(()))
    }
}

#[test]
fn clock_and_sleeper_accept_shared_trait_objects() {
    let instant = Instant::now();
    let clock: Arc<dyn Clock> = Arc::new(FixedTime(instant));
    let sleeper: Arc<dyn Sleeper> = Arc::new(FixedTime(instant));
    assert_eq!(clock.now(), instant);
    let mut sleep = sleeper.sleep(Duration::ZERO);
    let mut context = Context::from_waker(Waker::noop());
    assert_eq!(sleep.as_mut().poll(&mut context), Poll::Ready(()));
}
