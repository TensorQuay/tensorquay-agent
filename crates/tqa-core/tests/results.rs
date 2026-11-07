//! CORE-U-018 and CTX-U-006 at the loop boundary: errors and bounded authoritative results.
mod support;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::Runner;
use tqa_tools::{ToolOutput, ToolRegistry};
use tqa_types::{Event, ToolStatus};

#[tokio::test]
async fn core_u_002_large_tool_output_is_chunked_and_spilled_before_the_result() -> TestResult {
    let text = "é".repeat(70_000);
    let tool = fake("read_file", true, vec![output(&text, 0)])?;
    let mut h = Harness::new(
        vec![calls(&[("a", "read_file", r#"{"path":"a"}"#)]), answer()],
        registry(&[tool])?,
    )?;
    h.deps.limits.tool_result_bytes = 1024;
    Runner::new(h.deps)?
        .run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    let result = tool_results(&h.sink.events());
    assert!(result[0].2.len() <= 1024 && result[0].2.contains("output truncated"));
    assert_eq!(
        std::fs::read_to_string(h.directory.path().join(".tqa/conversation/spill/a.txt"))?,
        text
    );
    let mut assembled = tqa_types::ToolOutputAssembler::new(1, 200_000);
    let mut full = None;
    for event in h.sink.events() {
        if matches!(event, Event::ToolOutput { .. }) {
            full = assembled.push(&event)?;
        }
    }
    assert_eq!(full.as_deref(), Some(text.as_str()));
    Ok(())
}
#[tokio::test]
async fn core_u_018_an_undispatched_call_has_no_execution_pair_and_nonzero_exit_is_an_error()
-> TestResult {
    // A call its tool's schema refuses is answered without running: no tool.start, no tool.end.
    let tool = fake("shell", false, vec![])?;
    let h = Harness::new(
        vec![calls(&[("a", "shell", r#"{"path":7}"#)]), answer()],
        registry(std::slice::from_ref(&tool))?,
    )?;
    Runner::new(h.deps)?
        .run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert!(tool.calls().is_empty());
    assert_eq!(tool_results(&h.sink.events())[0].1, ToolStatus::Error);
    assert!(!h.sink.types().contains(&"tool.start".into()));
    let mut reply = output("process failed", 0);
    let mut failed = ToolOutput::text("process failed");
    failed.exit_code = Some(2);
    reply.result = Ok(failed);
    let tool = fake("shell", false, vec![reply])?;
    let h = Harness::new(
        vec![calls(&[("a", "shell", r#"{"path":"a"}"#)]), answer()],
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
    assert_eq!(tool_results(&h.sink.events())[0].1, ToolStatus::Error);
    assert!(h.sink.events().contains(&Event::ToolEnd {
        call_id: "a".parse()?,
        status: ToolStatus::Error,
        exit_code: Some(2),
        failure: None,
    }));
    Ok(())
}
#[tokio::test]
async fn core_e_010_an_invalid_call_identity_is_asked_again_once_then_fails() -> TestResult {
    // An invalid or empty tool-call identifier is an answer that couldn't be read: asked again once, as a cut stream
    // is, then the run fails (CORE-E-040); no call ever runs.
    for id in ["not/a/valid/id", ""] {
        let invalid = || calls(&[(id, "read_file", "{}")]);
        let h = Harness::new(vec![invalid(), invalid()], ToolRegistry::new())?;
        let result = drive(
            Runner::new(h.deps)?.run(
                &[],
                Some(turn()),
                CancellationToken::new(),
                CancellationToken::new(),
            ),
            &h.clock,
        )
        .await??;
        assert_eq!(
            result,
            tqa_core::RunOutcome::Failed {
                code: tqa_types::ErrorCode::ProviderError,
                reason: "malformed".into(),
            }
        );
        assert_eq!(h.provider.requests().len(), 2);
        assert!(!h.sink.types().contains(&"tool.call".into()));
    }
    Ok(())
}

#[tokio::test]
async fn core_u_024_a_mebibyte_of_output_keeps_head_and_tail_under_a_64_kib_cap() -> TestResult {
    const CAP: u32 = 64 * 1024;
    const TOTAL: usize = 1024 * 1024;
    let (head, tail) = ("HEAD-OF-OUTPUT\n", "\nTAIL-OF-OUTPUT");
    let text = format!(
        "{head}{}{tail}",
        "x".repeat(TOTAL - head.len() - tail.len())
    );
    let tool = fake("shell", false, vec![output(&text, 0)])?;
    let mut h = Harness::new(
        vec![calls(&[("big", "shell", r#"{"path":"big"}"#)]), answer()],
        registry(std::slice::from_ref(&tool))?,
    )?;
    h.deps.limits.tool_result_bytes = CAP;
    let run = Runner::new(h.deps)?;
    run.run(
        &[],
        Some(turn()),
        CancellationToken::new(),
        CancellationToken::new(),
    )
    .await?;
    let results = tool_results(&h.sink.events());
    let content = &results[0].2;
    assert!(content.len() <= CAP as usize, "{} bytes", content.len());
    assert!(
        content.starts_with(head) && content.ends_with(tail),
        "head and tail survive the cap"
    );
    assert!(
        content.contains(&format!("{TOTAL} bytes total")),
        "the total size is stated"
    );
    assert!(
        content.contains(".tqa/conversation/spill/big.txt"),
        "the spill path is stated"
    );
    let spilled =
        std::fs::read_to_string(h.directory.path().join(".tqa/conversation/spill/big.txt"))?;
    assert_eq!(spilled.len(), TOTAL);
    assert_eq!(spilled, text);
    Ok(())
}

#[tokio::test]
async fn core_e_028_text_and_a_tool_call_in_one_response_both_take_effect() -> TestResult {
    let tool = fake("read_file", true, vec![output("contents", 0)])?;
    let mut both = calls(&[("a", "read_file", r#"{"path":"a"}"#)]);
    if let Ok(chunks) = &mut both.result {
        let text = tqa_llm::StreamChunk::TextDelta("Let me read it first. ".into());
        chunks.insert(
            0,
            tqa_testkit::ProviderChunk {
                delay: std::time::Duration::ZERO,
                result: Ok(text),
            },
        );
    }
    let h = Harness::new(vec![both, answer()], registry(std::slice::from_ref(&tool))?)?;
    let sink = h.sink.clone();
    let run = Runner::new(h.deps)?;
    let outcome = run
        .run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert_eq!(outcome, tqa_core::RunOutcome::Completed);
    let events = sink.events();
    assert!(
        events.contains(&Event::TextDelta {
            text: "Let me read it first. ".into()
        }),
        "the text is emitted"
    );
    assert_eq!(tool.calls().len(), 1, "the call is executed");
    // The run completes only after a response without calls: two steps, the result before the second one.
    let steps = events
        .iter()
        .filter(|event| matches!(event, Event::StepCompleted { .. }))
        .count();
    assert_eq!(steps, 2);
    let result = events
        .iter()
        .position(|event| matches!(event, Event::ToolResult { .. }))
        .ok_or("no result")?;
    let completed = events
        .iter()
        .position(|event| matches!(event, Event::RunCompleted { .. }))
        .ok_or("not completed")?;
    assert!(result < completed);
    Ok(())
}

// TEST: CORE-U-082
#[tokio::test]
async fn core_u_082_a_computer_calls_target_reaches_its_tool_result_and_never_the_model()
-> TestResult {
    let mut clicked = ToolOutput::text("Clicked e7; no visible change.");
    clicked.target = Some(tqa_types::ScreenTarget::new(
        tqa_types::ScreenRole::Button,
        "Save".into(),
    )?);
    let tool = fake(
        "read_file",
        false,
        vec![
            tqa_testkit::FakeToolReply {
                delay: std::time::Duration::ZERO,
                result: Ok(clicked),
            },
            output("Typed 2 characters into the focus; no visible change.", 0),
        ],
    )?;
    let h = Harness::new(
        vec![
            calls(&[("a", "read_file", r#"{"path":"a"}"#)]),
            calls(&[("b", "read_file", r#"{"path":"b"}"#)]),
            answer(),
        ],
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
    let targets: Vec<Option<tqa_types::ScreenTarget>> = h
        .sink
        .events()
        .into_iter()
        .filter_map(|event| match event {
            Event::ToolResult { target, .. } => Some(target),
            _ => None,
        })
        .collect();
    assert_eq!(
        targets,
        vec![
            Some(tqa_types::ScreenTarget::new(
                tqa_types::ScreenRole::Button,
                "Save".into()
            )?),
            None
        ],
        "the element on the click's result; nothing on a result without one"
    );
    // The model's next requests hold the results' content and never the target.
    for request in h.provider.requests() {
        let sent = request_value(&request).to_string();
        assert!(
            !sent.contains("\"target\"") && !sent.contains("\"Save\""),
            "{sent}"
        );
    }
    Ok(())
}
