//! CORE-U-059, CORE-E-035, CORE-E-039: a named engine's decisions applied and checked (ADR-0013 points 2 to 4).
//! A `call_model` on the conversation's own route or a declared fallback runs there, with the step's request built for
//! that route; any other route or tool falls back with its code; each applied decision and each fallback is journaled;
//! after three fallbacks in a row the engine isn't asked again in the run, with one `circuit_open`.
mod support;
use async_trait::async_trait;
use std::{
    error::Error,
    sync::{Arc, Mutex},
};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{
    Decision, DecisionEngine, EngineFailure, ModelResponse, PlannedAction, RouteProvider, RunState,
    Runner,
};
use tqa_llm::Registry;
use tqa_testkit::{FakeProvider, ProviderReply};
use tqa_types::{DecisionChoice, DecisionFallbackReason, DecisionPoint, Event, RouteSnapshot};

/// An engine with a name that answers every step the same way and counts how often it is asked.
struct Scripted {
    before: Result<Decision, EngineFailure>,
    /// Drop the response's first call, or invent a call id.
    plan: Plan,
    asked: Mutex<u32>,
}
#[derive(Clone, Copy)]
enum Plan {
    All,
    SkipFirst,
    Invented,
}
impl Scripted {
    fn new(before: Result<Decision, EngineFailure>, plan: Plan) -> Arc<Self> {
        Arc::new(Self {
            before,
            plan,
            asked: Mutex::new(0),
        })
    }
    fn asked(&self) -> u32 {
        self.asked.lock().map_or(0, |asked| *asked)
    }
}
#[async_trait]
impl DecisionEngine for Scripted {
    fn name(&self) -> Option<&str> {
        Some("router")
    }
    async fn before_step(&self, _: &RunState) -> Result<Decision, EngineFailure> {
        if let Ok(mut asked) = self.asked.lock() {
            *asked += 1;
        }
        self.before.clone()
    }
    async fn after_response(
        &self,
        _: &RunState,
        response: &ModelResponse,
    ) -> Result<Vec<PlannedAction>, EngineFailure> {
        let ids = response.tool_calls.iter().map(|call| call.id.clone());
        Ok(match self.plan {
            Plan::All => ids.map(|call_id| PlannedAction { call_id }).collect(),
            Plan::SkipFirst => ids
                .skip(1)
                .map(|call_id| PlannedAction { call_id })
                .collect(),
            Plan::Invented => vec![PlannedAction {
                call_id: "invented".into(),
            }],
        })
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
fn with_backup(
    h: &mut Harness,
    script: Vec<ProviderReply>,
) -> Result<Arc<FakeProvider>, Box<dyn Error>> {
    let routes = Registry::from_toml(
        "[[routes]]\nname='backup'\nprotocol='openai_chat'\nbase_url='http://localhost'\nmodel='backup-model'\nauth={kind='none'}\n",
    )?;
    let provider = Arc::new(FakeProvider::new(
        routes.get("backup").ok_or("missing route")?.clone(),
        script,
        Arc::new(h.sleeper.clone()),
    ));
    h.deps.fallback_routes = vec![RouteProvider {
        route: backup_route(),
        provider: provider.clone(),
    }];
    Ok(provider)
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
fn decisions(events: &[Event]) -> Vec<(DecisionPoint, DecisionChoice)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Decision {
                point,
                engine,
                choice,
                ..
            } if engine == "router" => Some((*point, choice.clone())),
            _ => None,
        })
        .collect()
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

#[tokio::test]
async fn core_u_059_a_call_model_on_a_declared_fallback_runs_there() -> TestResult {
    let tool = fake("read_file", true, vec![output("ok", 0)])?;
    let mut h = Harness::new(vec![], registry(std::slice::from_ref(&tool))?)?;
    let backup = with_backup(
        &mut h,
        vec![calls(&[("a", "read_file", r#"{"path":"a"}"#)]), answer()],
    )?;
    let own = h.provider.clone();
    h.deps.decision = Scripted::new(
        Ok(Decision::CallModel {
            route: "backup".into(),
            tool_subset: vec!["read_file".into()],
        }),
        Plan::All,
    );
    let events = run(h).await?;
    assert_eq!(own.requests().len(), 0, "the own route isn't called");
    assert_eq!(
        backup.requests().len(),
        2,
        "both steps run on the chosen route"
    );
    let call_model = DecisionChoice::CallModel {
        route: "backup".into(),
        tools: vec!["read_file".into()],
    };
    assert_eq!(
        decisions(&events),
        [
            (DecisionPoint::BeforeStep, call_model.clone()),
            (
                DecisionPoint::AfterResponse,
                DecisionChoice::Select {
                    call_ids: vec!["a".parse()?],
                }
            ),
            (DecisionPoint::BeforeStep, call_model),
        ]
    );
    assert!(fallbacks(&events).is_empty());
    assert_eq!(tool.calls().len(), 1);
    Ok(())
}

#[tokio::test]
async fn core_u_059_another_route_or_tool_falls_back_with_its_code() -> TestResult {
    for (decision, code) in [
        (
            Decision::CallModel {
                route: "elsewhere".into(),
                tool_subset: vec!["read_file".into()],
            },
            DecisionFallbackReason::RouteNotAllowed,
        ),
        (
            Decision::CallModel {
                route: "test".into(),
                tool_subset: vec!["write_file".into()],
            },
            DecisionFallbackReason::ToolNotAllowed,
        ),
        (
            Decision::CallModel {
                route: "test".into(),
                tool_subset: vec!["read_file".into(), "read_file".into()],
            },
            DecisionFallbackReason::ToolNotAllowed,
        ),
    ] {
        let tool = fake("read_file", true, vec![output("ok", 0)])?;
        let mut h = Harness::new(vec![answer()], registry(std::slice::from_ref(&tool))?)?;
        let own = h.provider.clone();
        h.deps.decision = Scripted::new(Ok(decision), Plan::All);
        let events = run(h).await?;
        assert_eq!(fallbacks(&events), [(DecisionPoint::BeforeStep, code)]);
        assert!(decisions(&events).is_empty());
        assert_eq!(own.requests().len(), 1, "the step ran on its own route");
        assert_eq!(own.requests()[0].tools.len(), 1, "with every tool");
    }
    Ok(())
}

#[tokio::test]
async fn core_e_035_an_engine_failure_is_journaled_with_its_code() -> TestResult {
    for (failure, code) in [
        (
            EngineFailure::Unreachable,
            DecisionFallbackReason::Unreachable,
        ),
        (EngineFailure::Timeout, DecisionFallbackReason::Timeout),
        (EngineFailure::Invalid, DecisionFallbackReason::Invalid),
    ] {
        let mut h = Harness::new(vec![answer()], registry(&[])?)?;
        h.deps.decision = Scripted::new(Err(failure), Plan::All);
        let events = run(h).await?;
        assert_eq!(fallbacks(&events), [(DecisionPoint::BeforeStep, code)]);
        assert!(matches!(events.last(), Some(Event::RunCompleted { .. })));
    }
    Ok(())
}

#[tokio::test]
async fn core_e_035_a_plan_with_an_invented_call_falls_back_and_every_call_runs() -> TestResult {
    let tool = fake("read_file", true, vec![output("a", 0), output("b", 0)])?;
    let mut h = Harness::new(
        vec![
            calls(&[
                ("a", "read_file", r#"{"path":"a"}"#),
                ("b", "read_file", r#"{"path":"b"}"#),
            ]),
            answer(),
        ],
        registry(std::slice::from_ref(&tool))?,
    )?;
    h.deps.decision = Scripted::new(
        Ok(Decision::CallModel {
            route: "test".into(),
            tool_subset: vec!["read_file".into()],
        }),
        Plan::Invented,
    );
    let events = run(h).await?;
    assert_eq!(
        fallbacks(&events),
        [(
            DecisionPoint::AfterResponse,
            DecisionFallbackReason::Invalid
        )]
    );
    assert_eq!(tool.calls().len(), 2);
    Ok(())
}

#[tokio::test]
async fn core_u_059_a_selection_is_journaled_and_the_omitted_call_gets_an_error() -> TestResult {
    let tool = fake("read_file", true, vec![output("b", 0)])?;
    let mut h = Harness::new(
        vec![
            calls(&[
                ("a", "read_file", r#"{"path":"a"}"#),
                ("b", "read_file", r#"{"path":"b"}"#),
            ]),
            answer(),
        ],
        registry(std::slice::from_ref(&tool))?,
    )?;
    h.deps.decision = Scripted::new(
        Ok(Decision::CallModel {
            route: "test".into(),
            tool_subset: vec!["read_file".into()],
        }),
        Plan::SkipFirst,
    );
    let events = run(h).await?;
    assert!(decisions(&events).contains(&(
        DecisionPoint::AfterResponse,
        DecisionChoice::Select {
            call_ids: vec!["b".parse()?],
        }
    )));
    assert_eq!(tool.calls().len(), 1);
    Ok(())
}

#[tokio::test]
async fn core_e_039_three_fallbacks_in_a_row_open_the_circuit_for_the_run() -> TestResult {
    let tool = fake("read_file", true, (0..5).map(|_| output("ok", 0)).collect())?;
    let script: Vec<_> = (0..5)
        .map(|index| calls(&[(&*format!("c{index}"), "read_file", r#"{"path":"a"}"#)]))
        .chain(std::iter::once(answer()))
        .collect();
    let mut h = Harness::new(script, registry(std::slice::from_ref(&tool))?)?;
    let engine = Scripted::new(Err(EngineFailure::Timeout), Plan::All);
    h.deps.decision = engine.clone();
    let events = run(h).await?;
    assert_eq!(engine.asked(), 3, "not asked after the circuit opens");
    assert_eq!(
        fallbacks(&events),
        [
            (DecisionPoint::BeforeStep, DecisionFallbackReason::Timeout),
            (DecisionPoint::BeforeStep, DecisionFallbackReason::Timeout),
            (DecisionPoint::BeforeStep, DecisionFallbackReason::Timeout),
            (
                DecisionPoint::BeforeStep,
                DecisionFallbackReason::CircuitOpen
            ),
        ]
    );
    assert!(matches!(events.last(), Some(Event::RunCompleted { .. })));
    assert_eq!(
        tool.calls().len(),
        5,
        "the run completes on the model's decisions"
    );
    Ok(())
}

/// CORE-U-060 (the stop half): a valid `stop` is journaled, then ends the run as failed with `decision_stop` and the
/// engine's reason, before any model call; an engine can't declare a run completed (ADR-0013 point 2).
#[tokio::test]
async fn core_u_060_a_stop_ends_the_run_as_failed_with_its_reason() -> TestResult {
    let mut h = Harness::new(vec![answer()], registry(&[])?)?;
    let provider = h.provider.clone();
    h.deps.decision = Scripted::new(
        Ok(Decision::Stop("the task is out of scope".into())),
        Plan::All,
    );
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
    assert_eq!(
        outcome,
        tqa_core::RunOutcome::Failed {
            code: tqa_types::ErrorCode::DecisionStop,
            reason: "the task is out of scope".into(),
        }
    );
    assert_eq!(provider.requests().len(), 0, "no model call");
    let events = sink.events();
    assert_eq!(
        decisions(&events),
        [(
            DecisionPoint::BeforeStep,
            DecisionChoice::Stop {
                reason: "the task is out of scope".into(),
            }
        )]
    );
    let stop = events
        .iter()
        .position(|event| matches!(event, Event::Decision { .. }))
        .ok_or("no decision")?;
    let failed = events
        .iter()
        .position(|event| {
            matches!(
                event,
                Event::RunFailed {
                    code: tqa_types::ErrorCode::DecisionStop,
                    ..
                }
            )
        })
        .ok_or("no run.failed")?;
    assert!(stop < failed, "journaled before it takes effect");
    Ok(())
}

#[tokio::test]
async fn core_u_060_a_stop_reason_out_of_bounds_falls_back() -> TestResult {
    for reason in [String::new(), "r".repeat(201), "two\nlines".to_owned()] {
        let mut h = Harness::new(vec![answer()], registry(&[])?)?;
        h.deps.decision = Scripted::new(Ok(Decision::Stop(reason.clone())), Plan::All);
        let events = run(h).await?;
        assert_eq!(
            fallbacks(&events),
            [(DecisionPoint::BeforeStep, DecisionFallbackReason::Invalid)],
            "{reason:?}"
        );
        assert!(matches!(events.last(), Some(Event::RunCompleted { .. })));
    }
    Ok(())
}
