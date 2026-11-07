//! GW-U-042: MCP session binding through the proxy (ADR-0031 decision 4).
use std::time::Duration;

use tokio::time::Instant;

use super::{Found, MAX_SESSIONS_PER_CONVERSATION, SESSION_IDLE, Sessions};
use crate::connectors::CredentialSource;

fn conversation(n: u128) -> uuid::Uuid {
    uuid::Uuid::from_u128(n)
}

#[test]
fn gw_u_042_a_session_is_found_only_by_its_conversation_and_connector() {
    let now = Instant::now();
    let mut sessions = Sessions::default();
    let shared = Some(CredentialSource::Shared);
    let id = sessions.open(
        ("opaque-1".into(), "remote-1".into()),
        (conversation(1), "tracker"),
        shared,
        now,
    );
    assert_eq!(id, "opaque-1");
    assert_eq!(
        sessions.find(&id, (conversation(1), "tracker"), shared, now),
        Found::Remote("remote-1".into())
    );
    assert_eq!(
        sessions.find(&id, (conversation(2), "tracker"), shared, now),
        Found::Unknown,
        "another conversation"
    );
    assert_eq!(
        sessions.find(&id, (conversation(1), "wiki"), shared, now),
        Found::Unknown,
        "another connector"
    );
    assert_eq!(
        sessions.find("guess", (conversation(1), "tracker"), shared, now),
        Found::Unknown
    );
    assert_eq!(
        sessions.find(&id, (conversation(1), "tracker"), shared, now),
        Found::Remote("remote-1".into()),
        "a stranger's attempt leaves the owner's session alone"
    );
}

#[test]
fn gw_u_042_a_changed_credential_source_ends_the_session() {
    let now = Instant::now();
    let mut sessions = Sessions::default();
    let id = sessions.open(
        ("s".into(), "r".into()),
        (conversation(1), "tracker"),
        Some(CredentialSource::Shared),
        now,
    );
    assert_eq!(
        sessions.find(
            &id,
            (conversation(1), "tracker"),
            Some(CredentialSource::User),
            now
        ),
        Found::Unknown
    );
    assert_eq!(
        sessions.find(
            &id,
            (conversation(1), "tracker"),
            Some(CredentialSource::Shared),
            now
        ),
        Found::Unknown,
        "gone, not merely refused once"
    );
    let open = sessions.open(
        ("n".into(), "r2".into()),
        (conversation(1), "tracker"),
        None,
        now,
    );
    assert_eq!(
        sessions.find(
            &open,
            (conversation(1), "tracker"),
            Some(CredentialSource::User),
            now
        ),
        Found::Unknown
    );
}

#[test]
fn gw_u_042_idle_sessions_expire_unless_a_stream_is_open() {
    let start = Instant::now();
    let mut sessions = Sessions::default();
    let key = (conversation(1), "tracker");
    let quiet = sessions.open(("quiet".into(), "r1".into()), key, None, start);
    let used = sessions.open(("used".into(), "r2".into()), key, None, start);
    let streaming = sessions.open(("streaming".into(), "r3".into()), key, None, start);
    assert!(sessions.start_stream(&streaming));
    assert!(!sessions.start_stream(&streaming), "one stream per session");
    let later = start + SESSION_IDLE - Duration::from_secs(1);
    assert_eq!(
        sessions.find(&used, key, None, later),
        Found::Remote("r2".into()),
        "use refreshes it"
    );
    let expired = start + SESSION_IDLE;
    assert_eq!(sessions.find(&quiet, key, None, expired), Found::Unknown);
    assert_eq!(
        sessions.find(&used, key, None, expired),
        Found::Remote("r2".into())
    );
    assert_eq!(
        sessions.find(&streaming, key, None, expired),
        Found::Remote("r3".into()),
        "streaming is in use"
    );
    sessions.end_stream(&streaming);
    assert!(
        sessions.start_stream(&streaming),
        "a new stream after the last one ended"
    );
    sessions.end_stream("absent");
    assert!(!sessions.start_stream("absent"));
}

#[test]
fn gw_u_042_a_conversation_holds_at_most_eight_sessions_and_loses_the_least_recently_used() {
    let start = Instant::now();
    let mut sessions = Sessions::default();
    let mine = (conversation(1), "tracker");
    for index in 0..MAX_SESSIONS_PER_CONVERSATION {
        let at = start + Duration::from_secs(u64::try_from(index).unwrap_or(0));
        sessions.open((format!("s{index}"), format!("r{index}")), mine, None, at);
    }
    let other = sessions.open(
        ("theirs".into(), "rx".into()),
        (conversation(2), "tracker"),
        None,
        start,
    );
    let now = start + Duration::from_mins(1);
    // Using the oldest makes the second-oldest the least recently used.
    assert_eq!(
        sessions.find("s0", mine, None, now),
        Found::Remote("r0".into())
    );
    sessions.open(("s8".into(), "r8".into()), mine, None, now);
    assert_eq!(
        sessions.len(),
        MAX_SESSIONS_PER_CONVERSATION + 1,
        "eight of mine and one of theirs"
    );
    assert_eq!(
        sessions.find("s1", mine, None, now),
        Found::Unknown,
        "the least recently used went"
    );
    assert_eq!(
        sessions.find("s0", mine, None, now),
        Found::Remote("r0".into())
    );
    assert_eq!(
        sessions.find(&other, (conversation(2), "tracker"), None, now),
        Found::Remote("rx".into()),
        "untouched"
    );
}

#[test]
fn gw_u_042_close_forgets_only_the_callers_session() {
    let now = Instant::now();
    let mut sessions = Sessions::default();
    let id = sessions.open(
        ("s".into(), "r".into()),
        (conversation(1), "tracker"),
        None,
        now,
    );
    assert_eq!(
        sessions.close(&id, (conversation(2), "tracker")),
        Found::Unknown
    );
    assert_eq!(
        sessions.close(&id, (conversation(1), "tracker")),
        Found::Remote("r".into())
    );
    assert_eq!(
        sessions.close(&id, (conversation(1), "tracker")),
        Found::Unknown
    );
}
