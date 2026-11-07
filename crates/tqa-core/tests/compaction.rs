//! CORE-U-025 / CTX-U-002: compaction in the loop, its model summary and its fallback.
mod support;
use serde_json::json;
use std::error::Error;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_context::{COMPACTION_SUMMARY_PREFIX, Message, estimate, project};
use tqa_core::{RunOutcome, Runner, UserTurn};
use tqa_llm::{ContentPart, FinishReason, StreamChunk};
use tqa_testkit::ProviderReply;
use tqa_tools::ToolRegistry;
use tqa_types::{
    COMPACTION_DEFAULT_OUTPUT_TOKENS, COMPACTION_OUTPUT_TOKENS, CompactionSource,
    CompactionSummary, Event, FinishReason as WireFinish, ServerFrame, ToolStatus,
};

/// About 300 estimated tokens: ceil(1050 * 2 / 7).
const RESULT_BYTES: usize = 1050;
/// Window and output reserve of CORE-U-025.
const WINDOW: u32 = 8192;
const RESERVE: u32 = 1024;

/// One completed run: the user message, a tool step whose result is about 300 tokens, and an
/// answer step. Twenty runs are 60 turns of history.
fn completed_run(index: usize, seq: usize) -> Result<Vec<Event>, Box<dyn Error>> {
    let run_id: tqa_types::RunId = format!("r-{seq}").parse()?;
    let call_id: tqa_types::ToolCallId = format!("c{index}").parse()?;
    Ok(vec![
        Event::MessageUser {
            lead_brief: None,
            team_wake: None,
            text: format!("request {index}"),
            attachments: vec![],
        },
        Event::RunStarted {
            run_id: run_id.clone(),
        },
        Event::StepStarted {
            step: 1,
            attempt: 1,
        },
        Event::ToolCall {
            call_id: call_id.clone(),
            name: "read_file".into(),
            arguments: json!({"path": format!("src/{index}.rs")}),
        },
        Event::ModelCompleted {
            step: 1,
            attempt: 1,
            finish_reason: WireFinish::ToolCalls,
        },
        Event::ToolResult {
            call_id,
            status: ToolStatus::Ok,
            // Distinct per run: identical results would be deduplicated (CTX-U-012) and no longer fill the window.
            content: format!("{index:04}{}", "r".repeat(RESULT_BYTES - 4)),
            target: None,
            evidence: None,
        },
        Event::StepCompleted {
            step: 1,
            duration_ms: 0,
        },
        Event::StepStarted {
            step: 2,
            attempt: 1,
        },
        Event::TextDelta {
            text: format!("answer {index}"),
        },
        Event::ModelCompleted {
            step: 2,
            attempt: 1,
            finish_reason: WireFinish::Stop,
        },
        Event::StepCompleted {
            step: 2,
            duration_ms: 0,
        },
        Event::RunCompleted { run_id },
    ])
}
fn long_history(runs: usize) -> Result<Vec<Event>, Box<dyn Error>> {
    let mut events = Vec::new();
    for index in 1..=runs {
        let seq = events.len() + 1;
        events.extend(completed_run(index, seq)?);
    }
    Ok(events)
}
fn harness(script: Vec<ProviderReply>) -> Result<Harness, Box<dyn Error>> {
    let mut h = Harness::new(script, ToolRegistry::new())?;
    h.deps.route.context_window = Some(WINDOW);
    h.deps.route.max_output = Some(RESERVE);
    h.deps.limits.max_output_tokens = 1_000_000;
    Ok(h)
}
fn text_reply(text: &str) -> ProviderReply {
    reply(vec![
        StreamChunk::TextDelta(text.into()),
        usage(),
        StreamChunk::Finish(FinishReason::Stop),
    ])
}
fn model_summary() -> CompactionSummary {
    CompactionSummary {
        goal: "answer the requests".into(),
        done: vec!["read src/1.rs".into()],
        key_paths: vec!["src/1.rs".into()],
        ..CompactionSummary::default()
    }
}
fn compactions(events: &[Event]) -> Vec<(CompactionSource, CompactionSummary)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Compaction {
                source, summary, ..
            } => Some((*source, summary.clone())),
            _ => None,
        })
        .collect()
}
fn texts(message: &tqa_llm::Message) -> String {
    message
        .content
        .iter()
        .map(|part| match part {
            ContentPart::Text(text) => text.as_str(),
            _ => "",
        })
        .collect()
}
/// The window's accounting: text, call identifiers, names and arguments, plus 4 per message.
fn request_tokens(request: &tqa_llm::CompletionRequest) -> u64 {
    request
        .messages
        .iter()
        .map(|message| {
            let calls: String = message
                .tool_calls
                .iter()
                .flat_map(|call| [call.id.as_str(), &call.name, &call.arguments])
                .collect();
            let answered = message.tool_call_id.clone().unwrap_or_default();
            u64::from(estimate(&format!("{}{calls}{answered}", texts(message))).tokens) + 4
        })
        .sum()
}
async fn run_turn(h: Harness, history: &[ServerFrame]) -> Result<RunOutcome, Box<dyn Error>> {
    let turn = UserTurn {
        lead_brief: None,
        team_wake: None,
        text: "the current request".into(),
        attachments: vec![],
    };
    Ok(Runner::new(h.deps)?
        .run(
            history,
            Some(turn),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?)
}

// TEST: CORE-U-069
/// Every model call a run makes names that run, so the relay's usage meter and secret-read audit attribute it: here the
/// compaction summary and the step after it.
#[tokio::test]
async fn core_u_069_every_model_call_of_a_run_names_the_run() -> TestResult {
    let history = history(long_history(20)?)?;
    let summary_text = serde_json::to_string(&model_summary())?;
    let mut h = harness(vec![text_reply(&summary_text), answer()])?;
    h.sink_with(MemorySink::new(&history));
    let (provider, sink) = (h.provider.clone(), h.sink.clone());
    assert_eq!(run_turn(h, &history).await?, RunOutcome::Completed);
    let run = sink
        .events()
        .into_iter()
        .rev()
        .find_map(|event| match event {
            Event::RunStarted { run_id } => Some(run_id),
            _ => None,
        })
        .ok_or("no run")?;
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert_eq!(request.run_id.as_ref(), Some(&run));
    }
    Ok(())
}

