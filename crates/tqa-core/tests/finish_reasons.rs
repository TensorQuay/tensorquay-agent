//! LLM-C-002 at the loop boundary: every provider finish reason reaches the journal as its stable wire reason, so the
//! transcript can say why an answer ended.
mod support;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_llm::{FinishReason, StreamChunk};
use tqa_tools::ToolRegistry;
use tqa_types::{Event, FinishReason as WireFinish};

async fn finished_with(
    reason: FinishReason,
) -> Result<Vec<WireFinish>, Box<dyn std::error::Error>> {
    let h = Harness::new(
        vec![reply(vec![
            StreamChunk::TextDelta("an answer".into()),
            usage(),
            StreamChunk::Finish(reason),
        ])],
        ToolRegistry::new(),
    )?;
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
    Ok(sink
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::ModelCompleted { finish_reason, .. } => Some(*finish_reason),
            _ => None,
        })
        .collect())
}

#[tokio::test]
async fn llm_c_002_a_filtered_or_unknown_finish_is_journalled_as_such() -> TestResult {
    assert_eq!(
        finished_with(FinishReason::ContentFilter).await?,
        [WireFinish::ContentFilter]
    );
    assert_eq!(
        finished_with(FinishReason::Other("vendor_specific".into())).await?,
        [WireFinish::Other]
    );
    assert_eq!(finished_with(FinishReason::Stop).await?, [WireFinish::Stop]);
    Ok(())
}
