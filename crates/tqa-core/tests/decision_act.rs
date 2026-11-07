//! CORE-U-033 (acting) and SEC-036 (cases 1 and 2): a named engine's `act` runs one call the loop originates, through
//! the same path as a model call (schema, the prepared-action hash, budgets), as its own step without a model call;
//! the next request shows it as a user-role note. A tool outside the step's set is `tool_not_allowed`, arguments that
//! fail the tool's schema are `schema`, and a fourth `act` in a row is `act_loop` (ADR-0013 point 2): the engine can't
//! widen the task's tools.
mod support;
use async_trait::async_trait;
use std::{
    collections::VecDeque,
    error::Error,
    sync::{Arc, Mutex},
};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{
    Decision, DecisionEngine, EngineFailure, ModelDecides, ModelResponse, PlannedAction,
    RunOutcome, RunState, Runner,
};
use tqa_llm::ToolCall;
use tqa_types::{DecisionChoice, DecisionFallbackReason, DecisionPoint, Event, ToolStatus};

/// A named engine that gives its answers in order, then answers as the model would.
struct Sequence {
    answers: Mutex<VecDeque<Decision>>,
}
impl Sequence {
    fn new(answers: Vec<Decision>) -> Arc<Self> {
        Arc::new(Self {
            answers: Mutex::new(answers.into()),
        })
    }
}
#[async_trait]
impl DecisionEngine for Sequence {
    fn name(&self) -> Option<&str> {
        Some("router")
    }
    async fn before_step(&self, state: &RunState) -> Result<Decision, EngineFailure> {
        let next = self
            .answers
            .lock()
            .ok()
            .and_then(|mut answers| answers.pop_front());
        match next {
            Some(decision) => Ok(decision),
            None => ModelDecides.before_step(state).await,
        }
    }
    async fn after_response(
        &self,
        state: &RunState,
        response: &ModelResponse,
    ) -> Result<Vec<PlannedAction>, EngineFailure> {
        ModelDecides.after_response(state, response).await
    }
}
fn act(tool: &str, arguments: &str) -> Decision {
    Decision::Act(ToolCall {
        id: String::new(),
        name: tool.into(),
        arguments: arguments.into(),
    })
}
async fn run(h: Harness) -> Result<(RunOutcome, Vec<Event>), Box<dyn Error>> {
    let sink = h.sink.clone();
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
    Ok((outcome, sink.events()))
}
fn fallbacks(events: &[Event]) -> Vec<DecisionFallbackReason> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::DecisionFallback {
                reason,
                point: DecisionPoint::BeforeStep,
                ..
            } => Some(*reason),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn core_u_033_an_act_runs_one_call_as_its_own_step_and_the_model_sees_a_note() -> TestResult {
    let tool = fake("read_file", true, vec![output("fn main() {}", 0)])?;
    let mut h = Harness::new(vec![answer()], registry(std::slice::from_ref(&tool))?)?;
    h.deps.decision = Sequence::new(vec![act("read_file", r#"{"path":"a.rs"}"#)]);
    let provider = h.provider.clone();
    let (outcome, events) = run(h).await?;
    assert_eq!(outcome, RunOutcome::Completed);
    assert_eq!(tool.calls(), vec![serde_json::json!({"path":"a.rs"})]);
    let order: Vec<&str> = events
        .iter()
        .filter_map(|event| match event {
            Event::Decision {
                choice: DecisionChoice::Act { .. },
                ..
            } => Some("act"),
            Event::StepStarted { step: 1, .. } => Some("step 1"),
            Event::ToolCall { call_id, .. } if call_id.as_str() == "de-1-1" => Some("call"),
            Event::ModelCompleted { step: 1, .. } => Some("model 1"),
            Event::ToolResult {
                status: ToolStatus::Ok,
                ..
            } => Some("result"),
            _ => None,
        })
        .collect();
    assert_eq!(
        order,
        ["act", "step 1", "call", "result"],
        "no model in step 1"
    );
    let requests = provider.requests();
    assert_eq!(requests.len(), 1, "only step 2 calls the model");
    let messages = &requests[0].messages;
    assert!(
        messages.iter().all(|message| message.tool_calls.is_empty()),
        "no call the model didn't make"
    );
    let note = messages
        .iter()
        .flat_map(|message| &message.content)
        .filter_map(|part| match part {
            tqa_llm::ContentPart::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .find(|text| text.contains("not the model"))
        .ok_or("no note")?;
    assert!(
        note.contains("read_file") && note.contains("fn main() {}"),
        "{note}"
    );
    Ok(())
}

#[tokio::test]
async fn core_u_033_an_act_outside_the_step_or_its_schema_falls_back() -> TestResult {
    for (decision, code) in [
        (
            act("write_file", r#"{"path":"a"}"#),
            DecisionFallbackReason::ToolNotAllowed,
        ),
        (act("read_file", "not json"), DecisionFallbackReason::Schema),
        (act("read_file", "[1]"), DecisionFallbackReason::Schema),
    ] {
        let tool = fake("read_file", true, vec![])?;
        let mut h = Harness::new(vec![answer()], registry(std::slice::from_ref(&tool))?)?;
        h.deps.decision = Sequence::new(vec![decision]);
        let (outcome, events) = run(h).await?;
        assert_eq!(outcome, RunOutcome::Completed);
        assert_eq!(fallbacks(&events), [code]);
        assert!(tool.calls().is_empty());
    }
    Ok(())
}

#[tokio::test]
async fn core_u_033_a_fourth_act_in_a_row_is_act_loop_and_the_model_decides() -> TestResult {
    let tool = fake("read_file", true, (0..3).map(|_| output("ok", 0)).collect())?;
    let mut h = Harness::new(vec![answer()], registry(std::slice::from_ref(&tool))?)?;
    h.deps.decision = Sequence::new(
        (0..4)
            .map(|_| act("read_file", r#"{"path":"a"}"#))
            .collect(),
    );
    let provider = h.provider.clone();
    let (outcome, events) = run(h).await?;
    assert_eq!(outcome, RunOutcome::Completed);
    assert_eq!(tool.calls().len(), 3);
    assert_eq!(fallbacks(&events), [DecisionFallbackReason::ActLoop]);
    assert_eq!(provider.requests().len(), 1, "step 4 calls the model");
    Ok(())
}
