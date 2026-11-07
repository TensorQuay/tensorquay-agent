//! DNS resolution is an injected permanent failure, without an external name server.
use super::*;
use std::{
    io,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
struct FixedJitter;
impl crate::Jitter for FixedJitter {
    fn next_fraction(&mut self) -> f64 {
        0.5
    }
}

struct FailedDns {
    calls: AtomicUsize,
    kind: io::ErrorKind,
}
impl reqwest::dns::Resolve for FailedDns {
    fn resolve(&self, _name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let kind = self.kind;
        Box::pin(
            async move { Err(io::Error::new(kind, "synthetic DNS resolution failure").into()) },
        )
    }
}

#[tokio::test]
async fn llm_e_008_dns_failure_is_permanent_at_the_real_send_boundary()
-> Result<(), Box<dyn std::error::Error>> {
    for kind in [
        io::ErrorKind::Other,
        io::ErrorKind::ConnectionRefused,
        io::ErrorKind::TimedOut,
    ] {
        assert_permanent_dns(kind).await?;
    }
    Ok(())
}

async fn assert_permanent_dns(kind: io::ErrorKind) -> Result<(), Box<dyn std::error::Error>> {
    let route = crate::Registry::from_toml("[[routes]]\nname='test'\nprotocol='openai_chat'\nbase_url='http://models.invalid'\nmodel='test'\nauth={kind='none'}\n")?
        .get("test").cloned().ok_or("missing test route")?;
    let resolver = Arc::new(FailedDns {
        calls: AtomicUsize::new(0),
        kind,
    });
    let mut provider = ChatProvider::new(
        route,
        None,
        HttpConfig::new(StreamLimits::RECOMMENDED),
        Arc::new(SystemClock),
        Arc::new(TokioSleeper),
    )?;
    provider.transport.client = http::client(provider.transport.http, resolver.clone())?;
    let request = CompletionRequest {
        messages: vec![crate::Message {
            role: crate::Role::User,
            content: vec![crate::ContentPart::Text("question".into())],
            tool_calls: vec![],
            tool_call_id: None,
            name: None,
        }],
        ..CompletionRequest::default()
    };
    let error = provider
        .complete(request, CancellationToken::new())
        .await
        .err()
        .ok_or("DNS failure succeeded")?;
    assert_eq!(error.code(), "io");
    assert!(!error.retryable());
    let policy = crate::RetryPolicy::new(3, Duration::from_secs(2), Duration::from_secs(30), true)?;
    assert_eq!(policy.next_delay(1, &error, &mut FixedJitter)?, None);
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 1);
    Ok(())
}

async fn relay_dns_failure(
    kind: io::ErrorKind,
) -> Result<(LlmError, usize), Box<dyn std::error::Error>> {
    let route = crate::Registry::from_toml("[[routes]]\nname='test'\nprotocol='openai_chat'\nbase_url='http://gateway.invalid'\nmodel='test'\nauth={kind='none'}\n")?
        .get("test").cloned().ok_or("missing test route")?;
    let resolver = Arc::new(FailedDns {
        calls: AtomicUsize::new(0),
        kind,
    });
    let mut http = HttpConfig::new(StreamLimits::RECOMMENDED);
    http.peer = crate::Peer::Relay;
    let mut provider = ChatProvider::new(
        route,
        None,
        http,
        Arc::new(SystemClock),
        Arc::new(TokioSleeper),
    )?;
    provider.transport.client = http::client(provider.transport.http, resolver.clone())?;
    let request = CompletionRequest {
        messages: vec![crate::Message {
            role: crate::Role::User,
            content: vec![crate::ContentPart::Text("question".into())],
            tool_calls: vec![],
            tool_call_id: None,
            name: None,
        }],
        ..CompletionRequest::default()
    };
    let error = provider
        .complete(request, CancellationToken::new())
        .await
        .err()
        .ok_or("DNS failure succeeded")?;
    Ok((error, resolver.calls.load(Ordering::SeqCst)))
}

