//! The workspace lease renewals a running holder's heartbeats make.
use crate::GatewayError;
use std::time::Duration;
use tokio::time::Instant;

/// One link's lease renewals. A renewal that fails for the store (slow or unavailable) is skipped with a warning while
/// at least half the lease remains since this link's last successful one; the next heartbeat tries again. Otherwise
/// the link closes, as before, so the lease never lapses under an open link on heartbeats a third of it apart (30 s
/// and 90 s by default). Before any renewal on the link succeeded, nothing is known about the lease, and a failure
/// closes the link.
#[derive(Debug, Default)]
pub(crate) struct Renewals {
    renewed: Option<Instant>,
}

impl Renewals {
    /// What a renewal's `result` at `now`, for a lease of `lease_ttl`, means for the link: `Err` closes it.
    pub fn outcome(
        &mut self,
        now: Instant,
        lease_ttl: Duration,
        result: Result<(), GatewayError>,
    ) -> Result<(), GatewayError> {
        match result {
            Ok(()) => {
                self.renewed = Some(now);
                Ok(())
            }
            Err(error @ (GatewayError::Timeout | GatewayError::StoreUnavailable))
                if self.renewed.is_some_and(|renewed| {
                    now.saturating_duration_since(renewed) < lease_ttl / 2
                }) =>
            {
                tracing::warn!(
                    operation = "lease_renewal",
                    code = error.code(),
                    "the workspace lease renewal failed; the next heartbeat tries again"
                );
                Ok(())
            }
            Err(error) => Err(error),
        }
    }
}
