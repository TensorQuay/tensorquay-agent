use super::{Attempt, FIRST_RETRY, MAX_RETRY, retry_wait, retrying};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;

/// The WARN and INFO lines logged while `work` runs.
async fn logged<F: std::future::Future>(work: F) -> (F::Output, String) {
    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);
    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
        type Writer = Self;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }
    impl std::io::Write for Capture {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if let Ok(mut buffer) = self.0.lock() {
                buffer.extend_from_slice(bytes);
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let capture = Capture::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(capture.clone())
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .finish();
    let output = {
        let _guard = tracing::subscriber::set_default(subscriber);
        work.await
    };
    let text = capture
        .0
        .lock()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    (output, text)
}

/// One listing, counted: the engine doesn't answer the first `unanswered` ones, then answers.
fn listing(calls: &AtomicU32, unanswered: u32) -> std::future::Ready<Attempt> {
    std::future::ready(if calls.fetch_add(1, Ordering::SeqCst) < unanswered {
        Attempt::NoAnswer
    } else {
        Attempt::Done
    })
}

// TEST: GW-U-143
#[test]
fn gw_u_143_the_waits_grow_threefold_with_a_fifth_of_jitter_and_stop_growing_at_the_cap() {
    let s = Duration::from_secs;
    assert_eq!(FIRST_RETRY, s(5));
    assert_eq!(MAX_RETRY, s(120));
    // Middle of the jitter: 5, 15, 45, then capped at 120.
    let middle: Vec<u64> = (1..=6)
        .map(|attempt| retry_wait(attempt, 0.5).as_secs())
        .collect();
    assert_eq!(middle, [5, 15, 45, 120, 120, 120]);
    // The jitter's ends: four fifths to six fifths, never past the cap.
    for (attempt, low, high) in [
        (1, 4.0, 6.0),
        (2, 12.0, 18.0),
        (3, 36.0, 54.0),
        (4, 96.0, 120.0),
        (40, 96.0, 120.0),
    ] {
        for fraction in [0.0, 0.999] {
            let wait = retry_wait(attempt, fraction).as_secs_f64();
            assert!(
                (low..=high).contains(&wait),
                "attempt {attempt}, jitter {fraction}: {wait}"
            );
        }
    }
    // A fraction outside [0, 1) is clamped, never a zero or a huge wait.
    assert!(retry_wait(1, -3.0) >= s(4) && retry_wait(1, f64::NAN) >= s(4));
    assert!(retry_wait(1, 9.0) <= s(6) && retry_wait(u32::MAX, 0.5) == s(120));
}

// TEST: GW-U-143
#[tokio::test(start_paused = true)]
async fn gw_u_143_it_retries_until_the_engine_answers_and_warns_once_each_way() {
    let cancel = CancellationToken::new();
    let calls = Arc::new(AtomicU32::new(0));
    let start = tokio::time::Instant::now();
    let ((result, waited), text) = logged(async {
        let result = retrying(&cancel, || listing(&calls, 4), || 0.5).await;
        (result, start.elapsed())
    })
    .await;
    assert_eq!(
        result,
        Some(4),
        "four failed listings before the one that answered"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 5);
    // 5 + 15 + 45 + 120 s of waits, nothing more.
    assert_eq!(waited.as_secs(), 185);
    assert_eq!(text.matches("didn't answer").count(), 1, "{text}");
    assert_eq!(text.matches("answered again").count(), 1, "{text}");
    assert_eq!(text.lines().count(), 2, "{text}");
}

// TEST: GW-U-143
#[tokio::test(start_paused = true)]
async fn gw_u_143_an_answer_at_once_logs_nothing_and_shutdown_stops_the_retries() {
    let cancel = CancellationToken::new();
    let (result, text) = logged(retrying(&cancel, || async { Attempt::Done }, || 0.5)).await;
    assert_eq!((result, text.as_str()), (Some(0), ""));
    // An engine that never answers: the gateway's shutdown ends the retries between two attempts.
    let calls = Arc::new(AtomicU32::new(0));
    let stopper = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(30)).await;
        stopper.cancel();
    });
    let result = retrying(&cancel, || listing(&calls, u32::MAX), || 0.5).await;
    assert_eq!(result, None, "stopped, not answered");
    // At 0, 5 and 20 s; the next (at 65 s) never comes.
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    // A failure to store ends the retries at once, as a start-up's did.
    let (result, _) = logged(retrying(
        &CancellationToken::new(),
        || async { Attempt::Failed },
        || 0.5,
    ))
    .await;
    assert_eq!(result, None);
}
