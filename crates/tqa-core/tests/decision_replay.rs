//! CORE-U-058: on resume, a step whose decision or fallback is journaled uses that record and never asks the engine
//! again, so a resumed run takes exactly the decision it took (ADR-0013 point 4). That includes the selection of a
//! response's calls: a call the engine left out still doesn't run after a resume.
mod support;
use async_trait::async_trait;
use std::{
    error::Error,
    sync::{Arc, Mutex},
};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{
    Decision, DecisionEngine, EngineFailure, ModelDecides, ModelResponse, PlannedAction,
    RouteProvider, RunOutcome, RunState, Runner,
};
use tqa_llm::Registry;
use tqa_testkit::FakeProvider;
use tqa_types::{
    DecisionChoice, DecisionFallbackReason, DecisionPoint, Event, RouteSnapshot, ToolStatus,
};

/// A named engine that answers as the model would and records the steps it is asked about.
#[derive(Default)]
struct Watching {
    steps: Mutex<Vec<u32>>,
    /// The engine answers only after responses now, as a session.config naming fewer points would say.
    after_only: bool,
}
#[async_trait]
impl DecisionEngine for Watching {
    fn name(&self) -> Option<&str> {
        Some("router")
    }
    fn answers(&self, point: DecisionPoint) -> bool {
        !(self.after_only && point == DecisionPoint::BeforeStep)
    }
    async fn before_step(&self, state: &RunState) -> Result<Decision, EngineFailure> {
        if let Ok(mut steps) = self.steps.lock() {
            steps.push(state.step);
        }
        ModelDecides.before_step(state).await
    }
    async fn after_response(
        &self,
        state: &RunState,
        response: &ModelResponse,
    ) -> Result<Vec<PlannedAction>, EngineFailure> {
        ModelDecides.after_response(state, response).await
    }
}
impl Watching {
    fn steps(&self) -> Vec<u32> {
        self.steps
            .lock()
            .map(|steps| steps.clone())
            .unwrap_or_default()
    }
}

