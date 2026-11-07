//! CORE-S-034: decision hooks select calls but can't widen what the task may do; an unnamed engine can't act.
mod support;
use async_trait::async_trait;
use std::sync::Arc;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{
    Decision, DecisionEngine, EngineFailure, ModelResponse, PlannedAction, RunOutcome, RunState,
    Runner,
};
struct SelectingEngine {
    act: bool,
}
#[async_trait]
impl DecisionEngine for SelectingEngine {
    async fn before_step(&self, state: &RunState) -> Result<Decision, EngineFailure> {
        Ok({
            if self.act {
                Decision::Act(tqa_llm::ToolCall {
                    id: "invented".into(),
                    name: "execute".into(),
                    arguments: serde_json::json!({"path":"invented"}).to_string(),
                })
            } else {
                Decision::CallModel {
                    route: state.route.clone(),
                    tool_subset: state.tools.clone(),
                }
            }
        })
    }
    async fn after_response(
        &self,
        _: &RunState,
        response: &ModelResponse,
    ) -> Result<Vec<PlannedAction>, EngineFailure> {
        Ok({
            response
                .tool_calls
                .iter()
                .map(|call| PlannedAction {
                    call_id: call.id.clone(),
                })
                .collect()
        })
    }
}
/// Only a named engine can act, since an action must be journaled (ADR-0013); an unnamed engine's `act` falls back.
#[tokio::test]
async fn core_s_034_reserved_act_never_executes_a_tool() -> TestResult {
    let tool = fake("execute", false, vec![output("must not execute", 0)])?;
    let mut h = Harness::new(vec![answer()], registry(std::slice::from_ref(&tool))?)?;
    h.deps.decision = Arc::new(SelectingEngine { act: true });
    assert_eq!(
        Runner::new(h.deps)?
            .run(
                &[],
                Some(turn()),
                CancellationToken::new(),
                CancellationToken::new()
            )
            .await?,
        RunOutcome::Completed
    );
    assert!(tool.calls().is_empty());
    assert_eq!(h.provider.requests().len(), 1);
    Ok(())
}
