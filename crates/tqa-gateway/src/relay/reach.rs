//! Whether each route's model server could be reached on its latest call (GW-I-320). A call that can't connect marks
//! the route: the model list says it isn't answering, so New task doesn't start on it and the picker says why, and for
//! a short hold the relay answers the route's next calls at once instead of waiting for the connection again. A call
//! that connects clears the mark. Kept in memory: a restarted gateway knows nothing until a call tries.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long after a failed connection the relay answers a route's calls at once. A task's own retries fall within it;
/// the first call after it tries the server again, so a server that comes back is reached within this time.
pub const UNREACHABLE_HOLD: Duration = Duration::from_secs(10);

#[derive(Clone, Copy)]
enum Seen {
    Reached,
    Unreachable(Instant),
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

    /// The route's server couldn't be reached within the hold: the call is answered at once.
    pub(crate) fn held(&self, route: &str) -> bool {
        self.with(|routes| {
            matches!(routes.get(route), Some(Seen::Unreachable(at)) if at.elapsed() < UNREACHABLE_HOLD)
        })
    }

    pub(crate) fn unreachable(&self, route: &str) {
        self.with(|routes| routes.insert(route.to_owned(), Seen::Unreachable(Instant::now())));
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
    fn a_route_is_unknown_then_unreachable_and_held_then_reached() {
        let reach = Reach::default();
        assert_eq!(reach.answering("local"), None);
        assert!(!reach.held("local"));
        reach.unreachable("local");
        assert_eq!(reach.answering("local"), Some(false));
        assert!(reach.held("local"));
        assert!(!reach.held("other"), "only the route that failed");
        reach.reached("local");
        assert_eq!(reach.answering("local"), Some(true));
        assert!(!reach.held("local"));
    }
}
