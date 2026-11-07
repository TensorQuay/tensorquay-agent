//! CORE-U-001/002/003/004/026/036: exact event contracts, scheduling and deterministic requests.
mod support;
use std::time::Duration;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_tools::ToolRegistry;
use tqa_types::{Event, ServerFrame};

#[tokio::test]
async fn core_u_001_plain_answer_matches_golden_and_projection() -> TestResult {
    let h = Harness::new(vec![answer()], ToolRegistry::new())?;
    let runner = Runner::new(h.deps)?;
    assert_eq!(
        runner
            .run(
                &[],
                Some(turn()),
                CancellationToken::new(),
                CancellationToken::new()
            )
            .await?,
        RunOutcome::Completed
    );
    assert_eq!(
        h.sink.types(),
        event_types(include_str!("fixtures/plain.txt"))
    );
    let frames = h.sink.frames();
    assert_closed(&frames)?;
    let messages = tqa_context::project(&frames)?;
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[1].text, "final answer");
    Ok(())
}
#[tokio::test]
async fn core_u_002_one_tool_matches_golden_and_next_request() -> TestResult {
    let tool = fake("read_file", true, vec![output("file body", 0)])?;
    let h = Harness::new(
        vec![calls(&[("a", "read_file", r#"{"path":"file"}"#)]), answer()],
        registry(std::slice::from_ref(&tool))?,
    )?;
    let runner = Runner::new(h.deps)?;
    assert_eq!(
        runner
            .run(
                &[],
                Some(turn()),
                CancellationToken::new(),
                CancellationToken::new()
            )
            .await?,
        RunOutcome::Completed
    );
    assert_eq!(
        h.sink.types(),
        event_types(include_str!("fixtures/one-tool.txt"))
    );
    let requests = h.provider.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[1]
            .messages
            .last()
            .and_then(|message| message.tool_call_id.as_deref()),
        Some("a")
    );
    assert_eq!(tool.calls().len(), 1);
    assert_closed(&h.sink.frames())?;
    Ok(())
}
#[tokio::test]
async fn core_u_003_two_parallel_reads_match_golden() -> TestResult {
    let a = fake("read_a", true, vec![output("a", 0)])?;
    let b = fake("read_b", true, vec![output("b", 0)])?;
    let h = Harness::new(
        vec![
            calls(&[
                ("a", "read_a", r#"{"path":"a"}"#),
                ("b", "read_b", r#"{"path":"b"}"#),
            ]),
            answer(),
        ],
        registry(&[a, b])?,
    )?;
    Runner::new(h.deps)?
        .run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert_eq!(
        h.sink.types(),
        event_types(include_str!("fixtures/parallel.txt"))
    );
    assert_closed(&h.sink.frames())?;
    Ok(())
}
#[tokio::test]
async fn core_u_003_three_latencies_finish_in_time_order_but_project_in_call_order() -> TestResult {
    let a = fake("read_a", true, vec![output("a", 300)])?;
    let b = fake("read_b", true, vec![output("b", 100)])?;
    let c = fake("read_c", true, vec![output("c", 200)])?;
    let h = Harness::new(
        vec![
            calls(&[
                ("a", "read_a", r#"{"path":"a"}"#),
                ("b", "read_b", r#"{"path":"b"}"#),
                ("c", "read_c", r#"{"path":"c"}"#),
            ]),
            answer(),
        ],
        registry(&[a.clone(), b.clone(), c.clone()])?,
    )?;
    let runner = Runner::new(h.deps)?;
    let run = runner.run(
        &[],
        Some(turn()),
        CancellationToken::new(),
        CancellationToken::new(),
    );
    tokio::pin!(run);
    assert!(futures::poll!(&mut run).is_pending());
    for _ in 0..100 {
        tokio::task::yield_now().await;
        if a.calls().len() + b.calls().len() + c.calls().len() == 3 {
            break;
        }
    }
    assert_eq!(a.calls().len() + b.calls().len() + c.calls().len(), 3);
    let mut outcome = None;
    for count in 1..=3 {
        h.clock.advance(Duration::from_millis(100))?;
        outcome = wait_for_results(&mut run, &h.sink, count).await;
        assert_eq!(tool_results(&h.sink.events()).len(), count);
    }
    assert_eq!(
        match outcome {
            Some(result) => result?,
            None => run.await?,
        },
        RunOutcome::Completed
    );
    assert_eq!(
        tool_results(&h.sink.events())
            .iter()
            .map(|(id, _, _)| id.as_str())
            .collect::<Vec<_>>(),
        ["b", "c", "a"]
    );
    let requests = h.provider.requests();
    assert_eq!(
        requests[1]
            .messages
            .iter()
            .filter_map(|message| message.tool_call_id.as_deref())
            .collect::<Vec<_>>(),
        ["a", "b", "c"]
    );
    Ok(())
}
#[tokio::test]
async fn core_u_004_read_write_read_batches_preserve_dependency_order() -> TestResult {
    let a = fake("read_a", true, vec![output("a", 0), output("c", 0)])?;
    let b = fake("read_b", true, vec![output("b", 0)])?;
    let shell = fake("shell", false, vec![output("wrote", 0)])?;
    let h = Harness::new(
        vec![
            calls(&[
                ("a", "read_a", r#"{"path":"a"}"#),
                ("b", "read_b", r#"{"path":"b"}"#),
                ("s", "shell", r#"{"path":"s"}"#),
                ("c", "read_a", r#"{"path":"c"}"#),
            ]),
            answer(),
        ],
        registry(&[a, b, shell])?,
    )?;
    Runner::new(h.deps)?
        .run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    let events = h.sink.events();
    let at = |wanted: &str, start: bool| {
        events.iter().position(|event| match event {
            Event::ToolStart { call_id } if start => call_id.as_str() == wanted,
            Event::ToolResult { call_id, .. } if !start => call_id.as_str() == wanted,
            _ => false,
        })
    };
    assert!(at("a", false) < at("s", true) && at("b", false) < at("s", true));
    assert!(at("s", false) < at("c", true));
    assert_closed(&h.sink.frames())?;
    Ok(())
}
#[tokio::test]
async fn core_u_026_core_u_036_fifty_runs_rebuild_identical_requests_from_projection() -> TestResult
{
    let mut previous = None;
    for _ in 0..50 {
        let tool = fake("read_file", true, vec![output("same", 0)])?;
        let h = Harness::new(
            vec![calls(&[("a", "read_file", r#"{"path":"file"}"#)]), answer()],
            registry(&[tool])?,
        )?;
        Runner::new(h.deps)?
            .run(
                &[],
                Some(turn()),
                CancellationToken::new(),
                CancellationToken::new(),
            )
            .await?;
        let requests = h.provider.requests();
        let serialized =
            serde_json::to_vec(&requests.iter().map(request_value).collect::<Vec<_>>())?;
        let events = serde_json::to_vec(&h.sink.events())?;
        if let Some((prior_requests, prior_events)) = &previous {
            assert_eq!(&serialized, prior_requests);
            assert_eq!(&events, prior_events);
        }
        previous = Some((serialized, events));
        assert_eq!(
            requests[0].messages[0].content,
            vec![tqa_llm::ContentPart::Text("system slot".into())]
        );
        assert_eq!(
            requests[0].messages[1].content,
            vec![tqa_llm::ContentPart::Text("instruction slot".into())]
        );
        // A native-tool route: the tools slot is empty and the catalogue travels in `tools` (CORE-U-053).
        assert_eq!(
            requests[0].messages[2].content,
            vec![tqa_llm::ContentPart::Text(String::new())]
        );
        assert!(
            requests[0]
                .tools
                .iter()
                .any(|tool| tool.name == "read_file")
        );
        assert_projection(&h.sink.frames(), &requests)?;
    }
    Ok(())
}

fn assert_projection(
    frames: &[ServerFrame],
    requests: &[tqa_llm::CompletionRequest],
) -> TestResult {
    for (index, frame) in frames.iter().enumerate() {
        if matches!(
            frame,
            ServerFrame::Event {
                event: Event::StepStarted { attempt: 1, .. },
                ..
            }
        ) {
            let projected = tqa_context::project(&frames[..=index])?;
            let number = frames[..=index]
                .iter()
                .filter(|f| {
                    matches!(
                        f,
                        ServerFrame::Event {
                            event: Event::StepStarted { .. },
                            ..
                        }
                    )
                })
                .count()
                - 1;
            assert_eq!(
                requests[number].messages[3..]
                    .iter()
                    .flat_map(|m| m.content.iter())
                    .collect::<Vec<_>>(),
                projected
                    .iter()
                    .map(|m| tqa_llm::ContentPart::Text(m.text.clone()))
                    .collect::<Vec<_>>()
                    .iter()
                    .collect::<Vec<_>>()
            );
        }
    }
    Ok(())
}
