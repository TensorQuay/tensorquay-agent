//! LLM-E-007/008: observed connection failures retry; permanent classes fail once.
#[path = "fixtures/http.rs"]
#[allow(
    dead_code,
    reason = "Shared request fixtures expose helpers used by other integration tests."
)]
mod fixtures;
#[path = "fixtures/transport.rs"]
mod transport;

use std::{
    error::Error,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use tqa_llm::{
    HttpConfig, LlmError, RetryPolicy, StreamLimits, SystemClock, TokioSleeper, collect,
    provider_for, retry,
};
use tqa_testkit::FixedJitter;
use tqa_types::Sleeper;
use transport::{Fault, Transport};

#[derive(Default)]
struct RetrySleeper(Mutex<Vec<Duration>>);
impl Sleeper for RetrySleeper {
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(duration);
        Box::pin(std::future::ready(()))
    }
}

async fn run_fault(
    fault: Fault,
) -> Result<
    (
        Result<tqa_llm::Completion, LlmError>,
        usize,
        Vec<Duration>,
        usize,
    ),
    Box<dyn Error>,
> {
    let transport = Arc::new(Mutex::new(Transport::new(fault)?));
    let url = transport
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .url
        .clone();
    let connections = transport
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .connections
        .clone();
    let mut config = HttpConfig::new(StreamLimits::RECOMMENDED);
    config.body_cap = 32;
    let provider = provider_for(
        fixtures::route(&url)?,
        None,
        config,
        Arc::new(SystemClock),
        Arc::new(TokioSleeper),
    )?;
    let sleeper = RetrySleeper::default();
    let attempts = AtomicUsize::new(0);
    let policy = RetryPolicy::new(3, Duration::from_secs(2), Duration::from_secs(30), true)?;
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        retry(
            &policy,
            &SystemClock,
            &sleeper,
            &mut FixedJitter::new(0.5)?,
            &CancellationToken::new(),
            || async {
                attempts.fetch_add(1, Ordering::SeqCst);
                let result = provider
                    .complete(fixtures::request(), CancellationToken::new())
                    .await;
                transport
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .recover_refused()
                    .map_err(|error| LlmError::Io(error.to_string()))?;
                collect(result?, StreamLimits::RECOMMENDED).await
            },
        ),
    )
    .await;
    let mut fixture = Arc::try_unwrap(transport)
        .map_err(|_| "fixture still shared")?
        .into_inner()?;
    fixture.close().await?;
    Ok((
        result?,
        attempts.load(Ordering::SeqCst),
        sleeper.0.into_inner()?,
        connections.load(Ordering::SeqCst),
    ))
}

async fn assert_retry(fault: Fault) -> Result<(), Box<dyn Error>> {
    let (result, attempts, delays, connections) = run_fault(fault).await?;
    assert!(result.is_ok(), "{fault:?}: {result:?}");
    assert_eq!(result?.text, "shared prefix complete");
    assert_eq!(attempts, 2, "{fault:?}");
    assert_eq!(delays.len(), 1);
    assert_eq!(
        connections,
        if matches!(fault, Fault::Refused) {
            1
        } else {
            2
        }
    );
    Ok(())
}

#[tokio::test]
async fn llm_e_007_refused_before_headers_retries_then_succeeds() -> Result<(), Box<dyn Error>> {
    assert_retry(Fault::Refused).await
}

#[tokio::test]
async fn llm_e_007_reset_before_headers_retries_then_succeeds() -> Result<(), Box<dyn Error>> {
    assert_retry(Fault::Reset).await
}

#[tokio::test]
async fn llm_e_007_closed_before_headers_retries_then_succeeds() -> Result<(), Box<dyn Error>> {
    assert_retry(Fault::Closed).await
}

#[tokio::test]
async fn llm_e_007_mid_stream_cut_retry_returns_only_the_complete_attempt()
-> Result<(), Box<dyn Error>> {
    let (result, attempts, delays, connections) = run_fault(Fault::StreamCut).await?;
    assert_eq!(result?.text, "shared prefix complete");
    assert_eq!((attempts, delays.len(), connections), (2, 1, 2));
    Ok(())
}

#[tokio::test]
async fn llm_e_008_protocol_tls_and_body_limit_errors_never_retry() -> Result<(), Box<dyn Error>> {
    for (fault, code) in [
        (Fault::Malformed, "io"),
        (Fault::Tls, "io"),
        (Fault::BodyLimit, "body_too_large"),
    ] {
        let (result, attempts, delays, connections) = run_fault(fault).await?;
        let error = result.err().ok_or("accepted a permanent failure")?;
        assert_eq!(error.code(), code);
        assert!(!error.retryable());
        assert_eq!((attempts, delays.len(), connections), (1, 0, 1));
    }
    Ok(())
}
