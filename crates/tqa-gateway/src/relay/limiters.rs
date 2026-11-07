//! One request limiter per model route (LLM-U-012), shared by every conversation and user of the route. A route whose
//! limits change gets a new limiter; requests already in flight keep their permits on the old one until they end.

use super::RelayConfig;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use tqa_llm::{Limits, QueueBounds, Route, RouteLimiter, SystemClock, TokioSleeper};

pub(super) struct Limiters {
    routes: Mutex<HashMap<String, (Limits, Arc<RouteLimiter>)>>,
    bounds: QueueBounds,
}

impl Limiters {
    pub fn new(config: &RelayConfig) -> Self {
        Self {
            routes: Mutex::new(HashMap::new()),
            bounds: QueueBounds {
                wait: Duration::from_secs(config.queue_wait),
                max_waiting: config.queue_max,
            },
        }
    }

    /// The route's limiter, built from its current limits. Entries are never evicted while their limits are
    /// unchanged: a request in flight holds its permit through the limiter's semaphore, so replacing a busy limiter
    /// would admit more requests than the route allows. The table holds one small entry per admin-defined route.
    pub fn for_route(&self, route: &Route) -> Arc<RouteLimiter> {
        let mut routes = self.routes.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((limits, limiter)) = routes.get(&route.name)
            && *limits == route.limits
        {
            return limiter.clone();
        }
        let limiter = Arc::new(RouteLimiter::new(
            route.limits,
            self.bounds,
            Arc::new(SystemClock),
            Arc::new(TokioSleeper),
        ));
        routes.insert(route.name.clone(), (route.limits, limiter.clone()));
        limiter
    }
}