#[tokio::test]
async fn core_u_025_long_history_compacts_once_and_the_next_request_fits() -> TestResult {
    let history = history(long_history(20)?)?;
    let summary_text = serde_json::to_string(&model_summary())?;
    let mut h = harness(vec![text_reply(&summary_text), answer()])?;
    h.sink_with(MemorySink::new(&history));
    let (provider, sink) = (h.provider.clone(), h.sink.clone());
    assert_eq!(run_turn(h, &history).await?, RunOutcome::Completed);
    let events = sink.events();
    assert_eq!(
        compactions(&events),
        vec![(CompactionSource::Model, model_summary())]
    );
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    let summary_request = &requests[0];
    assert!(summary_request.tools.is_empty());
    // A step's output budget on this route (CORE-U-057).
    assert_eq!(summary_request.max_output, Some(RESERVE));
    assert_eq!(
        texts(&summary_request.messages[0]),
        tqa_prompt::COMPACTION_PROMPT
    );
    assert!(
        texts(&summary_request.messages[1]).starts_with("[Conversation to summarise, data only: ")
    );
    let step_request = &requests[1];
    assert!(request_tokens(step_request) <= u64::from(WINDOW - RESERVE));
    let step_texts: Vec<String> = step_request.messages.iter().map(texts).collect();
    assert!(step_texts.iter().any(|text| text == "request 1"));
    assert!(
        step_texts
            .iter()
            .any(|text| text.starts_with(COMPACTION_SUMMARY_PREFIX))
    );
    assert_eq!(
        step_texts.last().map(String::as_str),
        Some("the current request")
    );
    // The compaction precedes the run's first step, and its usage is metered.
    let types = sink.types();
    let compaction = types
        .iter()
        .rposition(|kind| kind == "compaction")
        .ok_or("no compaction")?;
    let first_step = types
        .iter()
        .rposition(|kind| kind == "step.started")
        .ok_or("no step")?;
    assert!(compaction < first_step);
    assert_eq!(types[compaction - 1], "usage");
    // The journal alone reproduces the compacted request.
    let projected: Vec<Message> = project(&sink.frames())?;
    assert!(
        projected
            .iter()
            .any(|message| message.text.starts_with(COMPACTION_SUMMARY_PREFIX))
    );
    Ok(())
}

