//! GW-U-106 (ADR-0037 §4, point 5): a placeholder never goes over plain http, in any spelling the head can carry.
//! GW-U-206: a tunnel closes after `[egress] idle_seconds` without traffic, never while bytes move.
use super::{copy_until_idle, holds_placeholder};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

type TestResult = Result<(), Box<dyn std::error::Error>>;

// TEST: GW-U-106
/// A plain-http head holding a placeholder's mark is caught raw, percent-encoded and percent-encoded twice, in either
/// case of hex (the second review's L4; `%25` is suspect, as in a prefixed path); a head without one isn't.
#[test]
fn gw_u_106_a_placeholder_over_plain_http_is_caught_in_any_spelling() {
    for head in [
        "GET http://example.test/?k=tqa_ph_abc HTTP/1.1\r\nHost: example.test\r\n\r\n",
        "GET http://example.test/?k=%74qa_ph_abc HTTP/1.1\r\nHost: example.test\r\n\r\n",
        "GET http://example.test/?k=tqa%5Fph%5Fabc HTTP/1.1\r\nHost: example.test\r\n\r\n",
        "GET http://example.test/ HTTP/1.1\r\nHost: example.test\r\nX-K: tqa%5fph%5fabc\r\n\r\n",
        "GET http://example.test/?k=%2574qa_ph_abc HTTP/1.1\r\nHost: example.test\r\n\r\n",
        "GET http://example.test/?k=tqa%255Fph%255fabc HTTP/1.1\r\nHost: example.test\r\n\r\n",
    ] {
        assert!(holds_placeholder(head.as_bytes()), "{head}");
    }
    for clean in [
        "GET http://example.test/?k=tqa-ph HTTP/1.1\r\nHost: example.test\r\n\r\n",
        "GET http://example.test/?k=%25%35Fph HTTP/1.1\r\nHost: example.test\r\n\r\n",
    ] {
        assert!(!holds_placeholder(clean.as_bytes()), "{clean}");
    }
}

// TEST: GW-U-206
/// A tunnel that keeps moving bytes, either way, stays open well past the idle bound; once nothing moves for the
/// bound it ends with `TimedOut`, and not before (a review before 17.0: the whole tunnel was cut at 300 s, traffic or
/// not, so a long download through the proxy failed).
#[tokio::test(start_paused = true)]
async fn gw_u_206_a_busy_tunnel_outlives_the_idle_bound_and_an_idle_one_ends_at_it() -> TestResult {
    let idle = Duration::from_mins(5);
    let (mut sandbox, mut near) = tokio::io::duplex(64);
    let (mut far, mut host) = tokio::io::duplex(64);
    let copy = tokio::spawn(async move { copy_until_idle(&mut near, &mut far, idle).await });
    let mut byte = [0_u8; 1];
    // Twenty minutes of a byte a minute, out and back by turns: four idle bounds.
    for minute in 0..20_u8 {
        tokio::time::sleep(Duration::from_mins(1)).await;
        if minute % 2 == 0 {
            sandbox.write_all(&[minute]).await?;
            host.read_exact(&mut byte).await?;
        } else {
            host.write_all(&[minute]).await?;
            sandbox.read_exact(&mut byte).await?;
        }
        assert_eq!(byte, [minute]);
        assert!(!copy.is_finished(), "cut after minute {minute}");
    }
    // Quiet: still open a second short of the bound, ended by it.
    tokio::time::sleep(idle.saturating_sub(Duration::from_secs(1))).await;
    assert!(!copy.is_finished(), "ended before the idle bound");
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(copy.is_finished(), "still open past the idle bound");
    let ended = copy.await?;
    assert_eq!(
        ended.err().map(|error| error.kind()),
        Some(std::io::ErrorKind::TimedOut)
    );
    Ok(())
}

// TEST: GW-U-206
/// A side that closes ends the copy at once with what moved each way, without waiting for the idle bound.
#[tokio::test(start_paused = true)]
async fn gw_u_206_a_closed_tunnel_ends_at_once() -> TestResult {
    let (mut sandbox, mut near) = tokio::io::duplex(64);
    let (mut far, mut host) = tokio::io::duplex(64);
    let started = tokio::time::Instant::now();
    let copy =
        tokio::spawn(
            async move { copy_until_idle(&mut near, &mut far, Duration::from_mins(5)).await },
        );
    sandbox.write_all(b"abc").await?;
    let mut sent = [0_u8; 3];
    host.read_exact(&mut sent).await?;
    host.write_all(b"de").await?;
    let mut back = [0_u8; 2];
    sandbox.read_exact(&mut back).await?;
    sandbox.shutdown().await?;
    host.shutdown().await?;
    assert_eq!(copy.await??, (3, 2));
    assert_eq!((&sent, &back), (b"abc", b"de"));
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );
    Ok(())
}
