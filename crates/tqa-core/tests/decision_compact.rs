//! CORE-U-060 (the compact half): a named engine's `compact` runs compaction now, even below the threshold, and the
//! engine is asked again; a second `compact` in the step is `repeated_compact`, and one with nothing old enough to fold
//! is `compact_nothing` (ADR-0013 point 2). The `compact` decision is journaled before the compaction.
mod support;
use async_trait::async_trait;
use serde_json::json;
use std::{
    collections::VecDeque,
    error::Error,
    sync::{Arc, Mutex},
};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{
    Decision, DecisionEngine, EngineFailure, ModelDecides, ModelResponse, PlannedAction, RunState,
    Runner, UserTurn,
};
use tqa_llm::{FinishReason, StreamChunk};
use tqa_testkit::ProviderReply;
use tqa_tools::ToolRegistry;
use tqa_types::{
    CompactionSummary, DecisionChoice, DecisionFallbackReason, DecisionPoint, Event,
    FinishReason as WireFinish, ServerFrame, ToolStatus,
};

/// An engine with a name that gives its answers in order, then answers as the model would.
struct Sequence {
    answers: Mutex<VecDeque<Decision>>,
    asked: Mutex<u32>,
}
impl Sequence {
    fn new(answers: Vec<Decision>) -> Arc<Self> {
        Arc::new(Self {
            answers: Mutex::new(answers.into()),
            asked: Mutex::new(0),
        })
    }
    fn asked(&self) -> u32 {
        self.asked.lock().map_or(0, |asked| *asked)
    }
}
#[async_trait]
impl DecisionEngine for Sequence {
    fn name(&self) -> Option<&str> {
        Some("router")
    }
    async fn before_step(&self, state: &RunState) -> Result<Decision, EngineFailure> {
        if let Ok(mut asked) = self.asked.lock() {
            *asked += 1;
        }
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

/// One completed earlier run: a user message, a tool step with a result, and an answer step.
fn completed_run(index: usize, seq: usize) -> Result<Vec<Event>, Box<dyn Error>> {
    let run_id: tqa_types::RunId = format!("r-{seq}").parse()?;
    let call_id: tqa_types::ToolCallId = format!("c{index}").parse()?;
    Ok(vec![
        Event::MessageUser {
            lead_brief: None,
            team_wake: None,
            text: format!("request {index}"),
            attachments: vec![],
        },
        Event::RunStarted {
            run_id: run_id.clone(),
        },
        Event::StepStarted {
            step: 1,
            attempt: 1,
        },
        Event::ToolCall {
            call_id: call_id.clone(),
            name: "read_file".into(),
            arguments: json!({"path": format!("src/{index}.rs")}),
        },
        Event::ModelCompleted {
            step: 1,
            attempt: 1,
            finish_reason: WireFinish::ToolCalls,
        },
        Event::ToolResult {
            call_id,
            status: ToolStatus::Ok,
            content: format!("result {index}"),
            target: None,
            evidence: None,
        },
        Event::StepCompleted {
            step: 1,
            duration_ms: 0,
        },
        Event::StepStarted {
            step: 2,
            attempt: 1,
        },
        Event::TextDelta {
            text: format!("answer {index}"),
        },
        Event::ModelCompleted {
            step: 2,
            attempt: 1,
            finish_reason: WireFinish::Stop,
        },
        Event::StepCompleted {
            step: 2,
            duration_ms: 0,
        },
        Event::RunCompleted { run_id },
    ])
}
fn earlier_runs(runs: usize) -> Result<Vec<ServerFrame>, Box<dyn Error>> {
    let mut events = Vec::new();
    for index in 1..=runs {
        let seq = events.len() + 1;
        events.extend(completed_run(index, seq)?);
    }
    history(events)
}
fn summary_reply() -> Result<ProviderReply, Box<dyn Error>> {
    let summary = CompactionSummary {
        goal: "answer the requests".into(),
        ..CompactionSummary::default()
    };
    Ok(reply(vec![
        StreamChunk::TextDelta(serde_json::to_string(&summary)?),
        usage(),
        StreamChunk::Finish(FinishReason::Stop),
    ]))
}
async fn run(
    script: Vec<ProviderReply>,
    earlier: &[ServerFrame],
    engine: Arc<Sequence>,
) -> Result<(Vec<Event>, usize), Box<dyn Error>> {
    let mut h = Harness::new(script, ToolRegistry::new())?;
    h.sink_with(MemorySink::new(earlier));
    h.deps.decision = engine;
    let (sink, provider) = (h.sink.clone(), h.provider.clone());
    let turn = UserTurn {
        lead_brief: None,
        team_wake: None,
        text: "the current request".into(),
        attachments: vec![],
    };
    Runner::new(h.deps)?
        .run(
            earlier,
            Some(turn),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    Ok((sink.events(), provider.requests().len()))
}
fn engine_events(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Decision {
                choice: DecisionChoice::Compact,
                ..
            } => Some("compact".into()),
            Event::Decision {
                choice: DecisionChoice::CallModel { .. },
                ..
            } => Some("call_model".into()),
            Event::Compaction { .. } => Some("compaction".into()),
            Event::DecisionFallback { reason, point, .. } => {
                assert_eq!(*point, DecisionPoint::BeforeStep);
                Some(format!("fallback {reason:?}"))
            }
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn core_u_060_a_compact_runs_compaction_now_and_asks_again() -> TestResult {
    let engine = Sequence::new(vec![
        Decision::Compact,
        Decision::CallModel {
            route: "test".into(),
            tool_subset: vec![],
        },
    ]);
    let earlier = earlier_runs(20)?;
    let (events, requests) =
        run(vec![summary_reply()?, answer()], &earlier, engine.clone()).await?;
    assert_eq!(
        engine_events(&events),
        ["compact", "compaction", "call_model"],
        "journaled before the compaction, then asked again"
    );
    assert_eq!(engine.asked(), 2);
    assert_eq!(requests, 2, "the summary and the step");
    assert!(matches!(events.last(), Some(Event::RunCompleted { .. })));
    Ok(())
}

#[tokio::test]
async fn core_u_060_a_second_compact_in_a_step_falls_back() -> TestResult {
    let engine = Sequence::new(vec![Decision::Compact, Decision::Compact]);
    let earlier = earlier_runs(20)?;
    let (events, _) = run(vec![summary_reply()?, answer()], &earlier, engine).await?;
    assert_eq!(
        engine_events(&events),
        [
            "compact".to_owned(),
            "compaction".to_owned(),
            format!("fallback {:?}", DecisionFallbackReason::RepeatedCompact),
        ]
    );
    assert!(matches!(events.last(), Some(Event::RunCompleted { .. })));
    Ok(())
}

#[tokio::test]
async fn core_u_060_a_compact_with_nothing_to_fold_falls_back() -> TestResult {
    let engine = Sequence::new(vec![Decision::Compact]);
    let earlier = earlier_runs(1)?;
    let (events, requests) = run(vec![answer()], &earlier, engine).await?;
    assert_eq!(
        engine_events(&events),
        [format!(
            "fallback {:?}",
            DecisionFallbackReason::CompactNothing
        )]
    );
    assert_eq!(requests, 1, "no summary call");
    assert!(matches!(events.last(), Some(Event::RunCompleted { .. })));
    Ok(())
}
