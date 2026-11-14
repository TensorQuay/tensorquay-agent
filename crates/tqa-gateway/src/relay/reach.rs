//! Whether each route's model server could be reached on its latest call (GW-I-320). A call that can't connect marks
//! the route at once: the model list says it isn't answering, so New task doesn't start on it and the picker says why.
//! A server down for longer than a short grace is answered for at once, between real tries: a task's retries don't each
//! wait for the connection, and one real try every `UNREACHABLE_HOLD` finds a server that came back. A server down
//! only briefly (a restart) is tried on every call, so a task's own retries can still reach it. A call that connects
//! clears the mark. Kept in memory: a restarted gateway knows nothing until a call tries.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long a route must have been unreachable before its calls are answered at once: a server restarting within it is
/// still tried on every call.
pub const UNREACHABLE_GRACE: Duration = Duration::from_secs(30);

/// After the grace, how long after a real try the next calls are answered at once; the first call after it tries again.
pub const UNREACHABLE_HOLD: Duration = Duration::from_secs(10);

#[derive(Clone, Copy)]
enum Seen {
    Reached,
    /// Unreachable since the first failed call of this outage; `tried` is the latest real try.
    Unreachable {
        since: Instant,
        tried: Instant,
    },
}

#[derive(Default)]
pub(crate) struct Reach {
    routes: Mutex<HashMap<String, Seen>>,
}

impl Reach {
    fn with<T>(&self, run: impl FnOnce(&mut HashMap<String, Seen>) -> T) -> T {
        // A panic while the map was held leaves it whole (each change is one insert), so a poisoned lock is still read.
        let mut routes = self
            .routes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        run(&mut routes)
    }

    /// The call is answered at once: the server has been unreachable past the grace and was tried within the hold.
    pub(crate) fn held(&self, route: &str) -> bool {
        self.held_at(route, Instant::now())
    }

    fn held_at(&self, route: &str, now: Instant) -> bool {
        self.with(|routes| match routes.get(route) {
            Some(Seen::Unreachable { since, tried }) => {
                now.saturating_duration_since(*since) >= UNREACHABLE_GRACE
                    && now.saturating_duration_since(*tried) < UNREACHABLE_HOLD
            }
            _ => false,
        })
    }

    pub(crate) fn unreachable(&self, route: &str) {
        self.unreachable_at(route, Instant::now());
    }

    fn unreachable_at(&self, route: &str, now: Instant) {
        self.with(|routes| {
            let since = match routes.get(route) {
                Some(Seen::Unreachable { since, .. }) => *since,
                _ => now,
            };
            routes.insert(route.to_owned(), Seen::Unreachable { since, tried: now });
        });
    }

    pub(crate) fn reached(&self, route: &str) {
        self.with(|routes| routes.insert(route.to_owned(), Seen::Reached));
    }

    /// For the model list: true once a call reached the server, false while the latest call couldn't, null before any.
    pub(crate) fn answering(&self, route: &str) -> Option<bool> {
        self.with(|routes| routes.get(route).map(|seen| matches!(seen, Seen::Reached)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // TEST: GW-I-320
    #[test]
    fn a_route_is_unknown_then_unreachable_then_reached() {
        let reach = Reach::default();
        assert_eq!(reach.answering("local"), None);
        reach.unreachable("local");
        assert_eq!(reach.answering("local"), Some(false));
        assert_eq!(reach.answering("other"), None, "only the route that failed");
        reach.reached("local");
        assert_eq!(reach.answering("local"), Some(true));
        assert!(!reach.held("local"));
    }

    // TEST: GW-I-320
    #[test]
    fn a_brief_outage_is_tried_on_every_call_and_a_long_one_once_per_hold() {
        let reach = Reach::default();
        let start = Instant::now();
        let at = |seconds: u64| start + Duration::from_secs(seconds);
        reach.unreachable_at("local", at(0));
        // Within the grace (a server restarting): every call tries, so a task's retries can reach it.
        for second in [1, 5, 29] {
            assert!(!reach.held_at("local", at(second)), "{second} s");
        }
        reach.unreachable_at("local", at(29));
        // Past the grace: calls within the hold of the latest try are answered at once; the first after it tries.
        assert!(reach.held_at("local", at(31)));
        assert!(reach.held_at("local", at(38)));
        assert!(!reach.held_at("local", at(39)), "a real try after the hold");
        reach.unreachable_at("local", at(39));
        assert!(reach.held_at("local", at(40)), "the outage keeps its start");
        reach.reached("local");
        assert!(!reach.held_at("local", at(41)));
    }
}
