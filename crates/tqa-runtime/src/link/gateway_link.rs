//! The runner's `GatewayLink` over this runtime's link (CORE-U-062, RT-I-033). A model call's path to the gateway is the
//! link's path, so after a transport failure the link settles it: a registered link proves itself with a ping answered
//! within `io_timeout`; a link that is down (or fails that proof, and redials) is waited for until it registers again.
use super::{LinkProbe, LinkState};
use crate::{RuntimeLimits, time::Time};
use async_trait::async_trait;
use std::{sync::Arc, time::Duration};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;
use tqa_core::{GatewayLink, LinkSettle};

/// The name of a settle's wait for the link to register again (RT-I-033).
pub const SETTLE_WAIT: &str = "link.settle";

pub(crate) struct LinkHandle {
    pub state: watch::Receiver<LinkState>,
    pub probes: mpsc::Sender<LinkProbe>,
    pub time: Time,
    /// The most one step may wait for the link (`link_wait`).
    pub budget: Duration,
    /// How long a probe may take: the link's own write and pong bound, twice.
    pub probe_bound: Duration,
}

#[async_trait]
impl GatewayLink for LinkHandle {
    fn budget(&self) -> Duration {
        self.budget
    }

    async fn settle(&self, budget: Duration) -> LinkSettle {
        let started = self.time.clock.now();
        let seen = *self.state.borrow();
        if seen.up && self.probe().await {
            return LinkSettle::Reachable;
        }
        let mut state = self.state.clone();
        let registered = async {
            state
                .wait_for(|now| now.up && now.registrations > seen.registrations)
                .await
                .is_ok()
        };
        // The probe's time comes out of the same budget: a step never waits longer than it.
        let left = budget.saturating_sub(self.time.clock.now().saturating_duration_since(started));
        let restored = self.time.wait_named(left, SETTLE_WAIT, registered).await;
        let waited = self.time.clock.now().saturating_duration_since(started);
        match restored {
            Ok(true) => LinkSettle::Restored(waited),
            _ => LinkSettle::Unreachable(waited.min(budget)),
        }
    }
}

impl LinkHandle {
    /// The link's side (its state to publish, the probes to answer) and the runner's handle over them.
    pub fn open(
        time: Time,
        limits: &RuntimeLimits,
    ) -> (
        watch::Sender<LinkState>,
        mpsc::Receiver<LinkProbe>,
        Arc<dyn GatewayLink>,
    ) {
        let (state, seen) = watch::channel(LinkState::default());
        let (probes, requests) = mpsc::channel(4);
        let handle = Self {
            state: seen,
            probes,
            time,
            budget: limits.link_wait,
            probe_bound: limits.io_timeout.saturating_mul(2),
        };
        (state, requests, Arc::new(handle))
    }

    /// Whether the registered link answered a ping; the link itself declares a missed pong a lost link and redials.
    /// Probes are read only while the link is registered, so a full queue (the link dropped with probes waiting) or a
    /// closed one is an unanswered probe, never a wait outside the bounds.
    async fn probe(&self) -> bool {
        let (reply, answer) = oneshot::channel();
        if self.probes.try_send(reply).is_err() {
            return false;
        }
        matches!(
            self.time
                .wait(self.probe_bound, &CancellationToken::new(), answer)
                .await,
            Ok(Ok(true))
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tqa_testkit::{FakeClock, FakeSleeper};

    /// Advances fake time timer by timer until `settled` finishes, within 5 s of real time.
    async fn drive(
        clock: &FakeClock,
        sleeper: &FakeSleeper,
        settled: tokio::task::JoinHandle<LinkSettle>,
    ) -> Result<LinkSettle, Box<dyn std::error::Error>> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while !settled.is_finished() {
            if tokio::time::Instant::now() > deadline {
                return Err("settle did not return within its bound".into());
            }
            if let Some(next) = sleeper.pending_durations().into_iter().min() {
                clock.advance(next)?;
            }
            tokio::task::yield_now().await;
        }
        Ok(settled.await?)
    }

    // TEST: RT-I-033 (a probe never waits outside its bounds)
    #[tokio::test]
    async fn a_full_probe_queue_with_the_link_down_settles_within_the_budget()
    -> Result<(), Box<dyn std::error::Error>> {
        let clock = FakeClock::new();
        let sleeper = Arc::new(FakeSleeper::new(clock.clone()));
        let time = Time {
            clock: Arc::new(clock.clone()),
            sleeper: sleeper.clone(),
        };
        // The runner saw the link registered; it then dropped with a stale probe queued, and nothing reads the queue.
        let (_state, seen) = watch::channel(LinkState {
            up: true,
            registrations: 1,
        });
        let (probes, _unread) = mpsc::channel(1);
        probes.try_send(oneshot::channel().0)?;
        let bound = Duration::from_secs(10);
        let handle = LinkHandle {
            state: seen,
            probes,
            time,
            budget: bound,
            probe_bound: bound,
        };
        let settled = tokio::spawn(async move { handle.settle(bound).await });
        let outcome = drive(&clock, &sleeper, settled).await?;
        assert_eq!(outcome, LinkSettle::Unreachable(bound));
        Ok(())
    }

    // TEST: RT-I-033 (the probe's time comes out of the step's budget)
    #[tokio::test]
    async fn an_unanswered_probe_and_the_wait_after_it_share_one_budget()
    -> Result<(), Box<dyn std::error::Error>> {
        let clock = FakeClock::new();
        let sleeper = Arc::new(FakeSleeper::new(clock.clone()));
        let time = Time {
            clock: Arc::new(clock.clone()),
            sleeper: sleeper.clone(),
        };
        // A registered link whose probe is taken but never answered (no pong, and it has not yet noticed).
        let (_state, seen) = watch::channel(LinkState {
            up: true,
            registrations: 1,
        });
        let (probes, _unanswered) = mpsc::channel(1);
        let (budget, probe_bound) = (Duration::from_secs(10), Duration::from_secs(4));
        let handle = LinkHandle {
            state: seen,
            probes,
            time,
            budget,
            probe_bound,
        };
        let started = tqa_types::Clock::now(&clock);
        let settled = tokio::spawn(async move { handle.settle(budget).await });
        let outcome = drive(&clock, &sleeper, settled).await?;
        assert_eq!(outcome, LinkSettle::Unreachable(budget));
        assert_eq!(
            tqa_types::Clock::now(&clock).saturating_duration_since(started),
            budget,
            "4 s of probe and 6 s of waiting, not 4 + 10"
        );
        Ok(())
    }
}
