//! TK-U-035: a scripted reply's `wait_ms` holds the stream before its first record, with keep-alive comments a client's
//! SSE decoder skips, so a stack test can keep a run (its slot, its lease) active and end it with Stop. The bound is
//! 0–600000 ms.
use super::support::*;
use serde_json::{Value, json};
use std::{io::Write, time::Duration};
use tokio::process::Command;

#[tokio::test]
async fn tk_u_035_a_held_reply_starts_after_its_wait_with_only_comments_before() -> TestResult {
    let plain = Binary::start(DEMO).await?;
    let expected = plain.complete(request(0)).await?;
    plain.stop().await?;
    let mut scenario: Value = serde_json::from_str(DEMO)?;
    scenario["responses"][0]["wait_ms"] = json!(1500);
    let held = Binary::start(&scenario.to_string()).await?;
    let started = std::time::Instant::now();
    let wire = held.complete(request(0)).await?;
    assert!(
        started.elapsed() >= Duration::from_millis(1500),
        "the reply waited"
    );
    let (comments, rest) = wire.split_at(wire.find("data:").ok_or("no records")?);
    assert!(
        !comments.is_empty()
            && comments
                .lines()
                .all(|line| line.is_empty() || line.starts_with(':'))
    );
    assert_eq!(
        rest,
        expected.trim_start_matches(|c: char| c != 'd'),
        "the records are the unheld reply's"
    );
    // Only the held turn waits; the next is served at once. A held reply opens with keep-alive comments, so a reply
    // that starts with its first record wasn't held (no wall-clock bound, which a loaded host would break).
    let next = held.complete(request(1)).await?;
    assert!(
        next.starts_with("data:"),
        "the next turn was not held: {next}"
    );
    held.stop().await?;
    Ok(())
}

#[tokio::test]
async fn tk_u_035_the_wait_is_bounded() -> TestResult {
    let mut scenario: Value = serde_json::from_str(DEMO)?;
    scenario["responses"][0]["wait_ms"] = json!(600_000);
    Binary::start(&scenario.to_string()).await?.stop().await?;
    scenario["responses"][0]["wait_ms"] = json!(600_001);
    let mut file = tempfile::NamedTempFile::new()?;
    file.write_all(scenario.to_string().as_bytes())?;
    let result = tokio::time::timeout(
        tqa_testkit::deadline::scaled(Duration::from_secs(5)),
        Command::new(env!("CARGO_BIN_EXE_tqa-fake-model"))
            .arg("--scenario")
            .arg(file.path())
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    assert!(!result.status.success(), "accepted a wait past the bound");
    assert!(String::from_utf8(result.stderr)?.contains("wait"));
    Ok(())
}
