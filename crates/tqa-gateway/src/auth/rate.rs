use crate::GatewayError;
use std::{collections::BTreeMap, net::IpAddr, sync::Mutex, time::Duration};
use tokio::time::Instant;

const WINDOW: Duration = Duration::from_mins(1);
const ATTEMPTS: u32 = 10;

pub(crate) struct LoginRate {
    windows: Mutex<BTreeMap<IpAddr, (Instant, u32)>>,
    capacity: usize,
}

impl LoginRate {
    pub fn new(capacity: usize) -> Self {
        Self {
            windows: Mutex::new(BTreeMap::new()),
            capacity,
        }
    }

    pub fn check(&self, address: IpAddr, now: Instant) -> Result<(), GatewayError> {
        let mut windows = self
            .windows
            .lock()
            .map_err(|_| GatewayError::StoreUnavailable)?;
        windows.retain(|_, (start, _)| now.saturating_duration_since(*start) < WINDOW);
        if !windows.contains_key(&address) && windows.len() >= self.capacity {
            return Err(GatewayError::Busy);
        }
        let window = windows.entry(address).or_insert((now, 0));
        if window.1 >= ATTEMPTS {
            // The address's own limit: a rate limit with the window's real wait, rounded up (GW-I-001), so the person
            // reads "Too many attempts" with when to try again, not a busy server's fixed second.
            let left = WINDOW.saturating_sub(now.saturating_duration_since(window.0));
            let seconds = left.as_secs() + u64::from(left.subsec_nanos() > 0);
            return Err(GatewayError::RateLimited(seconds.max(1)));
        }
        window.1 += 1;
        Ok(())
    }
}

/// Failed personal-token authentications per transport address: after `ATTEMPTS` failures in a minute
/// the address is refused for the rest of that minute, even with a valid token, so a guesser learns nothing.
pub(crate) struct TokenFailures {
    windows: Mutex<BTreeMap<IpAddr, (Instant, u32)>>,
    capacity: usize,
}

impl TokenFailures {
    const ATTEMPTS: u32 = 30;

    pub fn new(capacity: usize) -> Self {
        Self {
            windows: Mutex::new(BTreeMap::new()),
            capacity,
        }
    }

    /// Refuses an address that used up its failures in the current window.
    ///
    /// # Errors
    /// `Busy` while the address is limited, or while too many addresses are tracked.
    pub fn check(&self, address: IpAddr, now: Instant) -> Result<(), GatewayError> {
        let mut windows = self
            .windows
            .lock()
            .map_err(|_| GatewayError::StoreUnavailable)?;
        windows.retain(|_, (start, _)| now.saturating_duration_since(*start) < WINDOW);
        if windows
            .get(&address)
            .is_some_and(|(_, failures)| *failures >= Self::ATTEMPTS)
        {
            return Err(GatewayError::Busy);
        }
        Ok(())
    }

    /// Counts one failed authentication.
    ///
    /// # Errors
    /// `Busy` when too many addresses are tracked at once.
    pub fn fail(&self, address: IpAddr, now: Instant) -> Result<(), GatewayError> {
        let mut windows = self
            .windows
            .lock()
            .map_err(|_| GatewayError::StoreUnavailable)?;
        if !windows.contains_key(&address) && windows.len() >= self.capacity {
            return Err(GatewayError::Busy);
        }
        windows.entry(address).or_insert((now, 0)).1 += 1;
        Ok(())
    }
}
