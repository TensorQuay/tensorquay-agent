//! The per-user connector rate (ADR-0031, the `mcp.calls_per_minute` limit): a token bucket per user holding a minute's
//! worth of messages and refilled continuously, like the relay's. Every message the proxy forwards costs one; a batch
//! larger than the whole bucket is admitted from a full one, so no body is refused for ever. Pure, with time and the
//! effective limit passed in, so an administrator's override applies from the next call (GW-U-043).
use std::collections::HashMap;

use tokio::time::Instant;

/// Users with a bucket at once; a full bucket carries nothing and is forgotten first.
pub(crate) const MAX_USERS: usize = 10_000;

struct Bucket {
    credits: f64,
    at: Instant,
}

#[derive(Default)]
pub(crate) struct Rate {
    buckets: HashMap<uuid::Uuid, Bucket>,
}

impl Rate {
    /// Takes `cost` messages from `user`'s bucket of `per_minute`.
    ///
    /// # Errors
    /// The whole seconds to wait before the call would pass.
    pub(crate) fn admit(
        &mut self,
        user: uuid::Uuid,
        cost: usize,
        per_minute: u32,
        now: Instant,
    ) -> Result<(), u64> {
        let capacity = f64::from(per_minute.max(1));
        if !self.buckets.contains_key(&user) && self.buckets.len() >= MAX_USERS {
            self.buckets
                .retain(|_, bucket| balance(bucket, now, capacity) < capacity);
            if self.buckets.len() >= MAX_USERS {
                return Err(1);
            }
        }
        let bucket = self.buckets.entry(user).or_insert(Bucket {
            credits: capacity,
            at: now,
        });
        bucket.credits = balance(bucket, now, capacity);
        bucket.at = now;
        let cost = f64::from(u32::try_from(cost).unwrap_or(u32::MAX)).min(capacity);
        if bucket.credits < cost {
            // `cost` never exceeds `capacity`, so a minute always suffices.
            let credits = bucket.credits;
            let wait = (1..=60_u32)
                .find(|seconds| credits + f64::from(*seconds) * capacity / 60.0 >= cost)
                .unwrap_or(60);
            return Err(u64::from(wait));
        }
        bucket.credits -= cost;
        Ok(())
    }

    #[cfg(test)]
    fn users(&self) -> usize {
        self.buckets.len()
    }
}

/// A bucket's credits at `now`: refilled since its last use, never above `capacity` (a lowered limit caps it at once).
fn balance(bucket: &Bucket, now: Instant, capacity: f64) -> f64 {
    let elapsed = now.saturating_duration_since(bucket.at).as_secs_f64();
    (bucket.credits + elapsed * capacity / 60.0).min(capacity)
}

#[cfg(test)]
#[path = "rate_tests.rs"]
mod tests;
