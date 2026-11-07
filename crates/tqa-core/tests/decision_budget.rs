//! CORE-U-064 (ADR-0013, the adapter amendment): an engine whose budget is spent. The gateway's refusal reaches the
//! loop as its own failure and is journaled once as `budget_spent`; the engine is then not asked again in that run, at
//! either point, and no `circuit_open` follows. The run completes on the model's own decisions, and a new run asks
//! again (the breaker reads only this run's events), so a raised budget takes effect then.
mod support;
use async_trait::async_trait;
use std::{
    error::Error,
    sync::{Arc, Mutex},
};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{
    Decision, DecisionEngine, EngineFailure, ModelResponse, PlannedAction, RunState, Runner,
};
use tqa_types::{DecisionFallbackReason, DecisionPoint, Event};

/// An engine whose budget is spent at `point`; at the other point it answers as the model would. Counts every ask.
struct Spent {
    point: DecisionPoint,
    asked: Mutex<u32>,
}
impl Spent {
    fn new(point: DecisionPoint) -> Arc<Self> {
        Arc::new(Self {
            point,
            asked: Mutex::new(0),
        })
    }
    fn asked(&self) -> u32 {
        self.asked.lock().map_or(0, |asked| *asked)
    }
    fn count(&self) {
        if let Ok(mut asked) = self.asked.lock() {
            *asked += 1;
        }
    }
}
#[async_trait]
impl DecisionEngine for Spent {
    fn name(&self) -> Option<&str> {
        Some("router")
    }
    async fn before_step(&self, state: &RunState) -> Result<Decision, EngineFailure> {
        self.count();
        if self.point == DecisionPoint::BeforeStep {
            return Err(EngineFailure::BudgetSpent);
        }
        Ok(Decision::CallModel {
            route: state.route.clone(),
            tool_subset: state.tools.clone(),
        })
    }
    async fn after_response(
        &self,
        _: &RunState,
        response: &ModelResponse,
    ) -> Result<Vec<PlannedAction>, EngineFailure> {
        self.count();
        if self.point == DecisionPoint::AfterResponse {
            return Err(EngineFailure::BudgetSpent);
        }
        Ok(response
            .tool_calls
            .iter()
            .map(|call| PlannedAction {
                call_id: call.id.clone(),
            })
            .collect())
    }
}

async fn run(h: Harness) -> Result<Vec<Event>, Box<dyn Error>> {
    let sink = h.sink.clone();
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
    Ok(sink.events())
}
fn fallbacks(events: &[Event]) -> Vec<(DecisionPoint, DecisionFallbackReason)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::DecisionFallback { point, reason, .. } => Some((*point, *reason)),
            _ => None,
        })
        .collect()
}

// TEST: CORE-U-064
#[tokio::test]
async fn core_u_064_a_spent_budget_is_one_fallback_and_the_engine_is_not_asked_again_that_run()
-> TestResult {
    for (point, asked) in [
        // Before the first step: asked once.
        (DecisionPoint::BeforeStep, 1),
        // After the first response: asked before that step and after its response, then never again.
        (DecisionPoint::AfterResponse, 2),
    ] {
        let tool = fake("read_file", true, (0..4).map(|_| output("ok", 0)).collect())?;
        let script: Vec<_> = (0..4)
            .map(|index| calls(&[(&*format!("c{index}"), "read_file", r#"{"path":"a"}"#)]))
            .chain(std::iter::once(answer()))
            .collect();
        let mut h = Harness::new(script, registry(std::slice::from_ref(&tool))?)?;
        let engine = Spent::new(point);
        h.deps.decision = engine.clone();
        let events = run(h).await?;
        assert_eq!(
            fallbacks(&events),
            [(point, DecisionFallbackReason::BudgetSpent)],
            "one event, no circuit_open"
        );
        assert_eq!(
            engine.asked(),
            asked,
            "{point:?}: not asked again in the run"
        );
        assert!(matches!(events.last(), Some(Event::RunCompleted { .. })));
        assert_eq!(
            tool.calls().len(),
            4,
            "the run completes on the model's decisions"
        );
    }
    Ok(())
}
