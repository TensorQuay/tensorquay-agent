//! CORE-U-066: a reply the model ends at its output limit (`finish_reason = length`) while the run still has output
//! budget left was cut by the model server or the route's max output, not by the run's budget: the run fails as
//! `reply_cut`, and the reason names the tokens the reply used and the tokens requested. Only a reply that spends the
//! run's own budget is `budget.output_tokens`. A cut tool call never runs.
mod support;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner, RunnerDeps};
use tqa_llm::{FinishReason, StreamChunk, Usage};
use tqa_tools::ToolRegistry;
use tqa_types::ErrorCode;

fn tokens(count: u64) -> StreamChunk {
    StreamChunk::Usage(Usage {
        prompt_tokens: 10,
        completion_tokens: count,
        cached_tokens: None,
        reasoning_tokens: None,
    })
}

fn cut(used: u64, requested: u32) -> RunOutcome {
    RunOutcome::Failed {
        code: ErrorCode::ReplyCut,
        reason: format!(
            "the model stopped at its output limit (finish: length) after {used} tokens; {requested} requested"
        ),
    }
}

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

#[tokio::test]
async fn core_u_066_a_text_reply_cut_with_budget_left_is_reply_cut_with_its_numbers() -> TestResult
{
    let mut h = Harness::new(
        vec![reply(vec![
            StreamChunk::TextDelta("partial answer".into()),
            tokens(40),
            StreamChunk::Finish(FinishReason::Length),
        ])],
        ToolRegistry::new(),
    )?;
    h.deps.limits.max_output_tokens = 1_000;
    let outcome = run(h.deps).await?;
    let requested = h.provider.requests()[0]
        .max_output
        .ok_or("no max_output requested")?;
    assert_eq!(outcome, cut(40, requested));
    assert_eq!(h.provider.requests().len(), 1, "a cut reply is not retried");
    Ok(())
}

#[tokio::test]
async fn core_u_066_a_reply_that_spends_the_runs_own_budget_is_still_the_budget() -> TestResult {
    let mut h = Harness::new(
        vec![reply(vec![
            StreamChunk::TextDelta("partial answer".into()),
            tokens(100),
            StreamChunk::Finish(FinishReason::Length),
        ])],
        ToolRegistry::new(),
    )?;
    h.deps.limits.max_output_tokens = 100;
    assert_eq!(
        run(h.deps).await?,
        RunOutcome::Failed {
            code: ErrorCode::Budget,
            reason: "budget.output_tokens".into(),
        }
    );
    Ok(())
}

#[tokio::test]
async fn core_u_066_a_reasoning_only_cut_is_reply_cut() -> TestResult {
    let mut h = Harness::new(
        vec![reply(vec![
            StreamChunk::ReasoningDelta("thinking about it".into()),
            tokens(50),
            StreamChunk::Finish(FinishReason::Length),
        ])],
        ToolRegistry::new(),
    )?;
    h.deps.limits.max_output_tokens = 1_000;
    let outcome = run(h.deps).await?;
    let requested = h.provider.requests()[0]
        .max_output
        .ok_or("no max_output requested")?;
    // Only thinking: it says the model spent its limit thinking (CORE-U-072).
    assert_eq!(
        outcome,
        RunOutcome::Failed {
            code: ErrorCode::ReplyCut,
            reason: format!(
                "the model spent its whole output limit thinking (50 tokens) and wrote no answer; {requested} requested"
            ),
        }
    );
    assert_eq!(h.provider.requests().len(), 1);
    Ok(())
}

#[tokio::test]
async fn core_u_066_a_cut_tool_call_is_reply_cut_and_never_runs() -> TestResult {
    let tool = fake("read_file", true, vec![])?;
    let mut h = Harness::new(
        vec![
            reply(vec![
                StreamChunk::ToolCallDelta {
                    index: 0,
                    id: Some("a".into()),
                    name: Some("read_file".into()),
                    arguments_delta: r#"{"path":"a"}"#.into(),
                },
                tokens(30),
                StreamChunk::Finish(FinishReason::Length),
            ]),
            answer(),
        ],
        registry(std::slice::from_ref(&tool))?,
    )?;
    h.deps.limits.max_output_tokens = 1_000;
    let outcome = run(h.deps).await?;
    let requested = h.provider.requests()[0]
        .max_output
        .ok_or("no max_output requested")?;
    assert_eq!(outcome, cut(30, requested));
    assert!(tool.calls().is_empty(), "a cut tool call never runs");
    assert!(
        !h.sink
            .events()
            .iter()
            .any(|event| matches!(event, tqa_types::Event::ToolCall { .. })),
        "a cut tool call is never journaled, so the page never shows a call that didn't run"
    );
    assert_eq!(h.provider.requests().len(), 1);
    Ok(())
}
