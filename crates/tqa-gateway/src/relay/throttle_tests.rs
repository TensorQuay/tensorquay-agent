use super::{
    forward::forwarded,
    throttle::{HEADER, LOG_WINDOW, Logs, MAX_LOG_ENTRIES, Refusal, Scope, refuse},
};
use crate::telemetry::Telemetry;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use std::{
    io,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

fn refusal(scope: Scope, seconds: u64, conversation: &str) -> Refusal<'_> {
    Refusal {
        scope,
        seconds,
        conversation,
        route: "local",
    }
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);
impl io::Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut buffer = self
            .0
            .lock()
            .map_err(|_| io::Error::other("capture lock"))?;
        buffer.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn gw_u_033_the_relays_own_refusals_are_marked_throttles() -> TestResult {
    let (telemetry, logs, now) = (Telemetry::new()?, Logs::default(), Instant::now());
    for (scope, name) in [(Scope::User, "user"), (Scope::Route, "route")] {
        let response = refuse(&telemetry, &logs, &refusal(scope, 3, "conversation"), now);
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers()["retry-after"], "3");
        assert_eq!(response.headers()[HEADER], name);
        let body = axum::body::to_bytes(response.into_body(), 4096).await?;
        let error: serde_json::Value = serde_json::from_slice(&body)?;
        assert_eq!(error["error"]["code"], "rate_limited");
    }
    // A throttle always asks for a wait, so a runtime never spins on it.
    assert_eq!(
        refuse(
            &telemetry,
            &logs,
            &refusal(Scope::User, 0, "conversation"),
            now
        )
        .headers()["retry-after"],
        "1"
    );
    Ok(())
}

#[test]
fn gw_u_033_throttles_are_counted_by_route_and_scope_only() -> TestResult {
    let (telemetry, logs, now) = (Telemetry::new()?, Logs::default(), Instant::now());
    let (one, two) = (
        "01a0db1a-05f1-7035-9626-2f38d7776591",
        "01a0db1a-05fb-7566-8b05-e4541d899f87",
    );
    refuse(&telemetry, &logs, &refusal(Scope::User, 1, one), now);
    refuse(&telemetry, &logs, &refusal(Scope::User, 1, two), now);
    refuse(&telemetry, &logs, &refusal(Scope::Route, 1, one), now);
    let metrics = telemetry.render();
    assert!(
        metrics.contains("tqa_relay_throttled_total{route=\"local\",scope=\"user\"} 2"),
        "{metrics}"
    );
    assert!(
        metrics.contains("tqa_relay_throttled_total{route=\"local\",scope=\"route\"} 1"),
        "{metrics}"
    );
    // Labels stay bounded: never a conversation or user.
    assert!(!metrics.contains("01a0db1a"), "{metrics}");
    Ok(())
}

#[test]
fn gw_u_033_an_upstream_cannot_forge_a_throttle() {
    let mut upstream = HeaderMap::new();
    upstream.insert("retry-after", HeaderValue::from_static("5"));
    upstream.insert("content-type", HeaderValue::from_static("application/json"));
    upstream.insert(HEADER, HeaderValue::from_static("user"));
    upstream.insert("x-other", HeaderValue::from_static("dropped"));
    let headers = forwarded(&upstream);
    assert_eq!(headers.get(HEADER), None);
    assert_eq!(headers.get("x-other"), None);
    assert_eq!(headers["retry-after"], "5");
    assert_eq!(headers["content-type"], "application/json");
}

