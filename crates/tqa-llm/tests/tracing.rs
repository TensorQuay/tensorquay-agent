//! LLM-C-019 and LLM-E-016: diagnostics never leak credentials into tracing.

use std::{
    error::Error,
    fmt::{Debug, Write as _},
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use tqa_llm::{LlmError, RetryPolicy, retry};
use tqa_testkit::{FakeClock, FakeSleeper, FixedJitter};
use tracing::{
    Event, Metadata, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
};

struct Recorder(Arc<Mutex<String>>);
struct Visitor<'a>(&'a mut String);

impl Visit for Visitor<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        assert!(writeln!(self.0, "{field}={value:?}").is_ok());
    }
}

impl Subscriber for Recorder {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, _: &Record<'_>) {}
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, event: &Event<'_>) {
        event.record(&mut Visitor(
            &mut self.0.lock().unwrap_or_else(PoisonError::into_inner),
        ));
    }
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

#[test]
fn llm_c_019_retry_traces_and_error_fields_are_redacted() -> Result<(), Box<dyn Error>> {
    let output = Arc::new(Mutex::new(String::new()));
    let clock = FakeClock::new();
    let sleeper = FakeSleeper::new(clock.clone());
    let policy = RetryPolicy::new(2, Duration::from_secs(1), Duration::from_secs(1), true)?;
    let mut jitter = FixedJitter::new(0.0)?;
    tracing::subscriber::with_default(Recorder(output.clone()), || {
        let cancel = CancellationToken::new();
        let mut task = tokio_test::task::spawn(retry(
            &policy,
            &clock,
            &sleeper,
            &mut jitter,
            &cancel,
            || {
                std::future::ready(Err::<(), _>(LlmError::Http {
                    status: 503,
                    body_excerpt: "x-api-key: example-value".into(),
                }))
            },
        ));
        assert!(task.poll().is_pending());
        assert_eq!(sleeper.durations(), [Duration::from_millis(500)]);
        clock.advance(Duration::from_millis(500))?;
        let std::task::Poll::Ready(Err(error)) = task.poll() else {
            return Err("retry did not exhaust at its fake-clock deadline".into());
        };
        tracing::warn!(error = %error, "completion failed");
        Ok::<_, Box<dyn Error>>(())
    })?;
    let output = output.lock().unwrap_or_else(PoisonError::into_inner);
    assert!(output.contains("completion attempt"));
    assert!(output.contains("completion retry waiting"));
    assert!(output.contains("[redacted]"));
    assert!(!output.contains("example-value"));
    Ok(())
}

#[test]
fn llm_e_016_authorization_is_redacted_from_debug_and_display_logs() {
    for header in ["Authorization", "authorization", "AUTHORIZATION"] {
        let output = Arc::new(Mutex::new(String::new()));
        let error = LlmError::Http {
            status: 401,
            body_excerpt: format!("{header}: Bearer synthetic-credential\nrequest refused"),
        };
        tracing::subscriber::with_default(Recorder(output.clone()), || {
            tracing::debug!(debug_error = ?error, display_error = %error, "request failed");
        });
        let output = output.lock().unwrap_or_else(PoisonError::into_inner);
        assert!(output.contains("debug_error="));
        assert!(output.contains("display_error="));
        assert_eq!(output.matches("[redacted]").count(), 2);
        assert!(output.contains("request refused"));
        assert!(!output.contains("synthetic-credential"), "header: {header}");
    }
}
