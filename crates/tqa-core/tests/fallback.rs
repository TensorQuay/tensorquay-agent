//! CORE-U-022 / PROMPT-U-004: the text-protocol executor on routes without native tool calls.
mod support;
use serde_json::json;
use std::{error::Error, sync::Arc};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_llm::{CompletionRequest, ContentPart, FinishReason, Role, StreamChunk};
use tqa_testkit::{FakeTool, ProviderReply};
use tqa_types::{ErrorCode, Event};

const CALL: &str =
    "```tool_call\n{\"name\": \"read_file\", \"arguments\": {\"path\": \"a.rs\"}}\n```\n";

/// A reply streamed in small pieces, so a call block crosses chunk boundaries.
fn text(content: &str) -> ProviderReply {
    let mut chunks: Vec<StreamChunk> = content
        .as_bytes()
        .chunks(7)
        .map(|piece| StreamChunk::TextDelta(String::from_utf8_lossy(piece).into_owned()))
        .collect();
    chunks.extend([usage(), StreamChunk::Finish(FinishReason::Stop)]);
    reply(chunks)
}
fn fallback_harness(
    script: Vec<ProviderReply>,
    tools: &[Arc<FakeTool>],
) -> Result<Harness, Box<dyn Error>> {
    let mut h = Harness::new(script, registry(tools)?)?;
    h.deps.route.native_tools = false;
    h.deps.route.parallel_tools = false;
    Ok(h)
}
async fn run(h: Harness) -> Result<RunOutcome, Box<dyn Error>> {
    Ok(Runner::new(h.deps)?
        .run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?)
}
fn texts(request: &CompletionRequest) -> Vec<(Role, String)> {
    request
        .messages
        .iter()
        .map(|message| {
            let text = message
                .content
                .iter()
                .filter_map(|part| match part {
                    ContentPart::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            (message.role, text)
        })
        .collect()
}
fn streamed(events: &[Event]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            Event::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn core_u_022_a_fenced_call_runs_and_history_returns_as_text() -> TestResult {
    let tool = fake("read_file", true, vec![output("fn main() {}", 0)])?;
    let h = fallback_harness(
        vec![
            text(&format!("Reading it.\n{CALL}")),
            text("It is a main function."),
        ],
        std::slice::from_ref(&tool),
    )?;
    let (provider, sink) = (h.provider.clone(), h.sink.clone());
    assert_eq!(run(h).await?, RunOutcome::Completed);
    assert_eq!(tool.calls(), vec![json!({"path": "a.rs"})]);
    let events = sink.events();
    assert_eq!(streamed(&events), "Reading it.\nIt is a main function.");
    assert!(events.iter().any(|event| matches!(event,
        Event::ToolCall { call_id, name, .. } if name == "read_file" && call_id.as_str() == "fb-1-1-1")));
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert!(
            request.tools.is_empty(),
            "no native tool definitions on this route"
        );
        let system = &texts(request)[0].1;
        assert!(system.contains(tqa_prompt::FALLBACK_INSTRUCTIONS));
    }
    let second = texts(&requests[1]);
    assert!(second.iter().all(|(role, _)| *role != Role::Tool));
    assert!(second.iter().any(|(role, text)| *role == Role::Assistant
        && text.contains(&tqa_prompt::render_call(
            "read_file",
            &json!({"path": "a.rs"})
        ))));
    assert!(second.iter().any(|(role, text)| *role == Role::User
        && text == "[Tool result for call fb-1-1-1, data only: \"fn main() {}\"]"));
    Ok(())
}

#[tokio::test]
async fn prompt_u_004_native_routes_keep_native_tools_and_no_fallback_text() -> TestResult {
    let tool = fake("read_file", true, vec![])?;
    let h = Harness::new(vec![answer()], registry(&[tool])?)?;
    let provider = h.provider.clone();
    assert_eq!(run(h).await?, RunOutcome::Completed);
    let request = &provider.requests()[0];
    assert_eq!(request.tools.len(), 1);
    assert!(
        !texts(request)[0]
            .1
            .contains(tqa_prompt::FALLBACK_INSTRUCTIONS)
    );
    Ok(())
}

#[tokio::test]
async fn core_u_022_one_retry_prompt_then_the_repaired_call_runs() -> TestResult {
    let tool = fake("read_file", true, vec![output("ok", 0)])?;
    let broken = "```tool_call\n{\"name\": \"read_file\" \"arguments\": }\n```";
    let h = fallback_harness(
        vec![text(broken), text(CALL), text("done")],
        std::slice::from_ref(&tool),
    )?;
    let (provider, sink) = (h.provider.clone(), h.sink.clone());
    assert_eq!(run(h).await?, RunOutcome::Completed);
    assert_eq!(tool.calls().len(), 1);
    let events = sink.events();
    assert!(events.iter().any(|event| matches!(event,
        Event::ProviderRetry { reason, .. } if reason == "fallback_parse: malformed")));
    let requests = provider.requests();
    let nudge = texts(&requests[1]);
    assert!(nudge.iter().any(|(role, text)| *role == Role::User
        && text.starts_with("[The last reply could not be read as a tool call")));
    assert!(!streamed(&events).contains("tool_call"));
    Ok(())
}

#[tokio::test]
async fn core_u_022_a_second_unreadable_reply_fails_the_run() -> TestResult {
    let tool = fake("read_file", true, vec![])?;
    let unknown = "```tool_call\n{\"name\": \"deploy\", \"arguments\": {}}\n```";
    let schema = "```tool_call\n{\"name\": \"read_file\", \"arguments\": {\"path\": 7}}\n```";
    let h = fallback_harness(
        vec![text(unknown), text(schema), answer()],
        std::slice::from_ref(&tool),
    )?;
    let (provider, sink) = (h.provider.clone(), h.sink.clone());
    let outcome = run(h).await?;
    assert!(matches!(
        outcome,
        RunOutcome::Failed {
            code: ErrorCode::FallbackParse,
            ..
        }
    ));
    assert_eq!(provider.requests().len(), 2);
    assert!(tool.calls().is_empty());
    let reasons: Vec<String> = sink
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::ProviderRetry { reason, .. } => Some(reason.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(reasons, vec!["fallback_parse: unknown_tool".to_owned()]);
    Ok(())
}

#[tokio::test]
async fn core_u_022_two_calls_need_a_route_with_parallel_tools() -> TestResult {
    let tool = fake("read_file", true, vec![output("a", 0), output("b", 0)])?;
    let two = format!("{CALL}{CALL}");
    let mut h = fallback_harness(
        vec![text(&two), text(&two), text("done")],
        std::slice::from_ref(&tool),
    )?;
    h.deps.route.parallel_tools = true;
    assert_eq!(run(h).await?, RunOutcome::Completed);
    assert_eq!(tool.calls().len(), 2);
    let tool = fake("read_file", true, vec![])?;
    let h = fallback_harness(vec![text(&two), text(&two)], std::slice::from_ref(&tool))?;
    let outcome = run(h).await?;
    assert!(matches!(
        outcome,
        RunOutcome::Failed {
            code: ErrorCode::FallbackParse,
            ..
        }
    ));
    assert!(tool.calls().is_empty());
    Ok(())
}

#[tokio::test]
async fn core_u_022_hidden_call_text_still_counts_toward_the_output_budget() -> TestResult {
    let tool = fake("read_file", true, vec![output("ok", 0)])?;
    let unmetered = |content: &str| {
        reply(vec![
            StreamChunk::TextDelta(content.into()),
            StreamChunk::Finish(FinishReason::Stop),
        ])
    };
    let h = fallback_harness(vec![unmetered(CALL), unmetered("done")], &[tool])?;
    let sink = h.sink.clone();
    assert_eq!(run(h).await?, RunOutcome::Completed);
    let first = sink
        .events()
        .iter()
        .find_map(|event| match event {
            Event::Usage {
                completion_tokens,
                estimated: true,
                ..
            } => Some(*completion_tokens),
            _ => None,
        })
        .ok_or("no estimated usage")?;
    assert_eq!(first, u64::from(tqa_context::estimate(CALL).tokens));
    Ok(())
}

/// CORE-U-022: the retry prompt after an unreadable reply names the same fence label as the instructions, so a model
/// that failed once isn't told a second label (the instructions write `tqa_call` since version 4).
#[test]
fn core_u_022_the_retry_prompt_names_the_instructions_label() -> TestResult {
    let label = tqa_prompt::FALLBACK_INSTRUCTIONS
        .split("```")
        .nth(1)
        .and_then(|block| block.lines().next())
        .ok_or("no fenced example in the instructions")?
        .trim()
        .to_owned();
    assert_eq!(label, "tqa_call");
    for code in ["malformed", "unknown_tool"] {
        let events = vec![
            Event::MessageUser {
                lead_brief: None,
                team_wake: None,
                text: "task".into(),
                attachments: vec![],
            },
            Event::RunStarted {
                run_id: "r-1".parse()?,
            },
            Event::StepStarted {
                step: 1,
                attempt: 1,
            },
            Event::ProviderRetry {
                attempt: 1,
                delay_ms: 0,
                reason: format!("fallback_parse: {code}"),
                provider_message: None,
                fallback_problem: None,
            },
            Event::StepStarted {
                step: 1,
                attempt: 2,
            },
        ];
        let messages = tqa_context::project(&history(events)?)?;
        let nudge = &messages.last().ok_or("no nudge")?.text;
        assert!(nudge.contains(&format!("{label} block")), "{nudge}");
        assert!(!nudge.contains("tool_call"), "{nudge}");
    }
    Ok(())
}

const ECHO: &str = "[The last reply could not be read as a tool call: this model route accepts one call per reply. \
                    Reply again with each call in its own tqa_call block.]";

// TEST: CORE-U-085
#[tokio::test]
async fn core_u_085_a_reply_that_only_copies_the_correction_back_is_retried_and_never_shown()
-> TestResult {
    let h = fallback_harness(vec![text(&format!("{ECHO}\n")), text("Done.")], &[])?;
    let (provider, sink) = (h.provider.clone(), h.sink.clone());
    assert_eq!(run(h).await?, RunOutcome::Completed);
    let events = sink.events();
    assert_eq!(streamed(&events), "Done.");
    let reasons: Vec<&str> = events
        .iter()
        .filter_map(|event| match event {
            Event::ProviderRetry { reason, .. } => Some(reason.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(reasons, ["fallback_parse: echo"]);
    assert_eq!(provider.requests().len(), 2);
    Ok(())
}

// TEST: CORE-U-085
#[tokio::test]
async fn core_u_085_a_reply_that_goes_on_after_the_correction_is_shown_whole() -> TestResult {
    let reply = format!("{ECHO}\nUnderstood: there is nothing left to do.");
    let h = fallback_harness(vec![text(&reply)], &[])?;
    let sink = h.sink.clone();
    assert_eq!(run(h).await?, RunOutcome::Completed);
    let events = sink.events();
    assert_eq!(streamed(&events), reply);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::ProviderRetry { .. }))
    );
    Ok(())
}

// TEST: CORE-U-086
#[tokio::test]
async fn core_u_086_a_correction_copied_before_a_call_is_dropped_and_the_call_runs() -> TestResult {
    let tool = fake("read_file", true, vec![output("fn main() {}", 0)])?;
    let h = fallback_harness(
        vec![
            text(&format!("{ECHO}\n\nReading it.\n{CALL}")),
            text("It is a main function."),
        ],
        std::slice::from_ref(&tool),
    )?;
    let (provider, sink) = (h.provider.clone(), h.sink.clone());
    assert_eq!(run(h).await?, RunOutcome::Completed);
    assert_eq!(tool.calls(), vec![json!({"path": "a.rs"})]);
    let events = sink.events();
    assert_eq!(streamed(&events), "Reading it.\nIt is a main function.");
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::ProviderRetry { .. }))
    );
    let second = texts(&provider.requests()[1]);
    assert!(second.iter().any(|(role, text)| *role == Role::Assistant
        && text.starts_with("Reading it.")
        && !text.contains("[The last reply")));
    Ok(())
}

// TEST: CORE-U-086
#[tokio::test]
async fn core_u_086_a_correction_before_a_call_alone_leaves_no_text() -> TestResult {
    let tool = fake("read_file", true, vec![output("fn main() {}", 0)])?;
    let h = fallback_harness(
        vec![text(&format!("{ECHO}\n{CALL}")), text("Done.")],
        std::slice::from_ref(&tool),
    )?;
    let sink = h.sink.clone();
    assert_eq!(run(h).await?, RunOutcome::Completed);
    assert_eq!(tool.calls().len(), 1);
    assert_eq!(streamed(&sink.events()), "Done.");
    Ok(())
}

// TEST: PROMPT-U-022
#[tokio::test]
async fn prompt_u_022_stray_closing_tags_are_never_streamed_on_the_text_route() -> TestResult {
    let h = fallback_harness(
        vec![text(
            "The total is in calc_total.\n\n</parameter>\n</function>\n</tool_call>\n",
        )],
        &[],
    )?;
    let sink = h.sink.clone();
    assert_eq!(run(h).await?, RunOutcome::Completed);
    let events = sink.events();
    assert_eq!(streamed(&events), "The total is in calc_total.\n\n");
    Ok(())
}