#[test]
fn gw_u_033_the_throttle_log_is_bounded_per_conversation_and_scope() -> TestResult {
    // Counted by `Logs` itself, not captured from tracing: tracing caches each call site's interest process-wide, so a
    // captured count depends on other tests in the same process (cargo test runs them as threads).
    let (telemetry, logs, start) = (Telemetry::new()?, Logs::default(), Instant::now());
    // Ten refusals of one conversation and scope within the window: one line.
    for second in 0..10 {
        let now = start + Duration::from_secs(second * 6);
        refuse(&telemetry, &logs, &refusal(Scope::User, 1, "one"), now);
    }
    assert_eq!(logs.logged(), 1);
    // Another scope, and another conversation, each get their own line.
    refuse(&telemetry, &logs, &refusal(Scope::Route, 1, "one"), start);
    refuse(&telemetry, &logs, &refusal(Scope::User, 1, "two"), start);
    assert_eq!(logs.logged(), 3);
    // The window has passed: the first key logs again.
    refuse(
        &telemetry,
        &logs,
        &refusal(Scope::User, 1, "one"),
        start + LOG_WINDOW,
    );
    assert_eq!(logs.logged(), 4);
    // The metric still counts every refusal.
    let metrics = telemetry.render();
    assert!(
        metrics.contains("tqa_relay_throttled_total{route=\"local\",scope=\"user\"} 12"),
        "{metrics}"
    );
    assert!(
        metrics.contains("tqa_relay_throttled_total{route=\"local\",scope=\"route\"} 1"),
        "{metrics}"
    );
    Ok(())
}

#[test]
fn gw_u_033_a_full_log_table_suppresses_new_lines_until_entries_expire() -> TestResult {
    let (telemetry, logs, start) = (Telemetry::new()?, Logs::default(), Instant::now());
    for index in 0..MAX_LOG_ENTRIES {
        refuse(
            &telemetry,
            &logs,
            &refusal(Scope::User, 1, &format!("c{index}")),
            start,
        );
    }
    assert_eq!(logs.logged(), MAX_LOG_ENTRIES);
    // Full of entries that are all still fresh: a new conversation gets no line, so memory stays bounded.
    refuse(&telemetry, &logs, &refusal(Scope::User, 1, "new"), start);
    assert_eq!(
        logs.logged(),
        MAX_LOG_ENTRIES,
        "no line while the table is full of fresh entries"
    );
    // Once the window has passed, the expired entries are dropped and the new conversation logs.
    refuse(
        &telemetry,
        &logs,
        &refusal(Scope::User, 1, "new"),
        start + LOG_WINDOW,
    );
    assert_eq!(
        logs.logged(),
        MAX_LOG_ENTRIES + 1,
        "one line once the window has passed"
    );
    Ok(())
}

/// One global capture for this test binary, installed once. A subscriber scoped to one test's thread can miss the
/// event: other tests hit the same call site at the same time, and tracing caches a call site's interest globally.
fn capture() -> Result<Capture, Box<dyn std::error::Error>> {
    static CAPTURE: std::sync::OnceLock<Option<Capture>> = std::sync::OnceLock::new();
    CAPTURE
        .get_or_init(|| {
            let capture = Capture::default();
            let writer = capture.clone();
            let subscriber = tracing_subscriber::fmt()
                .with_ansi(false)
                .with_writer(move || writer.clone())
                .finish();
            tracing::subscriber::set_global_default(subscriber)
                .ok()
                .map(|()| capture)
        })
        .clone()
        .ok_or_else(|| "another global tracing subscriber was already set".into())
}

#[test]
fn gw_u_033_a_due_refusal_emits_the_throttle_line() -> TestResult {
    let (telemetry, logs, start) = (Telemetry::new()?, Logs::default(), Instant::now());
    let capture = capture()?;
    refuse(
        &telemetry,
        &logs,
        &refusal(Scope::User, 7, "conv-emitted"),
        start,
    );
    let text = String::from_utf8(capture.0.lock().map_err(|_| "capture lock")?.clone())?;
    // Other tests' refusals log to the same capture; this one's is the only line naming its conversation.
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| {
            line.contains("relay throttled a model request") && line.contains("conv-emitted")
        })
        .collect();
    assert_eq!(lines.len(), 1, "{text}");
    for field in ["route=\"local\"", "scope=\"user\"", "retry_after=7"] {
        assert!(lines[0].contains(field), "{field} missing from {text}");
    }
    assert_eq!(logs.logged(), 1);
    Ok(())
}