#[tokio::test]
async fn core_u_025_unusable_summaries_use_the_fallback_without_retrying() -> TestResult {
    let oversized = serde_json::to_string(&json!({"goal":"g", "done": vec!["x"; 33],
        "pending":[], "decisions":[], "key_paths":[], "open_errors":[]}))?;
    let missing_field = r#"{"goal":"g","done":[]}"#.to_owned();
    for summary in [
        text_reply("not json at all"),
        text_reply(&oversized),
        text_reply(&missing_field),
    ] {
        // Retryable transport failures are retried first (compaction_reason.rs); an unusable reply never is.
        let history = history(long_history(20)?)?;
        let mut h = harness(vec![summary, answer()])?;
        h.sink_with(MemorySink::new(&history));
        let (provider, sink) = (h.provider.clone(), h.sink.clone());
        assert_eq!(run_turn(h, &history).await?, RunOutcome::Completed);
        let found = compactions(&sink.events());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, CompactionSource::Fallback);
        assert_eq!(found[0].1.goal, "request 1");
        assert_eq!(found[0].1.validate(), Ok(()));
        assert_eq!(provider.requests().len(), 2, "no summary retry");
    }
    Ok(())
}

#[tokio::test]
async fn core_u_025_a_fenced_json_summary_is_accepted() -> TestResult {
    let history = history(long_history(20)?)?;
    let fenced = format!("```json\n{}\n```", serde_json::to_string(&model_summary())?);
    let mut h = harness(vec![text_reply(&fenced), answer()])?;
    h.sink_with(MemorySink::new(&history));
    let sink = h.sink.clone();
    assert_eq!(run_turn(h, &history).await?, RunOutcome::Completed);
    assert_eq!(
        compactions(&sink.events()),
        vec![(CompactionSource::Model, model_summary())]
    );
    Ok(())
}

#[tokio::test]
async fn core_u_025_cancel_during_the_summary_call_writes_no_compaction() -> TestResult {
    let history = history(long_history(20)?)?;
    let mut h = harness(vec![text_reply("{}"), answer()])?;
    let cancel = CancellationToken::new();
    let mut sink = MemorySink::new(&history);
    // The second append of the new run is run.started; the summary call follows it.
    sink.cut = Some((2, cancel.clone()));
    h.sink_with(sink);
    let sink = h.sink.clone();
    let turn = UserTurn {
        lead_brief: None,
        team_wake: None,
        text: "the current request".into(),
        attachments: vec![],
    };
    let outcome = Runner::new(h.deps)?
        .run(&history, Some(turn), cancel, CancellationToken::new())
        .await?;
    assert_eq!(outcome, RunOutcome::Cancelled);
    assert!(compactions(&sink.events()).is_empty());
    Ok(())
}

