//! LLM-U-012: the per-route limiter. Concurrency and requests per minute, FIFO, a bounded wait and queue, with a
//! fake clock.

use std::{error::Error, sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tqa_llm::{LimiterRefusal, Limits, QueueBounds, RouteLimiter};
use tqa_testkit::{FakeClock, FakeSleeper};

type TestResult = Result<(), Box<dyn Error>>;

fn limiter(
    concurrency: Option<u32>,
    requests_per_minute: Option<u32>,
    bounds: QueueBounds,
) -> (Arc<RouteLimiter>, FakeClock, Arc<FakeSleeper>) {
    let clock = FakeClock::new();
    let sleeper = Arc::new(FakeSleeper::new(clock.clone()));
    let limits = Limits {
        concurrency,
        requests_per_minute,
        ..Limits::default()
    };
    let limiter = RouteLimiter::new(limits, bounds, Arc::new(clock.clone()), sleeper.clone());
    (Arc::new(limiter), clock, sleeper)
}

const WIDE: QueueBounds = QueueBounds {
    wait: Duration::from_secs(30),
    max_waiting: 64,
};

/// Yields until `ready` holds; the limiter's own tasks run on the same test runtime.
async fn until(mut ready: impl FnMut() -> bool) -> TestResult {
    for _ in 0..10_000 {
        if ready() {
            return Ok(());
        }
        tokio::task::yield_now().await;
    }
    Err("condition not reached".into())
}

#[tokio::test]
async fn llm_u_012_at_most_two_in_flight_and_the_rest_in_arrival_order() -> TestResult {
    let (limiter, _, _) = limiter(Some(2), None, WIDE);
    let (granted, mut order) = mpsc::unbounded_channel();
    let mut unblock = Vec::new();
    let mut seen = Vec::new();
    for index in 0..10 {
        let (release, released) = tokio::sync::oneshot::channel::<()>();
        unblock.push(release);
        let (task_limiter, granted) = (limiter.clone(), granted.clone());
        tokio::spawn(async move {
            let permit = task_limiter.acquire(&CancellationToken::new()).await;
            let _ = granted.send((index, permit.is_ok()));
            let _ = released.await;
            drop(permit);
        });
        // Each request is in place before the next arrives, so arrival order is defined: the first two start,
        // every later one queues.
        if index < 2 {
            seen.push(order.recv().await.ok_or("not granted")?);
        } else {
            until(|| limiter.queued() == index - 1).await?;
        }
    }
    assert!(
        order.try_recv().is_err(),
        "a third request started while two were in flight"
    );
    // Each of the first eight releases lets exactly one queued request start; the last two free nothing queued.
    for release in unblock.drain(..8) {
        let _ = release.send(());
        seen.push(order.recv().await.ok_or("no request started")?);
        assert!(order.try_recv().is_err(), "two started for one release");
    }
    drop(unblock);
    let expected: Vec<_> = (0..10).map(|index| (index, true)).collect();
    assert_eq!(seen, expected);
    Ok(())
}

#[tokio::test]
async fn llm_u_012_the_rpm_window_holds_the_next_start_for_the_rolling_minute() -> TestResult {
    let (limiter, clock, sleeper) = limiter(None, Some(60), WIDE);
    let cancel = CancellationToken::new();
    for _ in 0..60 {
        drop(limiter.acquire(&cancel).await?);
    }
    // Thirty seconds later the oldest start leaves the window in 30 s, within the 30 s wait bound.
    clock.advance(Duration::from_secs(30))?;
    let waiting = {
        let limiter = limiter.clone();
        tokio::spawn(async move { limiter.acquire(&CancellationToken::new()).await.map(drop) })
    };
    until(|| sleeper.pending_durations() == [Duration::from_secs(30)]).await?;
    clock.advance(Duration::from_secs(29))?;
    tokio::task::yield_now().await;
    assert!(
        !waiting.is_finished(),
        "the 61st start waits for the rolling minute"
    );
    clock.advance(Duration::from_secs(1))?;
    assert_eq!(waiting.await?, Ok(()));
    assert_eq!(limiter.queued(), 0);
    Ok(())
}

#[tokio::test]
async fn llm_u_012_a_start_the_window_cannot_allow_in_time_is_refused_at_once() -> TestResult {
    let bounds = QueueBounds {
        wait: Duration::from_secs(10),
        max_waiting: 64,
    };
    let (limiter, _, sleeper) = limiter(None, Some(1), bounds);
    let cancel = CancellationToken::new();
    drop(limiter.acquire(&cancel).await?);
    assert_eq!(
        limiter.acquire(&cancel).await.map(drop),
        Err(LimiterRefusal::Timeout {
            retry_after: Duration::from_mins(1)
        })
    );
    assert!(sleeper.pending_durations().is_empty(), "no pointless wait");
    Ok(())
}

#[tokio::test]
async fn llm_u_012_a_full_queue_and_an_expired_wait_are_refused_with_retry_after() -> TestResult {
    let bounds = QueueBounds {
        wait: Duration::from_secs(30),
        max_waiting: 1,
    };
    let (limiter, clock, _) = limiter(Some(1), None, bounds);
    let cancel = CancellationToken::new();
    let held = limiter.acquire(&cancel).await?;
    let waiting = {
        let limiter = limiter.clone();
        tokio::spawn(async move { limiter.acquire(&CancellationToken::new()).await.map(drop) })
    };
    until(|| limiter.queued() == 1).await?;
    assert_eq!(
        limiter.acquire(&cancel).await.map(drop),
        Err(LimiterRefusal::QueueFull {
            retry_after: Duration::from_secs(1)
        })
    );
    clock.advance(Duration::from_secs(30))?;
    assert_eq!(
        waiting.await?,
        Err(LimiterRefusal::Timeout {
            retry_after: Duration::from_secs(1)
        })
    );
    assert_eq!(limiter.queued(), 0);
    drop(held);
    assert!(limiter.acquire(&cancel).await.is_ok());
    Ok(())
}

#[tokio::test]
async fn llm_u_012_a_cancelled_waiter_frees_its_place_for_the_next() -> TestResult {
    let (limiter, _, _) = limiter(Some(1), None, WIDE);
    let held = limiter.acquire(&CancellationToken::new()).await?;
    let cancel = CancellationToken::new();
    let first = {
        let (limiter, cancel) = (limiter.clone(), cancel.clone());
        tokio::spawn(async move { limiter.acquire(&cancel).await.map(drop) })
    };
    until(|| limiter.queued() == 1).await?;
    let second = {
        let limiter = limiter.clone();
        tokio::spawn(async move { limiter.acquire(&CancellationToken::new()).await.map(drop) })
    };
    until(|| limiter.queued() == 2).await?;
    cancel.cancel();
    assert_eq!(first.await?, Err(LimiterRefusal::Cancelled));
    drop(held);
    assert_eq!(second.await?, Ok(()));
    assert_eq!(limiter.queued(), 0);
    Ok(())
}

#[tokio::test]
async fn llm_u_012_a_route_without_limits_never_waits() -> TestResult {
    let (limiter, _, sleeper) = limiter(None, None, WIDE);
    let cancel = CancellationToken::new();
    let permits: Vec<_> =
        futures::future::join_all((0..100).map(|_| limiter.acquire(&cancel))).await;
    assert!(permits.iter().all(Result::is_ok));
    assert!(sleeper.pending_durations().is_empty());
    assert_eq!(limiter.queued(), 0);
    Ok(())
}

#[tokio::test]
async fn llm_u_012_try_acquire_never_waits() -> TestResult {
    let (limiter, clock, sleeper) = limiter(Some(1), Some(2), WIDE);
    let held = limiter.try_acquire()?;
    assert_eq!(
        limiter.try_acquire().map(drop),
        Err(LimiterRefusal::QueueFull {
            retry_after: Duration::from_secs(1)
        })
    );
    drop(held);
    drop(limiter.try_acquire()?);
    // Two starts used the rolling minute; the next one is refused with the time until the oldest leaves it.
    clock.advance(Duration::from_secs(10))?;
    assert_eq!(
        limiter.try_acquire().map(drop),
        Err(LimiterRefusal::QueueFull {
            retry_after: Duration::from_secs(50)
        })
    );
    assert!(sleeper.pending_durations().is_empty(), "nothing waited");
    Ok(())
}
