//! CORE-U-071: a route with no Max output means the model server's own limit: on Chat Completions and Responses the
//! request carries no output limit (the context window still plans with the documented reserve), while a set Max output
//! is sent as it is. CORE-U-072: a step the model ends at its output limit after only thinking (no text, no call) fails
//! `reply_cut` saying the model spent its whole output limit thinking (candidate 12: qwen3:4b spent 4096 tokens thinking
//! on step 1), so the person knows a larger Max output, a larger server context or less thinking is the way on. It
//! isn't asked again with more room: every bound on that room already bounds the request (the route's own Max output,
//! the window, the run's budget).
mod support;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{DEFAULT_OUTPUT_RESERVE, RunOutcome, Runner, RunnerDeps};
use tqa_llm::{FinishReason, StreamChunk, Usage};
use tqa_tools::ToolRegistry;
use tqa_types::{ErrorCode, Event, WireProtocol};

async fn run(deps: RunnerDeps) -> Result<RunOutcome, Box<dyn std::error::Error>> {
    Ok(Runner::new(deps)?
        .run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?)
}

fn spent(completion: u64) -> StreamChunk {
    StreamChunk::Usage(Usage {
        prompt_tokens: 900,
        completion_tokens: completion,
        cached_tokens: None,
        reasoning_tokens: Some(completion),
    })
}

/// The model thinks until the limit and writes nothing else.
fn thinking_cut(completion: u64) -> tqa_testkit::ProviderReply {
    reply(vec![
        StreamChunk::ReasoningDelta("Let me think about which file to read first. ".repeat(40)),
        spent(completion),
        StreamChunk::Finish(FinishReason::Length),
    ])
}

fn retries(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::ProviderRetry { reason, .. } => Some(reason.clone()),
            _ => None,
        })
        .collect()
}

// TEST: CORE-U-071
#[tokio::test]
async fn core_u_071_no_max_output_sends_no_limit_on_chat_and_responses() -> TestResult {
    for protocol in [WireProtocol::OpenAiChat, WireProtocol::OpenAiResponses] {
        let mut h = Harness::new(vec![answer()], ToolRegistry::new())?;
        h.deps.route.protocol = protocol;
        h.deps.route.max_output = None;
        assert_eq!(run(h.deps).await?, RunOutcome::Completed, "{protocol:?}");
        assert_eq!(h.provider.requests()[0].max_output, None, "{protocol:?}");
    }
    Ok(())
}

// TEST: CORE-U-071
#[tokio::test]
async fn core_u_071_a_set_max_output_is_sent_and_the_window_plans_with_the_reserve() -> TestResult {
    let mut h = Harness::new(vec![answer()], ToolRegistry::new())?;
    h.deps.route.max_output = Some(2048);
    assert_eq!(run(h.deps).await?, RunOutcome::Completed);
    assert_eq!(h.provider.requests()[0].max_output, Some(2048));
    // Planning only: the reserve the window keeps for an answer when the route names no limit.
    assert_eq!(DEFAULT_OUTPUT_RESERVE, 4096);
    Ok(())
}

// TEST: CORE-U-071
#[tokio::test]
async fn core_u_071_with_no_limit_sent_the_runs_own_budget_still_stops_a_reply() -> TestResult {
    let mut h = Harness::new(
        vec![reply(vec![
            StreamChunk::TextDelta("a long answer".into()),
            spent(150),
            StreamChunk::Finish(FinishReason::Stop),
        ])],
        ToolRegistry::new(),
    )?;
    h.deps.route.max_output = None;
    h.deps.limits.max_output_tokens = 100;
    assert_eq!(
        run(h.deps).await?,
        RunOutcome::Failed {
            code: ErrorCode::Budget,
            reason: "budget.output_tokens".into(),
        }
    );
    assert_eq!(h.provider.requests()[0].max_output, None);
    Ok(())
}

// TEST: CORE-U-072
#[tokio::test]
async fn core_u_072_a_step_spent_thinking_says_so_and_isnt_asked_again() -> TestResult {
    let mut h = Harness::new(vec![thinking_cut(4096)], ToolRegistry::new())?;
    h.deps.route.max_output = Some(4096);
    let sink = h.sink.clone();
    assert_eq!(
        run(h.deps).await?,
        RunOutcome::Failed {
            code: ErrorCode::ReplyCut,
            reason: "the model spent its whole output limit thinking (4096 tokens) and wrote no answer; 4096 requested"
                .into(),
        }
    );
    assert_eq!(h.provider.requests().len(), 1, "no retry");
    assert!(retries(&sink.events()).is_empty());
    Ok(())
}

// TEST: CORE-U-072
#[tokio::test]
async fn core_u_072_without_a_limit_it_says_so_too_and_a_cut_answer_keeps_its_words() -> TestResult
{
    // No limit sent: the cut was the model server's own limit or its context.
    let mut h = Harness::new(vec![thinking_cut(4096)], ToolRegistry::new())?;
    h.deps.route.max_output = None;
    assert!(matches!(
        run(h.deps).await?,
        RunOutcome::Failed { code: ErrorCode::ReplyCut, ref reason } if reason.starts_with("the model spent its whole")
    ));
    assert_eq!(h.provider.requests().len(), 1);
    // A cut answer (text, no thinking) keeps today's reply_cut, with no retry.
    let mut h = Harness::new(
        vec![reply(vec![
            StreamChunk::TextDelta("partial answer".into()),
            spent(4096),
            StreamChunk::Finish(FinishReason::Length),
        ])],
        ToolRegistry::new(),
    )?;
    h.deps.route.max_output = Some(4096);
    assert!(matches!(
        run(h.deps).await?,
        RunOutcome::Failed { code: ErrorCode::ReplyCut, ref reason } if reason.starts_with("the model stopped at its output limit")
    ));
    assert_eq!(h.provider.requests().len(), 1);
    Ok(())
}
