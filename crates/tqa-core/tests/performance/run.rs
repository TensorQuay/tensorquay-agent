use super::{STEPS, journal::Journal, support::*};
use std::{error::Error, time::Instant};
use tokio::runtime::Runtime;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_types::{Event, ServerFrame, ToolStatus};

#[derive(Clone, Copy)]
pub(super) enum Variant {
    Memory,
    Journal,
}

pub(super) struct Sample {
    pub run_ns: u64,
    pub frames: Vec<ServerFrame>,
}

pub(super) fn sample(runtime: &Runtime, variant: Variant) -> Result<Sample, Box<dyn Error>> {
    let tool = fake("read_file", true, vec![output("scripted result", 0); 9])?;
    let mut script = Vec::new();
    for step in 1..STEPS {
        script.push(calls(&[(
            &format!("call-{step}"),
            "read_file",
            r#"{"path":"fixture"}"#,
        )]));
    }
    script.push(answer());
    let mut harness = Harness::new(script, registry(std::slice::from_ref(&tool))?)?;
    let journal = match variant {
        Variant::Memory => None,
        Variant::Journal => Some(Journal::new(runtime)?),
    };
    if let Some(journal) = &journal {
        harness.deps.sink = journal.sink.clone();
    }
    let runner = Runner::new(harness.deps)?;
    let message = Some(turn());
    let cancel = CancellationToken::new();
    let shutdown = CancellationToken::new();
    // Setup, recovery, validation and teardown are outside the whole-run clock.
    let (outcome, elapsed) = runtime.block_on(async {
        let started = Instant::now();
        let outcome = runner.run(&[], message, cancel, shutdown).await;
        (outcome, started.elapsed())
    });
    drop(runner);
    let frames = if let Some(journal) = journal {
        let snapshot = journal.finish(runtime)?;
        assert_eq!(
            snapshot.synced.len(),
            11,
            "ten step syncs and one run-end sync"
        );
        snapshot.frames
    } else {
        harness.sink.frames()
    };
    validate(&frames)?;
    assert_eq!(outcome?, RunOutcome::Completed);
    assert_eq!(harness.provider.requests().len(), 10);
    assert_eq!(harness.provider.active_streams(), 0);
    assert_eq!(tool.calls().len(), 9);
    Ok(Sample {
        run_ns: u64::try_from(elapsed.as_nanos())?,
        frames,
    })
}

pub(super) fn validate(frames: &[ServerFrame]) -> TestResult {
    let events: Vec<_> = frames
        .iter()
        .filter_map(|frame| match frame {
            ServerFrame::Event { event, .. } => Some(event.clone()),
            _ => None,
        })
        .collect();
    let steps: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            Event::StepCompleted { step, .. } => Some(*step),
            _ => None,
        })
        .collect();
    if steps != (1..=10).collect::<Vec<_>>() {
        return Err("PERF-001 expected exactly ten completed steps in order".into());
    }
    if events
        .iter()
        .filter(|event| matches!(event, Event::RunCompleted { .. }))
        .count()
        != 1
    {
        return Err("PERF-001 expected exactly one completed run".into());
    }
    let expected: Vec<_> = (1..=9)
        .map(|step| {
            (
                format!("call-{step}"),
                ToolStatus::Ok,
                "scripted result".to_owned(),
            )
        })
        .collect();
    if tool_results(&events) != expected {
        return Err("PERF-001 expected nine successful scripted tool results".into());
    }
    let text: String = events
        .iter()
        .filter_map(|event| match event {
            Event::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    if text != "final answer" {
        return Err("PERF-001 expected the scripted final answer".into());
    }
    assert_closed(frames)
}
