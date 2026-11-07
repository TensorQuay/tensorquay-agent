use std::time::Duration;
use tokio::time::Instant;

pub(super) const PING: Duration = Duration::from_secs(30);
const PONG_TIMEOUT: Duration = Duration::from_secs(90);

pub(super) struct PongDeadline {
    last: Instant,
}

impl PongDeadline {
    pub fn new(now: Instant) -> Self {
        Self { last: now }
    }
    pub fn observed(&mut self, now: Instant) {
        self.last = now;
    }
    pub fn expired(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.last) >= PONG_TIMEOUT
    }
}

#[cfg(test)]
mod tests {
    use super::{PING, PongDeadline};
    #[tokio::test(start_paused = true)]
    async fn gw_i_006_socket_liveness_expires_after_three_missed_pongs() {
        let mut deadline = PongDeadline::new(tokio::time::Instant::now());
        for _ in 0..2 {
            tokio::time::advance(PING).await;
            assert!(!deadline.expired(tokio::time::Instant::now()));
        }
        tokio::time::advance(PING).await;
        assert!(deadline.expired(tokio::time::Instant::now()));
        deadline.observed(tokio::time::Instant::now());
        assert!(!deadline.expired(tokio::time::Instant::now()));
    }
}
