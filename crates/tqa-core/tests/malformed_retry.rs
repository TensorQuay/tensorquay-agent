//! CORE-E-040: an answer that couldn't be read (an unparsable chunk, an inconsistent tool call) is asked again once, as
//! a cut stream is; a second fails the run; a refused request is never retried (13h Gate 5, 2 Oct 2026).
mod support;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_llm::LlmError;
use tqa_testkit::ProviderReply;
use tqa_tools::ToolRegistry;
use tqa_types::{ErrorCode, Event};

fn failure(error: LlmError) -> ProviderReply {
    ProviderReply {
        delay: std::time::Duration::ZERO,
        result: Err(error),
    }
}

async fn outcome(
    replies: Vec<ProviderReply>,
) -> Result<(RunOutcome, Vec<String>, usize), Box<dyn std::error::Error>> {
    let h = Harness::new(replies, ToolRegistry::new())?;
    let runner = Runner::new(h.deps)?;
    let outcome = drive(
        runner.run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        ),
        &h.clock,
    )
    .await??;
    let retries = h
        .sink
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::ProviderRetry { reason, .. } => Some(reason.clone()),
            _ => None,
        })
        .collect();
    assert_closed(&h.sink.frames())?;
    Ok((outcome, retries, h.provider.requests().len()))
}

#[tokio::test]
async fn core_e_040_a_malformed_answer_is_asked_again_once() -> TestResult {
    let malformed = || {
        failure(LlmError::Malformed(
            "tool call is missing its identifier or name".into(),
        ))
    };
    let (done, retries, requests) = outcome(vec![malformed(), answer()]).await?;
    assert_eq!(
        (done, retries, requests),
        (RunOutcome::Completed, vec!["malformed".to_owned()], 2)
    );
    let (failed, retries, requests) = outcome(vec![malformed(), malformed(), answer()]).await?;
    assert_eq!(
        (failed, retries, requests),
        (
            RunOutcome::Failed {
                code: ErrorCode::ProviderError,
                reason: "malformed".into()
            },
            vec!["malformed".to_owned()],
            2
        ),
        "one retry only"
    );
    Ok(())
}

#[tokio::test]
async fn core_e_040_a_refused_request_is_never_retried() -> TestResult {
    let (failed, retries, requests) = outcome(vec![
        failure(LlmError::Protocol("temperature must be finite".into())),
        answer(),
    ])
    .await?;
    assert_eq!(
        (failed, retries.len(), requests),
        (
            RunOutcome::Failed {
                code: ErrorCode::ProviderError,
                reason: "protocol".into()
            },
            0,
            1
        )
    );
    Ok(())
}
