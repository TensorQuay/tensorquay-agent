//! GW-U-226: a relayed response the relay ends early is logged with why, its route, the bytes it carried and how long
//! it ran. The runtime only sees its read fail (`stream_cut`); on 19 Nov 2026 a server that held a long tool call back
//! was cut at the idle limit with nothing in the log.
use super::ended;
use std::{
    io,
    sync::{Arc, Mutex},
    time::Duration,
};

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

// TEST: GW-U-226
#[test]
fn gw_u_226_an_early_end_is_logged_with_its_reason_route_bytes_and_time()
-> Result<(), Box<dyn std::error::Error>> {
    let capture = Capture::default();
    let writer = capture.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        ended(
            "local",
            "relay response idle deadline",
            48_213,
            Duration::from_secs(243),
        );
    });
    let text = String::from_utf8(capture.0.lock().map_err(|_| "capture lock")?.clone())?;
    for expected in [
        "WARN",
        "relayed response ended early",
        "route=local",
        "relay response idle deadline",
        "received_bytes=48213",
        "elapsed_seconds=243",
    ] {
        assert!(text.contains(expected), "{expected} in {text}");
    }
    Ok(())
}

// TEST: GW-U-241
// Decided on 21 Nov 2026: a server may send a tool call whole after a long silence. With the default idle limit
// (300 s) a silence longer than the old 60 s and shorter than 300 s completes; one past 300 s is cut with the same
// words as before, which the "ended early" line logs. A title request and a route check keep their 60 s.
#[tokio::test(start_paused = true)]
async fn gw_u_241_a_long_silence_completes_and_one_past_the_limit_is_cut()
-> Result<(), Box<dyn std::error::Error>> {
    use futures::StreamExt as _;
    let config = super::super::config::RelayConfig::default();
    assert_eq!(config.idle_timeout, 300);
    let idle = Duration::from_secs(config.idle_timeout);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(config.response_timeout);
    let cancel = tokio_util::sync::CancellationToken::new();
    let after = |seconds| {
        futures::stream::once(async move {
            tokio::time::sleep(Duration::from_secs(seconds)).await;
            "the tool call, whole"
        })
        .boxed()
    };
    let mut held_back = after(250);
    assert_eq!(
        super::next_within(&mut held_back, (idle, deadline), &cancel).await?,
        Some("the tool call, whole")
    );
    let mut too_long = after(301);
    let cut = super::next_within(&mut too_long, (idle, deadline), &cancel).await;
    assert_eq!(
        cut.map_err(|error| error.to_string()),
        Err("relay response idle deadline".to_owned())
    );
    // The checks keep the old bound, and a lower setting still applies to them.
    assert_eq!(config.check_idle(), Duration::from_mins(1));
    let lower = super::super::config::RelayConfig {
        idle_timeout: 20,
        ..config
    };
    assert_eq!(lower.check_idle(), Duration::from_secs(20));
    Ok(())
}
