//! SEC-036 (case 4) and PROTO-U-022's exposure half: an engine sees only what its history exposure allows (ADR-0013
//! point 5). With `none`, the default, a response's calls go by id and name only and nothing of the transcript reaches
//! it; `calls` adds the calls' arguments; `full` adds the response's text and the transcript. The loop filters, so the
//! same holds for an in-process engine and for the relay adapter.
mod support;
use async_trait::async_trait;
use std::{
    error::Error,
    sync::{Arc, Mutex},
};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{
    Decision, DecisionEngine, EngineFailure, ModelDecides, ModelResponse, PlannedAction, RunState,
    Runner,
};
use tqa_llm::{FinishReason, StreamChunk};
use tqa_types::DecisionExposure;

/// A named engine with an exposure, recording every state and response it is shown.
struct Seer {
    exposure: DecisionExposure,
    states: Mutex<Vec<RunState>>,
    responses: Mutex<Vec<ModelResponse>>,
}
impl Seer {
    fn new(exposure: DecisionExposure) -> Arc<Self> {
        Arc::new(Self {
            exposure,
            states: Mutex::new(Vec::new()),
            responses: Mutex::new(Vec::new()),
        })
    }
    /// Everything it was shown, as text.
    fn seen(&self) -> String {
        let states = self
            .states
            .lock()
            .map(|states| format!("{states:?}"))
            .unwrap_or_default();
        let responses = self
            .responses
            .lock()
            .map(|responses| format!("{responses:?}"))
            .unwrap_or_default();
        format!("{states}{responses}")
    }
}
#[async_trait]
impl DecisionEngine for Seer {
    fn name(&self) -> Option<&str> {
        Some("router")
    }
    fn exposure(&self) -> DecisionExposure {
        self.exposure
    }
    async fn before_step(&self, state: &RunState) -> Result<Decision, EngineFailure> {
        if let Ok(mut states) = self.states.lock() {
            states.push(state.clone());
        }
        ModelDecides.before_step(state).await
    }
    async fn after_response(
        &self,
        state: &RunState,
        response: &ModelResponse,
    ) -> Result<Vec<PlannedAction>, EngineFailure> {
        if let Ok(mut responses) = self.responses.lock() {
            responses.push(response.clone());
        }
        ModelDecides.after_response(state, response).await
    }
}

/// A run that reads a secret file: the model says why, calls `read_file`, and the tool returns the secret.
async fn run(exposure: DecisionExposure) -> Result<Arc<Seer>, Box<dyn Error>> {
    let tool = fake("read_file", true, vec![output("TOP SECRET CONTENT", 0)])?;
    let first = reply(vec![
        StreamChunk::TextDelta("I'll read the secret file".into()),
        StreamChunk::ToolCallDelta {
            index: 0,
            id: Some("a".into()),
            name: Some("read_file".into()),
            arguments_delta: r#"{"path":"secret.txt"}"#.into(),
        },
        usage(),
        StreamChunk::Finish(FinishReason::ToolCalls),
    ]);
    let mut h = Harness::new(
        vec![first, answer()],
        registry(std::slice::from_ref(&tool))?,
    )?;
    let engine = Seer::new(exposure);
    h.deps.decision = engine.clone();
    let runner = Runner::new(h.deps)?;
    drive(
        runner.run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        ),
        &h.clock,
    )
    .await??;
    Ok(engine)
}

#[tokio::test]
async fn sec_036_with_exposure_none_no_transcript_content_reaches_the_engine() -> TestResult {
    let engine = run(DecisionExposure::None).await?;
    let responses = engine.responses.lock().map_err(|_| "poisoned")?.clone();
    assert_eq!(responses.len(), 1);
    let call = &responses[0].tool_calls[0];
    assert_eq!(
        (call.id.as_str(), call.name.as_str()),
        ("a", "read_file"),
        "ids and names stay"
    );
    assert_eq!(call.arguments, "{}", "arguments are left out");
    assert!(responses[0].text.is_empty() && responses[0].reasoning.is_empty());
    let seen = engine.seen();
    for secret in ["secret.txt", "TOP SECRET", "I'll read", "inspect files"] {
        assert!(
            !seen.contains(secret),
            "{secret:?} reached the engine: {seen}"
        );
    }
    let states = engine.states.lock().map_err(|_| "poisoned")?.clone();
    assert_eq!(states[1].previous_calls[0].name, "read_file");
    assert_eq!(states[1].previous_calls[0].arguments, None);
    assert_eq!(states[1].transcript, None);
    Ok(())
}

#[tokio::test]
async fn proto_u_022_exposure_calls_adds_arguments_and_full_adds_the_transcript() -> TestResult {
    let calls = run(DecisionExposure::Calls).await?;
    let seen = calls.seen();
    assert!(seen.contains("secret.txt"), "calls: the arguments");
    assert!(
        !seen.contains("TOP SECRET") && !seen.contains("I'll read"),
        "calls: no transcript: {seen}"
    );
    let states = calls.states.lock().map_err(|_| "poisoned")?.clone();
    assert_eq!(
        states[1].previous_calls[0].arguments,
        Some(serde_json::json!({"path":"secret.txt"}))
    );
    assert_eq!(states[1].transcript, None);

    let full = run(DecisionExposure::Full).await?;
    let seen = full.seen();
    for part in [
        "secret.txt",
        "I'll read the secret file",
        "TOP SECRET CONTENT",
    ] {
        assert!(seen.contains(part), "full: {part:?}");
    }
    Ok(())
}
