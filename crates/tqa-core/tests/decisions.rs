//! CORE-U-032: invalid engine decisions fall back and valid subsets stay restrictive; the engine is asked once per step
//! (ADR-0013 decision 1), sees the step's allowed routes, budget use and the previous step's calls, and a failure it
//! reports falls back like an invalid decision.
mod support;
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{
    CallSummary, Decision, DecisionEngine, EngineFailure, ModelDecides, ModelResponse,
    PlannedAction, RunState, Runner,
};
use tqa_types::ToolStatus;

struct ScriptEngine {
    before: Decision,
    duplicate: bool,
}
#[async_trait]
impl DecisionEngine for ScriptEngine {
    async fn before_step(&self, _: &RunState) -> Result<Decision, EngineFailure> {
        Ok(self.before.clone())
    }
    async fn after_response(
        &self,
        _: &RunState,
        response: &ModelResponse,
    ) -> Result<Vec<PlannedAction>, EngineFailure> {
        let mut actions: Vec<_> = response
            .tool_calls
            .iter()
            .map(|call| PlannedAction {
                call_id: call.id.clone(),
            })
            .collect();
        if self.duplicate {
            actions.extend(actions.clone());
        }
        Ok(actions)
    }
}

/// Records every state it is asked about and answers as the model would, or fails as told.
#[derive(Default)]
struct Recorder {
    states: Mutex<Vec<RunState>>,
    fail: Option<EngineFailure>,
}
#[async_trait]
impl DecisionEngine for Recorder {
    async fn before_step(&self, state: &RunState) -> Result<Decision, EngineFailure> {
        self.states
            .lock()
            .map_err(|_| EngineFailure::Invalid)?
            .push(state.clone());
        match self.fail {
            Some(failure) => Err(failure),
            None => ModelDecides.before_step(state).await,
        }
    }
    async fn after_response(
        &self,
        state: &RunState,
        response: &ModelResponse,
    ) -> Result<Vec<PlannedAction>, EngineFailure> {
        match self.fail {
            Some(failure) => Err(failure),
            None => ModelDecides.after_response(state, response).await,
        }
    }
}

#[tokio::test]
async fn core_u_032_the_engine_is_asked_once_per_step_not_per_attempt() -> TestResult {
    let tool = fake("read_file", true, vec![output("ok", 0)])?;
    // Step 1's first attempt fails and is retried; step 2 answers.
    let mut h = Harness::new(
        vec![
            http(500),
            calls(&[("a", "read_file", r#"{"path":"a"}"#)]),
            answer(),
        ],
        registry(std::slice::from_ref(&tool))?,
    )?;
    let engine = Arc::new(Recorder::default());
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
    let states = engine.states.lock().map_err(|_| "poisoned")?.clone();
    assert_eq!(
        states.iter().map(|state| state.step).collect::<Vec<_>>(),
        [1, 2],
        "one consultation per step, the retry reuses it"
    );
    assert_eq!(states[0].attempt, 1);
    assert_eq!(states[0].routes, ["test"]);
    assert_eq!(states[0].budget.steps_used, 0);
    assert!(states[0].previous_calls.is_empty());
    assert_eq!(states[1].budget.steps_used, 1);
    assert_eq!(states[1].budget.max_steps, 50);
    assert!(states[1].budget.output_tokens_used > 0);
    assert_eq!(
        states[1].previous_calls,
        [CallSummary {
            name: "read_file".into(),
            status: Some(ToolStatus::Ok),
            arguments: None,
        }]
    );
    Ok(())
}

#[tokio::test]
async fn core_u_032_a_failure_the_engine_reports_falls_back_to_the_model() -> TestResult {
    for failure in [
        EngineFailure::Unreachable,
        EngineFailure::Timeout,
        EngineFailure::Invalid,
    ] {
        let tool = fake("read_file", true, vec![output("ok", 0)])?;
        let mut h = Harness::new(
            vec![calls(&[("a", "read_file", r#"{"path":"a"}"#)]), answer()],
            registry(std::slice::from_ref(&tool))?,
        )?;
        h.deps.decision = Arc::new(Recorder {
            fail: Some(failure),
            ..Recorder::default()
        });
        let outcome = Runner::new(h.deps)?
            .run(
                &[],
                Some(turn()),
                CancellationToken::new(),
                CancellationToken::new(),
            )
            .await?;
        assert_eq!(outcome, tqa_core::RunOutcome::Completed, "{failure:?}");
        assert_eq!(tool.calls().len(), 1, "{failure:?}: the model's call ran");
    }
    Ok(())
}
#[tokio::test]
async fn core_u_032_invalid_before_decisions_fall_back_to_registered_tools() -> TestResult {
    for decision in [
        Decision::Compact,
        Decision::Stop("stop".into()),
        Decision::Act(tqa_llm::ToolCall {
            id: "invented".into(),
            name: "read_file".into(),
            arguments: "{}".into(),
        }),
        Decision::CallModel {
            route: "other".into(),
            tool_subset: vec![],
        },
        Decision::CallModel {
            route: "test".into(),
            tool_subset: vec!["missing".into()],
        },
        Decision::CallModel {
            route: "test".into(),
            tool_subset: vec!["read_file".into(), "read_file".into()],
        },
    ] {
        let tool = fake("read_file", true, vec![output("ok", 0)])?;
        let mut h = Harness::new(
            vec![calls(&[("a", "read_file", r#"{"path":"a"}"#)]), answer()],
            registry(std::slice::from_ref(&tool))?,
        )?;
        h.deps.decision = Arc::new(ScriptEngine {
            before: decision,
            duplicate: false,
        });
        Runner::new(h.deps)?
            .run(
                &[],
                Some(turn()),
                CancellationToken::new(),
                CancellationToken::new(),
            )
            .await?;
        assert_eq!(tool.calls().len(), 1);
        assert_eq!(h.provider.requests()[0].tools[0].name, "read_file");
    }
    Ok(())
}
#[tokio::test]
async fn core_u_032_duplicate_plan_falls_back_and_valid_subset_is_stable() -> TestResult {
    let a = fake("read_a", true, vec![output("a", 0)])?;
    let b = fake("read_b", true, vec![])?;
    let mut h = Harness::new(
        vec![calls(&[("a", "read_a", r#"{"path":"a"}"#)]), answer()],
        registry(&[a.clone(), b])?,
    )?;
    h.deps.decision = Arc::new(ScriptEngine {
        before: Decision::CallModel {
            route: "test".into(),
            tool_subset: vec!["read_a".into()],
        },
        duplicate: true,
    });
    Runner::new(h.deps)?
        .run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert_eq!(a.calls().len(), 1);
    for request in h.provider.requests() {
        assert_eq!(
            request
                .tools
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>(),
            ["read_a"]
        );
    }
    assert_closed(&h.sink.frames())?;
    Ok(())
}