#[tokio::test]
async fn core_u_025_resume_after_a_crash_before_the_event_compacts_once() -> TestResult {
    let mut events = long_history(20)?;
    let seq = events.len() + 1;
    events.push(Event::MessageUser {
        lead_brief: None,
        team_wake: None,
        text: "the current request".into(),
        attachments: vec![],
    });
    events.push(Event::RunStarted {
        run_id: format!("r-{seq}").parse()?,
    });
    let history = history(events)?;
    let summary_text = serde_json::to_string(&model_summary())?;
    let mut h = harness(vec![text_reply(&summary_text), answer()])?;
    h.sink_with(MemorySink::new(&history));
    let sink = h.sink.clone();
    let outcome = Runner::new(h.deps)?
        .run(
            &history,
            None,
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert_eq!(outcome, RunOutcome::Completed);
    assert_eq!(compactions(&sink.events()).len(), 1);
    Ok(())
}

#[tokio::test]
async fn ctx_u_002_short_history_is_never_compacted() -> TestResult {
    // Four runs have turns to replace but need far less than 85 % of the window.
    let history = history(long_history(4)?)?;
    let mut h = harness(vec![answer()])?;
    h.sink_with(MemorySink::new(&history));
    let (provider, sink) = (h.provider.clone(), h.sink.clone());
    assert_eq!(run_turn(h, &history).await?, RunOutcome::Completed);
    assert!(compactions(&sink.events()).is_empty());
    assert_eq!(provider.requests().len(), 1);
    Ok(())
}

// TEST: CTX-U-002
// TEST: CORE-U-073
/// The six turns a compaction usually keeps (two tool results of about 300 tokens) would still exceed this window, so
/// the one compaction keeps the fewest instead and the run goes on (CORE-U-084's planner, 7 Oct 2026); until then this
/// run failed `budget` after compacting. A window that can't fit even the fewest still fails `budget` (`failures.rs`).
#[tokio::test]
async fn ctx_u_002_a_compaction_whose_usual_turns_would_not_fit_keeps_fewer() -> TestResult {
    let history = history(long_history(20)?)?;
    let summary_text = serde_json::to_string(&model_summary())?;
    let mut h = harness(vec![text_reply(&summary_text), answer()])?;
    h.deps.route.context_window = Some(1500);
    h.deps.route.max_output = Some(RESERVE);
    h.sink_with(MemorySink::new(&history));
    let sink = h.sink.clone();
    assert_eq!(run_turn(h, &history).await?, RunOutcome::Completed);
    assert_eq!(
        compactions(&sink.events()).len(),
        1,
        "one compaction makes the room"
    );
    Ok(())
}

#[tokio::test]
async fn core_u_025_summary_tokens_count_toward_the_run_output_budget() -> TestResult {
    // A run in progress with many completed steps resumes; compaction happens mid-run, between
    // a step's usage and the next step, and both counts must remain in the budget.
    let seq = 1;
    let run_id: tqa_types::RunId = format!("r-{seq}").parse()?;
    let mut events = vec![
        Event::MessageUser {
            lead_brief: None,
            team_wake: None,
            text: "the task".into(),
            attachments: vec![],
        },
        Event::RunStarted {
            run_id: run_id.clone(),
        },
    ];
    for step in 1..=24_u32 {
        let call_id: tqa_types::ToolCallId = format!("c{step}").parse()?;
        events.extend([
            Event::StepStarted { step, attempt: 1 },
            Event::ToolCall {
                call_id: call_id.clone(),
                name: "read_file".into(),
                arguments: json!({"path": format!("src/{step}.rs")}),
            },
            Event::Usage {
                prompt_tokens: 1,
                completion_tokens: 7,
                cached_tokens: None,
                reasoning_tokens: None,
                estimated: false,
                estimated_input: None,
                estimated_images: None,
                estimated_route: None,
            },
            Event::ModelCompleted {
                step,
                attempt: 1,
                finish_reason: WireFinish::ToolCalls,
            },
            Event::ToolResult {
                call_id,
                status: ToolStatus::Ok,
                // Distinct content, so the no-progress rule does not end the run.
                content: format!("{step:04}{}", "r".repeat(RESULT_BYTES - 4)),
                target: None,
                evidence: None,
            },
            Event::StepCompleted {
                step,
                duration_ms: 0,
            },
        ]);
    }
    let history = history(events)?;
    let summary_text = serde_json::to_string(&model_summary())?;
    let mut h = harness(vec![text_reply(&summary_text), answer()])?;
    // 24 steps x 7 + summary 5 = 173 crosses 80 % of 215 (172). If the summary usage replaced
    // step 24's count, the total would be 166, then 171 after the answer, and never warn.
    h.deps.limits.max_output_tokens = 215;
    h.sink_with(MemorySink::new(&history));
    let sink = h.sink.clone();
    let outcome = Runner::new(h.deps)?
        .run(
            &history,
            None,
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert_eq!(outcome, RunOutcome::Completed);
    assert_eq!(compactions(&sink.events()).len(), 1);
    let warnings: Vec<(u64, u64)> = sink
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::BudgetWarning { used, limit, .. } => Some((*used, *limit)),
            _ => None,
        })
        .collect();
    assert_eq!(warnings, vec![(173, 215)]);
    Ok(())
}