fn decision(point: DecisionPoint, choice: DecisionChoice) -> Event {
    Event::Decision {
        point,
        engine: "router".into(),
        choice,
        latency_ms: 0,
    }
}
fn backup_route() -> RouteSnapshot {
    RouteSnapshot {
        name: "backup".into(),
        relay_path: "/relay/r/backup".into(),
        model: "backup-model".into(),
        ..route()
    }
}
fn with_backup(h: &mut Harness) -> Result<Arc<FakeProvider>, Box<dyn Error>> {
    let routes = Registry::from_toml(
        "[[routes]]\nname='backup'\nprotocol='openai_chat'\nbase_url='http://localhost'\nmodel='backup-model'\nauth={kind='none'}\n",
    )?;
    let provider = Arc::new(FakeProvider::new(
        routes.get("backup").ok_or("missing route")?.clone(),
        vec![answer()],
        Arc::new(h.sleeper.clone()),
    ));
    h.deps.fallback_routes = vec![RouteProvider {
        route: backup_route(),
        provider: provider.clone(),
    }];
    Ok(provider)
}
async fn resume(
    h: Harness,
    history: &[tqa_types::ServerFrame],
) -> Result<RunOutcome, Box<dyn Error>> {
    Ok(Runner::new(h.deps)?
        .run(
            history,
            None,
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?)
}

#[tokio::test]
async fn core_u_058_a_journaled_call_model_is_reused_on_its_route() -> TestResult {
    let mut events = initial()?;
    events.push(decision(
        DecisionPoint::BeforeStep,
        DecisionChoice::CallModel {
            route: "backup".into(),
            tools: vec![],
        },
    ));
    let history = history(events)?;
    let mut h = Harness::new(vec![], registry(&[])?)?;
    h.sink_with(MemorySink::new(&history));
    let backup = with_backup(&mut h)?;
    let own = h.provider.clone();
    let engine = Arc::new(Watching::default());
    h.deps.decision = engine.clone();
    let sink = h.sink.clone();
    assert_eq!(resume(h, &history).await?, RunOutcome::Completed);
    assert!(engine.steps().is_empty(), "step 1 isn't asked again");
    assert_eq!(
        backup.requests().len(),
        1,
        "the step runs on the journaled route"
    );
    assert_eq!(own.requests().len(), 0);
    let new = &sink.events()[history.len()..];
    assert!(!new.iter().any(|event| matches!(
        event,
        Event::Decision { .. } | Event::DecisionFallback { .. }
    )));
    Ok(())
}

// TEST: CORE-U-058
#[tokio::test]
async fn core_u_058_a_journaled_call_model_is_replayed_even_if_the_engine_no_longer_answers_before_steps()
-> TestResult {
    let mut events = initial()?;
    events.push(decision(
        DecisionPoint::BeforeStep,
        DecisionChoice::CallModel {
            route: "backup".into(),
            tools: vec![],
        },
    ));
    let history = history(events)?;
    // A registered tool the journaled decision left out, so the replayed tool subset shows in the model request.
    let mut h = Harness::new(vec![], registry(&[fake("read_file", true, vec![])?])?)?;
    h.sink_with(MemorySink::new(&history));
    let backup = with_backup(&mut h)?;
    let own = h.provider.clone();
    let engine = Arc::new(Watching {
        after_only: true,
        ..Watching::default()
    });
    h.deps.decision = engine.clone();
    let sink = h.sink.clone();
    assert_eq!(resume(h, &history).await?, RunOutcome::Completed);
    assert!(engine.steps().is_empty(), "never asked before a step");
    assert_eq!(
        (backup.requests().len(), own.requests().len()),
        (1, 0),
        "the journaled decision holds: the step runs on the backup route"
    );
    assert!(
        backup.requests()[0].tools.is_empty(),
        "with the journaled tool subset (none), not every tool"
    );
    let new = &sink.events()[history.len()..];
    assert!(!new.iter().any(|event| matches!(
        event,
        Event::Decision { .. } | Event::DecisionFallback { .. }
    )));
    Ok(())
}

#[tokio::test]
async fn core_u_058_a_journaled_fallback_is_reused() -> TestResult {
    let mut events = initial()?;
    events.push(Event::DecisionFallback {
        point: DecisionPoint::BeforeStep,
        engine: "router".into(),
        reason: DecisionFallbackReason::Timeout,
    });
    let history = history(events)?;
    let tool = fake("read_file", true, vec![])?;
    let mut h = Harness::new(vec![answer()], registry(std::slice::from_ref(&tool))?)?;
    h.sink_with(MemorySink::new(&history));
    let engine = Arc::new(Watching::default());
    h.deps.decision = engine.clone();
    let provider = h.provider.clone();
    assert_eq!(resume(h, &history).await?, RunOutcome::Completed);
    assert!(engine.steps().is_empty());
    assert_eq!(
        provider.requests()[0].tools.len(),
        1,
        "the model's own decisions: every tool"
    );
    Ok(())
}

#[tokio::test]
async fn core_u_058_a_journaled_selection_holds_after_a_resume_in_the_tool_phase() -> TestResult {
    let mut events = initial()?;
    events.extend([
        decision(
            DecisionPoint::BeforeStep,
            DecisionChoice::CallModel {
                route: "test".into(),
                tools: vec!["read_file".into()],
            },
        ),
        call("a")?,
        call("b")?,
        model(),
        decision(
            DecisionPoint::AfterResponse,
            DecisionChoice::Select {
                call_ids: vec!["b".parse()?],
            },
        ),
    ]);
    let history = history(events)?;
    let tool = fake("read_file", true, vec![output("b's result", 0)])?;
    let mut h = Harness::new(vec![answer()], registry(std::slice::from_ref(&tool))?)?;
    h.sink_with(MemorySink::new(&history));
    let engine = Arc::new(Watching::default());
    h.deps.decision = engine.clone();
    let sink = h.sink.clone();
    assert_eq!(resume(h, &history).await?, RunOutcome::Completed);
    assert_eq!(
        tool.calls(),
        vec![serde_json::json!({"path":"b"})],
        "a stays left out"
    );
    let results = tool_results(&sink.events());
    assert_eq!(
        results[0],
        (
            "a".into(),
            ToolStatus::Error,
            "not run: skipped by the decision engine".into()
        )
    );
    assert_eq!(engine.steps(), [2], "only the next step is asked");
    Ok(())
}

/// A crash after an `act` was journaled but before its call: the resumed step issues the call without asking again.
#[tokio::test]
async fn core_u_058_a_journaled_act_is_issued_without_asking_again() -> TestResult {
    let events = vec![
        Event::MessageUser {
            lead_brief: None,
            team_wake: None,
            text: "question".into(),
            attachments: vec![],
        },
        Event::RunStarted {
            run_id: "r-1".parse()?,
        },
        decision(
            DecisionPoint::BeforeStep,
            DecisionChoice::Act {
                tool: "read_file".into(),
                arguments: serde_json::json!({"path":"a"}),
            },
        ),
    ];
    let mut history = history(events)?;
    // The loop journals a decision with the step it decides.
    if let Some(tqa_types::ServerFrame::Event { step, .. }) = history.get_mut(2) {
        *step = Some(1);
    }
    let tool = fake("read_file", true, vec![output("a's result", 0)])?;
    let mut h = Harness::new(vec![answer()], registry(std::slice::from_ref(&tool))?)?;
    h.sink_with(MemorySink::new(&history));
    let engine = Arc::new(Watching::default());
    h.deps.decision = engine.clone();
    assert_eq!(resume(h, &history).await?, RunOutcome::Completed);
    assert_eq!(tool.calls(), vec![serde_json::json!({"path":"a"})]);
    assert_eq!(engine.steps(), [2], "step 1 isn't asked again");
    Ok(())
}