// TEST: LLM-E-029
#[tokio::test]
async fn llm_e_029_a_relay_call_retries_a_failed_lookup() -> Result<(), Box<dyn std::error::Error>>
{
    // The gateway's name doesn't resolve while its container restarts; the runtime's call through the relay retries
    // it within the same bounded policy. The same failure on a provider's host stays permanent (LLM-E-008).
    for kind in [
        io::ErrorKind::Other,
        io::ErrorKind::ConnectionRefused,
        io::ErrorKind::TimedOut,
    ] {
        let (error, calls) = relay_dns_failure(kind).await?;
        assert!(
            matches!(
                error,
                LlmError::Connection(crate::ConnectionFailure::Unresolved)
            ),
            "{kind:?}: {error:?}"
        );
        assert_eq!(error.code(), "io");
        assert!(error.retryable(), "{kind:?}");
        let policy =
            crate::RetryPolicy::new(3, Duration::from_secs(2), Duration::from_secs(30), true)?;
        assert!(
            policy.next_delay(1, &error, &mut FixedJitter)?.is_some(),
            "{kind:?}"
        );
        assert_eq!(
            policy.next_delay(3, &error, &mut FixedJitter)?,
            None,
            "{kind:?}: bounded"
        );
        assert_eq!(calls, 1);
    }
    Ok(())
}

// TEST: LLM-E-029
#[test]
fn llm_e_029_an_unreachable_host_is_a_connection_failure_only_for_the_relay() {
    use crate::{ConnectionFailure, Peer};
    for (kind, relay) in [
        (
            io::ErrorKind::ConnectionRefused,
            Some(ConnectionFailure::Refused),
        ),
        (
            io::ErrorKind::ConnectionReset,
            Some(ConnectionFailure::Reset),
        ),
        (io::ErrorKind::BrokenPipe, Some(ConnectionFailure::Closed)),
        (
            io::ErrorKind::HostUnreachable,
            Some(ConnectionFailure::Unreachable),
        ),
        (
            io::ErrorKind::NetworkUnreachable,
            Some(ConnectionFailure::Unreachable),
        ),
        (io::ErrorKind::PermissionDenied, None),
        (io::ErrorKind::Other, None),
    ] {
        assert_eq!(
            http::connection_failure(kind, Peer::Relay),
            relay,
            "{kind:?}"
        );
        let provider = relay.filter(|failure| *failure != ConnectionFailure::Unreachable);
        assert_eq!(
            http::connection_failure(kind, Peer::Provider),
            provider,
            "{kind:?}"
        );
    }
    assert!(LlmError::Connection(ConnectionFailure::Unreachable).retryable());
    assert!(LlmError::Connection(ConnectionFailure::Unresolved).retryable());
}

/// The first test that needs it installs [`Silent`] as the global subscriber for the WHOLE tqa-llm lib test binary, and
/// a process has only one: a test that wants its own global subscriber here must build on this one instead.
static SILENT: std::sync::Once = std::sync::Once::new();

/// Records nothing, but keeps every call site's interest at "sometimes", so a thread's own subscriber decides.
struct Silent;
impl tracing::Subscriber for Silent {
    fn register_callsite(
        &self,
        _: &'static tracing::Metadata<'static>,
    ) -> tracing::subscriber::Interest {
        tracing::subscriber::Interest::sometimes()
    }
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        false
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, _: &tracing::Event<'_>) {}
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

/// Collects every event's fields, so a test can read what a failure logged.
struct Recorder(Arc<std::sync::Mutex<String>>);
impl tracing::Subscriber for Recorder {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        struct Fields<'a>(&'a mut String);
        impl tracing::field::Visit for Fields<'_> {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                let _ = std::fmt::Write::write_fmt(self.0, format_args!("{field}={value:?} "));
            }
        }
        let mut output = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        event.record(&mut Fields(&mut output));
        output.push('\n');
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

// TEST: LLM-E-029
#[tokio::test]
async fn llm_e_029_a_failure_before_headers_logs_its_class()
-> Result<(), Box<dyn std::error::Error>> {
    let output = Arc::new(std::sync::Mutex::new(String::new()));
    // Tracing caches each call site's interest for the whole process. Other tests hit this log line at the same time
    // with no subscriber, and a first hit from such a thread could cache "never", so this recorder never saw the event
    // (the flake of 29 Sep). A silent global subscriber answers "sometimes" for every call site, so each event asks the
    // current thread's subscriber: the recorder here, nothing elsewhere.
    SILENT.call_once(|| {
        let _ = tracing::subscriber::set_global_default(Silent);
    });
    let _guard = tracing::subscriber::set_default(Recorder(output.clone()));
    tracing::callsite::rebuild_interest_cache();
    relay_dns_failure(io::ErrorKind::Other).await?;
    let logged = output
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    // Our own line names the class and never the URL (the HTTP library's debug lines are its own).
    let ours = logged
        .lines()
        .find(|line| line.contains("model request failed before response headers"))
        .ok_or("no failure line")?;
    assert!(ours.contains("class=\"unresolved\""), "{ours}");
    assert!(!ours.contains("gateway.invalid"), "{ours}");
    Ok(())
}