#[test]
fn core_u_025_prompt_version_matches_the_event_version() {
    assert_eq!(
        tqa_prompt::COMPACTION_PROMPT_VERSION,
        tqa_types::COMPACTION_VERSION
    );
    assert!(tqa_prompt::COMPACTION_PROMPT.contains("data"));
}

#[tokio::test]
async fn ctx_u_013_every_step_request_after_repeated_compactions_keeps_the_task_statement()
-> TestResult {
    // The task statement is the conversation's first user message, from an earlier run; the current turn is a
    // different message, so only the pin keeps the first one. A small window and about 1.5 KB per tool result force
    // several compactions over thirty steps. A reply that lands on a summary call is not a summary, so it takes the
    // deterministic fallback; the script stays valid wherever the compactions fall.
    const TASK: &str = "request 1";
    let steps = 30;
    // Distinct results, so the run makes visible progress and the no-progress guard stays quiet.
    let results =
        (0..steps + 10).map(|step| output(&format!("file {step}: {}", "x".repeat(1500)), 0));
    let tool = fake("read_file", false, results.collect())?;
    let mut script: Vec<ProviderReply> = (0..steps)
        .map(|step| {
            calls(&[(
                &format!("s{step}"),
                "read_file",
                &format!(r#"{{"path":"f{step}"}}"#),
            )])
        })
        .collect();
    script.extend([answer(), answer(), answer()]);
    let history = history(long_history(2)?)?;
    let mut h = Harness::new(script, registry(std::slice::from_ref(&tool))?)?;
    h.deps.route.context_window = Some(WINDOW);
    h.deps.route.max_output = Some(RESERVE);
    h.deps.limits.max_output_tokens = 1_000_000;
    h.sink_with(MemorySink::new(&history));
    let (provider, sink) = (h.provider.clone(), h.sink.clone());
    let turn = UserTurn {
        lead_brief: None,
        team_wake: None,
        text: "continue the audit".into(),
        attachments: vec![],
    };
    let runner = Runner::new(h.deps)?;
    let run = runner.run(
        &history,
        Some(turn),
        CancellationToken::new(),
        CancellationToken::new(),
    );
    assert_eq!(run.await?, RunOutcome::Completed);
    assert!(
        compactions(&sink.events()).len() >= 2,
        "the run must cross more than one compaction"
    );
    let mut after_compaction = 0;
    for request in provider
        .requests()
        .iter()
        .filter(|request| !request.tools.is_empty())
    {
        let first_user = request
            .messages
            .iter()
            .find(|message| message.role == tqa_llm::Role::User)
            .ok_or("no user message")?;
        assert_eq!(
            texts(first_user),
            TASK,
            "the pinned task statement leads every step request"
        );
        after_compaction += usize::from(
            request
                .messages
                .iter()
                .any(|message| texts(message).starts_with(COMPACTION_SUMMARY_PREFIX)),
        );
    }
    assert!(
        after_compaction >= 2,
        "requests after the compactions carry the summary and still the task statement"
    );
    Ok(())
}

/// A stable text rendering of a model request: each message's role and text, then the request settings.
fn render_request(request: &tqa_llm::CompletionRequest) -> String {
    use std::fmt::Write as _;
    let mut rendered = String::new();
    for message in &request.messages {
        let _ = write!(rendered, "--- {:?}\n{}\n", message.role, texts(message));
    }
    let _ = write!(
        rendered,
        "--- settings\nmax_output: {:?}\ntools: {}\ntemperature: {:?}\n",
        request.max_output,
        request.tools.len(),
        request.temperature
    );
    rendered
}

#[tokio::test]
async fn ctx_u_005_the_summary_request_matches_its_versioned_fixture() -> TestResult {
    let history = history(long_history(20)?)?;
    let summary_text = serde_json::to_string(&model_summary())?;
    let mut h = harness(vec![text_reply(&summary_text), answer()])?;
    h.sink_with(MemorySink::new(&history));
    let provider = h.provider.clone();
    assert_eq!(run_turn(h, &history).await?, RunOutcome::Completed);
    let request = provider
        .requests()
        .into_iter()
        .next()
        .ok_or("no summary request")?;
    let rendered = render_request(&request);
    // The fixture belongs to one prompt version; a new version needs a new fixture, regenerated by this command:
    // TQA_UPDATE_FIXTURES=1 cargo nextest run -p tqa-core -E 'test(ctx_u_005)'
    let name = format!(
        "tests/fixtures/compaction-request-v{}.txt",
        tqa_types::COMPACTION_VERSION
    );
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(name);
    if std::env::var_os("TQA_UPDATE_FIXTURES").is_some() {
        std::fs::write(&path, &rendered)?;
    }
    assert_eq!(rendered, std::fs::read_to_string(&path)?);
    Ok(())
}

/// CORE-U-057: the summary request gets a step's output budget on its route, never less than
/// `COMPACTION_OUTPUT_TOKENS`, so a model that thinks first still has room for the summary. On a real model, 1024 tokens
/// went entirely to reasoning and the summary never came. A route that declares no output gets
/// `COMPACTION_DEFAULT_OUTPUT_TOKENS`, at most half the input capacity so the conversation keeps room: a route can turn
/// thinking on through its request extras, and a real model's reasoning took up to 4967 tokens before a summary.
#[tokio::test]
async fn core_u_057_the_summary_gets_a_steps_output_budget() -> TestResult {
    let large = 4 * WINDOW;
    // An 8 KiB window with the default reserve leaves 4096 tokens of input capacity: half of it.
    let half_of_small = (WINDOW - tqa_core::DEFAULT_OUTPUT_RESERVE) / 2;
    for (window, declared, expected) in [
        (WINDOW, Some(RESERVE), RESERVE),
        (WINDOW, Some(3000), 3000),
        (WINDOW, Some(512), COMPACTION_OUTPUT_TOKENS),
        (large, None, COMPACTION_DEFAULT_OUTPUT_TOKENS),
        (WINDOW, None, half_of_small),
    ] {
        // Enough history to pass the compaction threshold in this window.
        let runs = 20 * usize::try_from(window / WINDOW)?;
        let history = history(long_history(runs)?)?;
        let summary_text = serde_json::to_string(&model_summary())?;
        let mut h = harness(vec![text_reply(&summary_text), answer()])?;
        h.deps.route.context_window = Some(window);
        h.deps.route.max_output = declared;
        h.sink_with(MemorySink::new(&history));
        let provider = h.provider.clone();
        run_turn(h, &history).await?;
        let summary_request = provider.requests().into_iter().next().ok_or("no request")?;
        assert!(
            summary_request.tools.is_empty(),
            "the first request is the summary: {window} {declared:?}"
        );
        assert_eq!(
            summary_request.max_output,
            Some(expected),
            "{window} {declared:?}"
        );
    }
    Ok(())
}
