//! Denial notices: at most one per destination per conversation per minute, from a bounded table.
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};
use tqa_types::ConversationId;

/// One notice per conversation, host and port in this window (answers-045 Q3).
pub(super) const NOTICE_WINDOW: Duration = Duration::from_mins(1);
/// Entries kept; older ones are dropped first, so a scan of many hosts cannot grow memory.
const MAX_ENTRIES: usize = 4096;

#[derive(Default)]
pub(super) struct Notices(Mutex<HashMap<(ConversationId, String, u16), Instant>>);

impl Notices {
    /// Whether a notice for this destination is due now; records it when it is.
    pub(super) fn due(
        &self,
        conversation: &ConversationId,
        host: &str,
        port: u16,
        now: Instant,
    ) -> bool {
        let Ok(mut sent) = self.0.lock() else {
            return false;
        };
        let key = (conversation.clone(), host.to_owned(), port);
        if sent
            .get(&key)
            .is_some_and(|last| now.duration_since(*last) < NOTICE_WINDOW)
        {
            return false;
        }
        if sent.len() >= MAX_ENTRIES {
            sent.retain(|_, last| now.duration_since(*last) < NOTICE_WINDOW);
            if sent.len() >= MAX_ENTRIES {
                return false;
            }
        }
        sent.insert(key, now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{NOTICE_WINDOW, Notices};
    use std::time::{Duration, Instant};

    #[test]
    fn sec_001_one_denial_notice_per_destination_per_minute()
    -> Result<(), Box<dyn std::error::Error>> {
        let notices = Notices::default();
        let (one, two) = ("one".parse()?, "two".parse()?);
        let start = Instant::now();
        assert!(notices.due(&one, "evil.example", 443, start));
        assert!(
            !notices.due(
                &one,
                "evil.example",
                443,
                (start + NOTICE_WINDOW)
                    .checked_sub(Duration::from_millis(1))
                    .ok_or("instant underflow")?
            )
        );
        assert!(notices.due(&one, "evil.example", 80, start), "another port");
        assert!(
            notices.due(&two, "evil.example", 443, start),
            "another conversation"
        );
        assert!(notices.due(&one, "evil.example", 443, start + NOTICE_WINDOW));
        Ok(())
    }
}
