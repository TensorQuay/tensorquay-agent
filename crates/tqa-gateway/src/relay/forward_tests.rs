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
