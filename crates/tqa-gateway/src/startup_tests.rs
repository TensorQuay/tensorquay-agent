//! GW-U-071: a start-up step that fails is logged at ERROR with its name and cause, and the sandbox backend's connect
//! is retried on a timeout only, with capped backoff, until its start deadline.
use super::{BACKEND_DEADLINE, failed, retry_timeouts};
use crate::GatewayError;
use std::{
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};
use tqa_sandbox::SandboxError;

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

fn logged(error: GatewayError) -> Result<(GatewayError, String), Box<dyn std::error::Error>> {
    let capture = Capture::default();
    let writer = capture.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    let mapped = tracing::subscriber::with_default(subscriber, || failed("sandbox backend", error));
    let text = String::from_utf8(capture.0.lock().map_err(|_| "capture lock")?.clone())?;
    Ok((mapped, text))
}

#[test]
fn gw_u_071_every_failed_step_is_logged_at_error_with_its_name_and_cause()
-> Result<(), Box<dyn std::error::Error>> {
    // The Gate 8 start-up exit: a backend timeout became a sandbox error, and nothing but a WARN said why it stopped.
    let (mapped, text) = logged(GatewayError::Sandbox {
        stage: "configure",
        cause: "timeout",
    })?;
    assert_eq!(
        mapped,
        GatewayError::Sandbox {
            stage: "configure",
            cause: "timeout"
        }
    );
    for expected in [
        "ERROR",
        "sandbox backend",
        "sandbox configure failed: timeout",
    ] {
        assert!(text.contains(expected), "{text}");
    }
    // A timeout still names its step in the exit error (GW-U-060).
    let (mapped, text) = logged(GatewayError::Timeout)?;
    assert_eq!(mapped, GatewayError::StartupTimeout("sandbox backend"));
    assert!(
        text.contains("ERROR") && text.contains("sandbox backend"),
        "{text}"
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn gw_u_071_a_timed_out_connect_is_retried_until_it_succeeds() {
    let attempts = AtomicU32::new(0);
    let started = tokio::time::Instant::now();
    let result = retry_timeouts(BACKEND_DEADLINE, |_| {
        let attempt = attempts.fetch_add(1, Ordering::SeqCst);
        std::future::ready(if attempt < 3 {
            Err(SandboxError::Timeout)
        } else {
            Ok(attempt)
        })
    })
    .await;
    assert_eq!(result, Ok(3));
    // Backoff 1, 2, 4 s between the four attempts.
    assert_eq!(started.elapsed(), Duration::from_secs(7));
}

#[tokio::test(start_paused = true)]
async fn gw_u_071_other_errors_fail_at_once() {
    for error in [
        SandboxError::Preflight("storage.root is missing"),
        SandboxError::Backend("the Docker Engine is unavailable".into()),
    ] {
        let (attempts, elapsed, result) = failing_with(error.clone()).await;
        assert_eq!(result, Err(error));
        assert_eq!(attempts, 1);
        assert_eq!(elapsed, Duration::ZERO);
    }
}

/// Retries a connect that always fails with `error`: the attempts made, the time taken and the result.
async fn failing_with(error: SandboxError) -> (u32, Duration, Result<(), SandboxError>) {
    let attempts = AtomicU32::new(0);
    let started = tokio::time::Instant::now();
    let result = retry_timeouts(BACKEND_DEADLINE, |_| {
        attempts.fetch_add(1, Ordering::SeqCst);
        std::future::ready(Err(error.clone()))
    })
    .await;
    (attempts.load(Ordering::SeqCst), started.elapsed(), result)
}

#[tokio::test(start_paused = true)]
async fn gw_u_071_retries_stop_at_the_deadline_with_the_timeout() {
    let attempts = AtomicU32::new(0);
    let started = tokio::time::Instant::now();
    let result: Result<(), _> = retry_timeouts(BACKEND_DEADLINE, |left| {
        attempts.fetch_add(1, Ordering::SeqCst);
        async move {
            // Each attempt spends the engine's whole 10 s bound, cut to what is left, as on a starved engine.
            tokio::time::sleep(Duration::from_secs(10).min(left)).await;
            Err(SandboxError::Timeout)
        }
    })
    .await;
    assert_eq!(result, Err(SandboxError::Timeout));
    assert!(attempts.load(Ordering::SeqCst) >= 3, "{attempts:?}");
    // It gives up inside the deadline, before the image's health check (a 30 s start period plus 12 checks 5 s
    // apart) and compose's 120 s wait end.
    assert!(
        started.elapsed() <= BACKEND_DEADLINE,
        "{:?}",
        started.elapsed()
    );
    assert!(BACKEND_DEADLINE < Duration::from_mins(1) + Duration::from_secs(30));
}

#[tokio::test(start_paused = true)]
async fn gw_u_071_the_backoff_is_capped() {
    let attempts = AtomicU32::new(0);
    let started = tokio::time::Instant::now();
    let _: Result<(), _> = retry_timeouts(Duration::from_mins(10), |_| {
        attempts.fetch_add(1, Ordering::SeqCst);
        async { Err(SandboxError::Timeout) }
    })
    .await;
    // Waits 1, 2, 4, 8, then 10 s each: past the first few, one attempt per 10 s, never a longer silence.
    let n = attempts.load(Ordering::SeqCst);
    assert!(n >= 60, "{n}");
    assert!(started.elapsed() <= Duration::from_mins(10));
}

// TEST: GW-U-123
#[tokio::test(start_paused = true)]
async fn gw_u_123_each_attempt_is_cut_to_what_is_left_and_the_first_fits_a_full_create() {
    let mut given = Vec::new();
    let started = tokio::time::Instant::now();
    let result: Result<(), _> = retry_timeouts(BACKEND_DEADLINE, |left| {
        given.push(left);
        // A connect whose container creates take their whole bound, the create bound cut to what is left.
        let bound = Duration::from_mins(1).min(left);
        async move {
            tokio::time::sleep(bound).await;
            Err(SandboxError::Timeout)
        }
    })
    .await;
    assert_eq!(result, Err(SandboxError::Timeout));
    assert_eq!(given.first(), Some(&BACKEND_DEADLINE), "{given:?}");
    // One full create (60 s) fits the first attempt: a loaded engine's 44 s create starts the gateway.
    assert!(BACKEND_DEADLINE >= Duration::from_mins(1));
    assert!(given.len() >= 2, "{given:?}");
    assert!(given.windows(2).all(|pair| pair[1] < pair[0]), "{given:?}");
    assert!(
        started.elapsed() <= BACKEND_DEADLINE,
        "{:?}",
        started.elapsed()
    );
}
