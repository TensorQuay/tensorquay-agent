//! CORE-U-027: each step's `usage` event carries that step's tokens, so consumers derive the run total by adding them
//! (derived data is computed, ENGINEERING-GUARDRAILS §1.9). A step without reported usage adds an event flagged
//! estimated.
mod support;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_llm::{FinishReason, StreamChunk, Usage};
use tqa_testkit::ProviderReply;
use tqa_types::Event;

fn reported(prompt_tokens: u64, completion_tokens: u64) -> StreamChunk {
    StreamChunk::Usage(Usage {
        prompt_tokens,
        completion_tokens,
        cached_tokens: None,
        reasoning_tokens: None,
    })
}
fn call_step(id: &str, usage: Option<StreamChunk>) -> ProviderReply {
    let mut chunks = vec![StreamChunk::ToolCallDelta {
        index: 0,
        id: Some(id.into()),
        name: Some("read_file".into()),
        arguments_delta: format!(r#"{{"path":"{id}"}}"#),
    }];
    chunks.extend(usage);
    chunks.push(StreamChunk::Finish(FinishReason::ToolCalls));
    reply(chunks)
}
fn final_step(text: &str, usage: Option<StreamChunk>) -> ProviderReply {
    let mut chunks = vec![];
    if !text.is_empty() {
        chunks.push(StreamChunk::TextDelta(text.into()));
    }
    chunks.extend(usage);
    chunks.push(StreamChunk::Finish(FinishReason::Stop));
    reply(chunks)
}
/// Each usage event as (prompt, completion, estimated), in order.
fn usages(events: &[Event]) -> Vec<(u64, u64, bool)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Usage {
                prompt_tokens,
                completion_tokens,
                estimated,
                ..
            } => Some((*prompt_tokens, *completion_tokens, *estimated)),
            _ => None,
        })
        .collect()
}
async fn run(
    script: Vec<ProviderReply>,
    reads: usize,
) -> Result<Vec<Event>, Box<dyn std::error::Error>> {
    let tool = fake(
        "read_file",
        true,
        (0..reads).map(|_| output("contents", 0)).collect(),
    )?;
    let h = Harness::new(script, registry(&[tool])?)?;
    let sink = h.sink.clone();
    let outcome = Runner::new(h.deps)?
        .run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert_eq!(outcome, RunOutcome::Completed);
    Ok(sink.events())
}

#[tokio::test]
async fn core_u_027_each_of_five_steps_reports_its_own_usage_and_the_total_is_their_sum()
-> TestResult {
    let mut script: Vec<_> = (1..=4)
        .map(|step| call_step(&format!("c{step}"), Some(reported(100 * step, 10 * step))))
        .collect();
    script.push(final_step("done", Some(reported(500, 50))));
    let events = run(script, 4).await?;
    let steps = usages(&events);
    assert_eq!(
        steps,
        [
            (100, 10, false),
            (200, 20, false),
            (300, 30, false),
            (400, 40, false),
            (500, 50, false)
        ]
    );
    let total = steps.iter().fold((0, 0), |(prompt, completion), step| {
        (prompt + step.0, completion + step.1)
    });
    assert_eq!(
        total,
        (1500, 150),
        "the run total is derived from the step events"
    );
    assert!(
        events
            .iter()
            .all(|event| !matches!(event, Event::RunCompleted { .. })
                || serde_json::to_value(event).is_ok_and(|value| value.get("usage").is_none())),
        "run.completed stores no total"
    );
    Ok(())
}

#[tokio::test]
async fn core_u_027_a_step_without_reported_usage_adds_an_event_flagged_estimated() -> TestResult {
    let script = vec![
        call_step("c1", Some(reported(100, 10))),
        call_step("c2", None),
        final_step("done", None),
    ];
    let events = run(script, 2).await?;
    let steps = usages(&events);
    assert_eq!(steps.len(), 3, "one usage event per step: {steps:?}");
    assert_eq!(steps[0], (100, 10, false));
    for (prompt, completion, estimated) in &steps[1..] {
        assert!(
            *estimated,
            "a step without reported usage is flagged estimated"
        );
        assert_eq!(*prompt, 0, "an unreported prompt count is not guessed");
        assert!(
            *completion > 0,
            "its output is estimated from the streamed bytes"
        );
    }
    Ok(())
}
