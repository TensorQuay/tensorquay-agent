//! New result content resets stagnation; warning and recovery are pure journal projections.
mod support;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_types::{BudgetKind, ErrorCode, Event, ServerFrame};

const NUDGE: &str = "Recent steps repeated earlier tool results. Try a different approach, or explain why you cannot make progress.";

fn repeated(contents: &[&str]) -> Result<Harness, Box<dyn std::error::Error>> {
    let tool = fake(
        "read_file",
        true,
        contents.iter().map(|text| output(text, 0)).collect(),
    )?;
    let mut script = contents
        .iter()
        .enumerate()
        .map(|(index, _)| {
            calls(&[(
                &format!("call-{index}"),
                "read_file",
                &format!(r#"{{"path":"file-{index}"}}"#),
            )])
        })
        .collect::<Vec<_>>();
    script.push(answer());
    Harness::new(script, registry(&[tool])?)
}
fn warning(event: &Event) -> bool {
    matches!(
        event,
        Event::BudgetWarning {
            budget: BudgetKind::NoProgress,
            used: 6,
            limit: 12
        }
    )
}
fn no_progress() -> RunOutcome {
    RunOutcome::Failed {
        code: ErrorCode::NoProgress,
        reason: "no_progress".into(),
    }
}

#[tokio::test]
// TEST: CORE-U-036
async fn core_e_037_six_repeated_steps_nudge_and_twelve_fail_from_the_same_journal() -> TestResult {
    let h = repeated(&["same"; 14])?;
    assert_eq!(
        Runner::new(h.deps)?
            .run(
                &[],
                Some(turn()),
                CancellationToken::new(),
                CancellationToken::new()
            )
            .await?,
        no_progress()
    );
    let events = h.sink.events();
    assert_eq!(
        tool_results(&events).len(),
        13,
        "the first new result makes progress"
    );
    assert_eq!(events.iter().filter(|event| warning(event)).count(), 1);
    let warning_index = events.iter().position(warning).ok_or("missing warning")?;
    assert_eq!(tool_results(&events[..warning_index]).len(), 7);
    let requests = h.provider.requests();
    assert_eq!(requests.len(), 13);
    assert!(!request_value(&requests[6]).to_string().contains(NUDGE));
    assert!(request_value(&requests[7]).to_string().contains(NUDGE));
    let frames = h.sink.frames();
    for (index, (frame_index, _)) in frames
        .iter()
        .enumerate()
        .filter(|(_, frame)| {
            matches!(
                frame,
                ServerFrame::Event {
                    event: Event::StepStarted { .. },
                    ..
                }
            )
        })
        .enumerate()
    {
        let projected = tqa_context::project(&frames[..=frame_index])?;
        let actual = &requests[index].messages[3..];
        assert_eq!(actual.len(), projected.len());
        for (actual, expected) in actual.iter().zip(projected) {
            assert_eq!(
                actual.content,
                vec![tqa_llm::ContentPart::Text(expected.text)]
            );
            assert_eq!(
                actual.tool_call_id,
                expected.tool_call_id.map(|id| id.to_string())
            );
            assert_eq!(actual.tool_calls.len(), expected.tool_calls.len());
        }
    }
    assert_closed(&frames)?;
    Ok(())
}

#[tokio::test]
async fn core_e_037_resume_at_warning_boundary_preserves_stagnation_and_single_nudge() -> TestResult
{
    let h = repeated(&["same"; 14])?;
    Runner::new(h.deps)?
        .run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    let frames = h.sink.frames();
    let cut = h
        .sink
        .events()
        .iter()
        .position(warning)
        .ok_or("missing warning")?;
    for include_warning in [false, true] {
        let history = &frames[..cut + usize::from(include_warning)];
        let mut resumed = repeated(&["same"; 7])?;
        resumed.sink_with(MemorySink::new(history));
        assert_eq!(
            Runner::new(resumed.deps)?
                .run(
                    history,
                    None,
                    CancellationToken::new(),
                    CancellationToken::new()
                )
                .await?,
            no_progress()
        );
        assert_eq!(resumed.provider.requests().len(), 6);
        assert_eq!(
            resumed
                .sink
                .events()
                .iter()
                .filter(|event| warning(event))
                .count(),
            1
        );
        assert_eq!(
            resumed.provider.requests()[0]
                .messages
                .iter()
                .filter(|m| m.content == vec![tqa_llm::ContentPart::Text(NUDGE.into())])
                .count(),
            1
        );
        assert_closed(&resumed.sink.frames())?;
    }
    Ok(())
}

#[tokio::test]
async fn core_e_037_new_content_resets_the_counter_but_revisiting_old_content_does_not()
-> TestResult {
    let mut new = vec!["same"; 7];
    new.extend(["new"; 7]);
    let mut old = vec!["a", "b"];
    old.extend((0..12).map(|index| if index % 2 == 0 { "a" } else { "b" }));
    for (contents, expected) in [(new, RunOutcome::Completed), (old, no_progress())] {
        let h = repeated(&contents)?;
        assert_eq!(
            Runner::new(h.deps)?
                .run(
                    &[],
                    Some(turn()),
                    CancellationToken::new(),
                    CancellationToken::new()
                )
                .await?,
            expected
        );
        assert_eq!(
            h.sink
                .events()
                .iter()
                .filter(|event| warning(event))
                .count(),
            1
        );
        assert_closed(&h.sink.frames())?;
    }
    Ok(())
}
