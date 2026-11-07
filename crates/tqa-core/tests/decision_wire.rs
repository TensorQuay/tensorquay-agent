//! CORE-U-032 (the wire half): with `ModelDecides`, a run's journal, which is also what the runtime sends the gateway,
//! is byte for byte what it was before the decision events existed: no `decision` or `decision.fallback` frame and no
//! new field. So nothing of the decision engine reaches the wire before the protocol revision that carries it
//! (ADR-0013 point 7).
mod support;
use std::sync::Arc;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{ModelDecides, Runner};

/// The journal of one scripted run (a tool call, then an answer) with the default engine, one JSON frame per line.
async fn journal() -> Result<String, Box<dyn std::error::Error>> {
    let tool = fake("read_file", true, vec![output("ok", 0)])?;
    let mut h = Harness::new(
        vec![calls(&[("a", "read_file", r#"{"path":"a"}"#)]), answer()],
        registry(std::slice::from_ref(&tool))?,
    )?;
    h.deps.decision = Arc::new(ModelDecides);
    let sink = h.sink.clone();
    let runner = Runner::new(h.deps)?;
    drive(
        runner.run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        ),
        &h.clock,
    )
    .await??;
    let mut lines = String::new();
    for frame in sink.frames() {
        lines.push_str(&without_step_time(&serde_json::to_string(&frame)?));
        lines.push('\n');
    }
    Ok(lines)
}

/// The frame with `step.completed`'s `duration_ms` set to 0: it is measured wall time, the only byte that varies
/// between runs.
fn without_step_time(frame: &str) -> String {
    if !frame.contains(r#""type":"step.completed""#) {
        return frame.to_owned();
    }
    let Some(start) = frame
        .find(r#""duration_ms":"#)
        .map(|at| at + r#""duration_ms":"#.len())
    else {
        return frame.to_owned();
    };
    let end = frame[start..]
        .find(|c: char| !c.is_ascii_digit())
        .map_or(frame.len(), |offset| start + offset);
    format!("{}0{}", &frame[..start], &frame[end..])
}

#[tokio::test]
async fn core_u_032_the_default_engine_writes_todays_journal_byte_for_byte() -> TestResult {
    let first = journal().await?;
    assert_eq!(first, journal().await?, "a run's journal is deterministic");
    assert!(
        !first.contains("decision"),
        "no decision frame or field: {first}"
    );
    // Recorded before the decision events existed; regenerate only for a change that is meant to reach the wire:
    // TQA_UPDATE_FIXTURES=1 cargo nextest run -p tqa-core -E 'test(core_u_032_the_default_engine)'
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/model-decides-journal.jsonl");
    if std::env::var_os("TQA_UPDATE_FIXTURES").is_some() {
        std::fs::write(&path, &first)?;
    }
    assert_eq!(first, std::fs::read_to_string(&path)?);
    Ok(())
}
