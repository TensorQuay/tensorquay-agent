//! The runner's view of its own path to the gateway (CORE-U-062). Every model call goes to the gateway's relay, so a
//! transport failure (`Connection`, `Io`, `StreamCut`) may be the provider's or the path's; the embedding runtime
//! settles it through its link to the gateway. Trait only: `tqa-core` does no network I/O of its own.
use async_trait::async_trait;
use std::time::Duration;

/// What settling the link found.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinkSettle {
    /// A round trip over the link succeeded: the failure was not the path's, and it counts as before.
    Reachable,
    /// The link was down and came back after this long: the failed attempt is given back.
    Restored(Duration),
    /// The link was still down when the given budget, spent here, ran out: the failure counts as before.
    Unreachable(Duration),
}

/// The runtime's link to the gateway, consulted only after a transport failure of a model attempt.
#[async_trait]
pub trait GatewayLink: Send + Sync {
    /// The most one step may wait for the link, in total across its failures (`link_wait`).
    fn budget(&self) -> Duration;
    /// Proves the gateway reachable, or waits up to `budget` for the link to come back. The runner bounds the call by
    /// the run's cancellation, shutdown and wall clock, so it need not.
    async fn settle(&self, budget: Duration) -> LinkSettle;
}
