//! The proxy's recent refusals (ADR-0035 point 3, GW-I-323): a bounded, in-memory record of the last
//! [`RECENT_REFUSALS`] destinations it refused, with how often and when last, for Settings › Internet access and its
//! "Allow this site". The log keeps the rest; a restarted gateway starts empty.

use std::collections::HashMap;
use std::sync::Mutex;

/// How many refused destinations are kept: the least recently refused goes first.
pub(crate) const RECENT_REFUSALS: usize = 200;

/// One refused destination: its canonical host, port and the reason's code.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct Key {
    pub host: String,
    pub port: u16,
    pub reason: &'static str,
}

/// How often and when last.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Seen {
    pub count: u64,
    pub last_at: time::OffsetDateTime,
    /// Orders evictions: higher is more recent.
    tick: u64,
}

#[derive(Default)]
pub(crate) struct Recent {
    inner: Mutex<(HashMap<Key, Seen>, u64)>,
}

impl Recent {
    /// Records one refusal of `host:port` (already canonical) for `reason`, a refusal code.
    pub(crate) fn record(&self, host: &str, port: u16, reason: &'static str) {
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (entries, clock) = &mut *guard;
        *clock += 1;
        let tick = *clock;
        let key = Key {
            host: host.to_owned(),
            port,
            reason,
        };
        let now = time::OffsetDateTime::now_utc();
        entries
            .entry(key)
            .and_modify(|seen| {
                seen.count += 1;
                seen.last_at = now;
                seen.tick = tick;
            })
            .or_insert(Seen {
                count: 1,
                last_at: now,
                tick,
            });
        if entries.len() > RECENT_REFUSALS
            && let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, seen)| seen.tick)
                .map(|(key, _)| key.clone())
        {
            entries.remove(&oldest);
        }
    }

    /// Records the proxy's refusal of `host:port` for Settings › Internet access (GW-I-323): the canonical name or
    /// address only, as the refusal itself names it, and only a reason with a code.
    pub(crate) fn refused(&self, host: &str, port: u16, reason: &str) {
        if let (Ok(canonical), Some(code)) = (super::canonical_host(host), code(reason))
            && !canonical.starts_with("*.")
        {
            self.record(&canonical, port, code);
        }
    }

    /// Every kept refusal, the most recent first.
    pub(crate) fn list(&self) -> Vec<(Key, Seen)> {
        let guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut items: Vec<(Key, Seen)> = guard
            .0
            .iter()
            .map(|(key, seen)| (key.clone(), *seen))
            .collect();
        items.sort_by_key(|(_, seen)| std::cmp::Reverse(seen.tick));
        items
    }
}

/// A refusal's code, from the reason the proxy gives (`refusal.rs`); `None` for a reason with no code.
pub(crate) fn code(reason: &str) -> Option<&'static str> {
    use super::refusal::{
        DISABLED, INTERNAL_FORBIDDEN, INTERNAL_UNCONFIGURED, INTERNAL_UNVERIFIED, NOT_LISTED,
        PLAIN_HTTP_KEY, PRIVATE,
    };
    Some(match reason {
        r if r == NOT_LISTED => "not_listed",
        r if r == PRIVATE => "private",
        r if r == DISABLED => "disabled",
        r if r == INTERNAL_FORBIDDEN => "internal_forbidden",
        r if r == INTERNAL_UNCONFIGURED => "internal_unconfigured",
        r if r == INTERNAL_UNVERIFIED => "internal_unverified",
        r if r == PLAIN_HTTP_KEY => "key_over_plain_http",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // TEST: GW-I-323
    #[test]
    fn a_refusal_is_counted_and_the_least_recent_goes_past_the_bound() {
        let recent = Recent::default();
        recent.record("a.example", 443, "not_listed");
        recent.record("a.example", 443, "not_listed");
        recent.record("b.example", 80, "private");
        let items = recent.list();
        assert_eq!(items[0].0.host, "b.example", "the most recent first");
        assert_eq!(items[1].1.count, 2);
        for n in 0..RECENT_REFUSALS {
            recent.record(&format!("h{n}.example"), 443, "not_listed");
        }
        let items = recent.list();
        assert_eq!(items.len(), RECENT_REFUSALS);
        assert!(
            items.iter().all(|(key, _)| key.host != "a.example"),
            "the least recent went first"
        );
    }

    #[test]
    fn every_reason_has_its_code() {
        use super::super::refusal::{
            DISABLED, INTERNAL_FORBIDDEN, INTERNAL_UNCONFIGURED, INTERNAL_UNVERIFIED, NOT_LISTED,
            PLAIN_HTTP_KEY, PRIVATE,
        };
        for reason in [
            NOT_LISTED,
            PRIVATE,
            DISABLED,
            INTERNAL_FORBIDDEN,
            INTERNAL_UNCONFIGURED,
            INTERNAL_UNVERIFIED,
            PLAIN_HTTP_KEY,
        ] {
            assert!(code(reason).is_some(), "{reason}");
        }
        assert_eq!(code("anything else"), None);
    }
}
