//! CTX-U-013 (repeated compaction): every compaction of a long run takes the model's summary when the summary call
//! returns a valid one. The compaction-50 demo's second compaction fell back; this reproduces its shape in core.
mod support;
use async_trait::async_trait;
use std::sync::Arc;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_llm::{
    CompletionRequest, CompletionStream, FinishReason, LlmError, ModelProvider, Route, StreamChunk,
};
use tqa_testkit::{FakeProvider, ProviderReply};
use tqa_types::{CompactionSource, CompactionSummary, Event};

const WINDOW: u32 = 16_384;
const RESERVE: u32 = 1024;
const STEPS: usize = 60;

/// Summary calls (requests without tools) go to their own script, so the step script stays valid wherever the
/// compactions fall, like the fake model server's summary mode.
struct SummaryAware {
    steps: Arc<FakeProvider>,
    summaries: Arc<FakeProvider>,
}
#[async_trait]
impl ModelProvider for SummaryAware {
    fn route(&self) -> &Route {
        self.steps.route()
    }
    async fn complete(
        &self,
        request: CompletionRequest,
        cancel: CancellationToken,
    ) -> Result<CompletionStream, LlmError> {
        if request.tools.is_empty() {
            self.summaries.complete(request, cancel).await
        } else {
            self.steps.complete(request, cancel).await
        }
    }
}

fn summary_reply() -> Result<ProviderReply, serde_json::Error> {
    let summary = CompactionSummary {
        goal: "audit the ledger".into(),
        done: vec!["read the earlier parts".into()],
        pending: vec!["read the remaining parts".into()],
        key_paths: vec!["ledger/".into()],
        ..CompactionSummary::default()
    };
    Ok(reply(vec![
        StreamChunk::TextDelta(serde_json::to_string(&summary)?),
        usage(),
        StreamChunk::Finish(FinishReason::Stop),
    ]))
}

#[tokio::test]
async fn ctx_u_013_every_compaction_of_a_long_run_takes_a_valid_model_summary() -> TestResult {
    let script: Vec<_> = (0..STEPS)
        .map(|step| {
            calls(&[(
                &format!("c{step}"),
                "read_file",
                &format!(r#"{{"path":"part-{step:02}"}}"#),
            )])
        })
        .chain(std::iter::once(answer()))
        .collect();
    let outputs = (0..STEPS)
        .map(|step| output(&format!("{step:04}{}", "l".repeat(2000)), 0))
        .collect();
    let tool = fake("read_file", true, outputs)?;
    let mut h = Harness::new(script, registry(&[tool])?)?;
    let summaries = (0..8)
        .map(|_| summary_reply())
        .collect::<Result<Vec<_>, _>>()?;
    let summaries = Arc::new(FakeProvider::new(
        h.provider.route().clone(),
        summaries,
        Arc::new(h.sleeper.clone()),
    ));
    h.deps.provider = Arc::new(SummaryAware {
        steps: h.provider.clone(),
        summaries: summaries.clone(),
    });
    h.deps.route.context_window = Some(WINDOW);
    h.deps.route.max_output = Some(RESERVE);
    h.deps.limits.max_steps = 100;
    h.deps.limits.max_output_tokens = 1_000_000;
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
    let sources: Vec<_> = sink
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::Compaction { source, .. } => Some(*source),
            _ => None,
        })
        .collect();
    assert!(
        sources.len() >= 2,
        "the run compacts more than once: {sources:?}"
    );
    assert!(
        sources
            .iter()
            .all(|source| *source == CompactionSource::Model),
        "every compaction uses the valid model summary: {sources:?}, {} summary calls",
        summaries.requests().len()
    );
    Ok(())
}
