use super::RelayConfig;
use crate::GatewayError;
use std::{collections::HashMap, sync::Mutex, time::Instant};

struct Bucket {
    credits: f64,
    at: Instant,
}
pub(super) struct Rate {
    buckets: Mutex<HashMap<uuid::Uuid, Bucket>>,
    /// The relay settings; unset capacity and refill follow the effective `admission.per_user` at each request.
    config: RelayConfig,
    maximum: usize,
}
impl Rate {
    pub fn new(config: &RelayConfig) -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            config: config.clone(),
            maximum: config.users,
        }
    }
    /// Admits one request of `user`; `live_runs` is the effective `admission.per_user` (an administrator may override
    /// it, ADR-0028), which the unset per-user burst and refill scale with.
    pub fn admit(
        &self,
        user: uuid::Uuid,
        now: Instant,
        live_runs: u32,
    ) -> Result<(), GatewayError> {
        let config = self.config.clone().for_live_runs(live_runs);
        let (capacity, refill) = (config.capacity(), config.refill());
        let mut buckets = self
            .buckets
            .lock()
            .map_err(|_| GatewayError::StoreUnavailable)?;
        if !buckets.contains_key(&user) && buckets.len() >= self.maximum {
            buckets
                .retain(|_, bucket| balance(bucket, now, capacity, refill) < f64::from(capacity));
            if buckets.len() >= self.maximum {
                return Err(GatewayError::RateLimited(1));
            }
        }
        let bucket = buckets.entry(user).or_insert(Bucket {
            credits: f64::from(capacity),
            at: now,
        });
        bucket.credits = balance(bucket, now, capacity, refill);
        bucket.at = now;
        if bucket.credits < 1.0 {
            let delay =
                std::time::Duration::from_secs_f64((1.0 - bucket.credits) / refill).as_secs() + 1;
            return Err(GatewayError::RateLimited(delay));
        }
        bucket.credits -= 1.0;
        Ok(())
    }
}

/// A bucket's credits at `now`: refilled since its last use, never above `capacity`.
fn balance(bucket: &Bucket, now: Instant, capacity: u32, refill: f64) -> f64 {
    (bucket.credits + now.saturating_duration_since(bucket.at).as_secs_f64() * refill)
        .min(f64::from(capacity))
}
