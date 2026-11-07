//! Share-by-email attempts per caller (task 076g, GW-I-049): the sign-in window (`auth/rate.rs` `LoginRate`, 10 attempts
//! a minute) keyed by user instead of address, so an owner cannot probe many addresses for accounts.
use crate::GatewayError;
use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};
use uuid::Uuid;

#[cfg(test)]
#[path = "share_rate_tests.rs"]
mod tests;

const WINDOW: Duration = Duration::from_mins(1);
const ATTEMPTS: u32 = 10;
/// Callers tracked at once; beyond it new callers wait for the window to pass.
const CAPACITY: usize = 10_000;

pub(crate) struct ShareRate {
    windows: Mutex<HashMap<Uuid, (Instant, u32)>>,
}

pub(crate) static SHARE_RATE: LazyLock<ShareRate> = LazyLock::new(ShareRate::new);

impl ShareRate {
    pub(crate) fn new() -> Self {
        Self {
            windows: Mutex::new(HashMap::new()),
        }
    }

    /// Counts one attempt by `user`.
    ///
    /// # Errors
    /// `busy` (429) once the caller used up the window, or while too many callers are tracked.
    pub(crate) fn check(&self, user: Uuid, now: Instant) -> Result<(), GatewayError> {
        let mut windows = self
            .windows
            .lock()
            .map_err(|_| GatewayError::StoreUnavailable)?;
        windows.retain(|_, (start, _)| now.saturating_duration_since(*start) < WINDOW);
        if !windows.contains_key(&user) && windows.len() >= CAPACITY {
            return Err(GatewayError::Busy);
        }
        let window = windows.entry(user).or_insert((now, 0));
        if window.1 >= ATTEMPTS {
            return Err(GatewayError::Busy);
        }
        window.1 += 1;
        Ok(())
    }
}
