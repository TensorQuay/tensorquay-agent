//! `ext-check decision` (TK-U-030): the conformance suite an engine's author runs against their own `decision/v1`
//! server (ADR-0013 point 6). It asks what a runtime asks, through the runtime's own relay client, at every exposure
//! level, and checks each answer with the loop's own rules (CORE-U-063), so a pass means the loop would apply the
//! answer, and each failure names the fallback code the loop would record. A slow, down or wrong engine only costs a
//! task a `decision.fallback`, so the suite measures nothing more there than the answers themselves.
mod cases;

use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

use secrecy::SecretString;
use tqa_core::{
    Decision, DecisionEngine, EngineFailure, check_act, check_call_model, check_selection,
    valid_stop_reason,
};
use tqa_runtime::{RelayDecisionEngine, RelayEngineConfig};
use tqa_tools::ToolRegistry;
use tqa_types::{DecisionExposure, DecisionFallbackReason, DecisionPoint};
use url::Url;

pub use cases::{Case, cases, registry, routes};

use crate::report::{Check, Report, Status};

/// Every exposure level, each asked the whole canned set: an engine that depends on a field it won't always get fails
/// at the levels without it.
pub const EXPOSURES: [DecisionExposure; 3] = [
    DecisionExposure::None,
    DecisionExposure::Calls,
    DecisionExposure::Full,
];

/// The engine under test.
pub struct Engine {
    /// Its base URL; the suite posts to `<url>/v1/before_step` and `/v1/after_response`, as the gateway does.
    pub url: Url,
    /// The time it has to answer each consultation.
    pub timeout: Duration,
    /// The bearer credential to send, if any; never printed or reported.
    pub credential: Option<SecretString>,
}

/// What one consultation came to: the answer's summary, or the loop's fallback code.
type Outcome = Result<String, DecisionFallbackReason>;

/// Asks every canned consultation at every exposure level and records one check per consultation and level, then the
/// latency over the answers.
pub async fn check_engine(engine: &Engine, report: &mut Report) {
    let registry = registry();
    let Ok(cases) = cases() else {
        report.push(Check::new(
            "decision.cases",
            Status::Skip,
            "the canned consultations could not be built",
        ));
        return;
    };
    let mut latencies = Vec::new();
    let mut first = None;
    for exposure in EXPOSURES {
        let client = match client(engine, exposure, &registry) {
            Ok(client) => client,
            Err(message) => {
                report.push(Check::new("decision.client", Status::Skip, message));
                return;
            }
        };
        for case in &cases {
            let started = Instant::now();
            let outcome = consult(&client, case, &registry).await;
            let elapsed = started.elapsed();
            first.get_or_insert(elapsed);
            let code = format!("decision.{}.{}", level(exposure), case.name);
            match outcome {
                Ok(summary) => {
                    latencies.push(elapsed);
                    let message = format!("{summary} in {} ms", elapsed.as_millis());
                    report.push(Check::new(&code, Status::Pass, &message));
                }
                Err(reason) => {
                    let message = failure(reason, engine.timeout);
                    report.push(Check::new(&code, Status::Fail, &message));
                }
            }
        }
    }
    report.push(latency(&latencies, first));
    report.push(Check::new(
        "decision.unavailability",
        Status::Skip,
        "not measured: an engine that is slow, down or wrong costs a task only a decision.fallback for that step",
    ));
}

/// The runtime's relay client, pointed at the engine itself at `exposure`.
fn client(
    engine: &Engine,
    exposure: DecisionExposure,
    registry: &ToolRegistry,
) -> Result<RelayDecisionEngine, &'static str> {
    RelayDecisionEngine::new(RelayEngineConfig {
        name: "engine".into(),
        base: engine.url.clone(),
        token: engine
            .credential
            .clone()
            .unwrap_or_else(|| SecretString::from("")),
        timeout: engine.timeout,
        exposure,
        tools: registry
            .specs()
            .map(|spec| {
                (
                    spec.name.as_str().to_owned(),
                    spec.description.clone(),
                    spec.parameters.clone(),
                )
            })
            .collect(),
        routes: routes(),
        points: None,
    })
    .map_err(|_| "the HTTP client could not be built")
}

/// One consultation, checked as the loop checks it.
async fn consult(client: &RelayDecisionEngine, case: &Case, registry: &ToolRegistry) -> Outcome {
    if case.point == DecisionPoint::AfterResponse
        && let Some(response) = &case.response
    {
        let original: BTreeSet<String> = response
            .tool_calls
            .iter()
            .map(|call| call.id.clone())
            .collect();
        let planned = client
            .after_response(&case.state, response)
            .await
            .map_err(fallback)?;
        let selected = check_selection(&original, &planned)?;
        return Ok(format!(
            "select {} of {} calls",
            selected.len(),
            original.len()
        ));
    }
    let decision = client.before_step(&case.state).await.map_err(fallback)?;
    before_step(case, decision, registry)
}

/// A `before_step` decision through the loop's checks.
fn before_step(case: &Case, decision: Decision, registry: &ToolRegistry) -> Outcome {
    match decision {
        Decision::Act(call) => {
            check_act(&case.state, &call, |name, arguments| {
                registry.prepare(name, arguments).is_ok()
            })?;
            Ok(format!("act {}", call.name))
        }
        Decision::Compact => Ok("compact".into()),
        Decision::Stop(reason) if valid_stop_reason(&reason) => Ok("stop".into()),
        Decision::Stop(_) => Err(DecisionFallbackReason::Invalid),
        call_model @ Decision::CallModel { .. } => {
            let (tools, route) = check_call_model(&case.state, call_model)?;
            Ok(format!("call_model on {route} with {} tools", tools.len()))
        }
    }
}

/// The fallback code the loop records for an engine failure.
const fn fallback(failure: EngineFailure) -> DecisionFallbackReason {
    match failure {
        EngineFailure::Unreachable => DecisionFallbackReason::Unreachable,
        EngineFailure::Timeout => DecisionFallbackReason::Timeout,
        EngineFailure::Invalid => DecisionFallbackReason::Invalid,
        EngineFailure::BudgetSpent => DecisionFallbackReason::BudgetSpent,
    }
}

/// A failed consultation: the loop's code and what it means for the author.
fn failure(reason: DecisionFallbackReason, timeout: Duration) -> String {
    let (code, meaning) = match reason {
        DecisionFallbackReason::RouteNotAllowed => (
            "route_not_allowed",
            "call_model named a route outside the request's routes".to_owned(),
        ),
        DecisionFallbackReason::ToolNotAllowed => (
            "tool_not_allowed",
            "the answer named a tool outside the request's tools, or one tool twice".to_owned(),
        ),
        DecisionFallbackReason::Schema => (
            "schema",
            "act's arguments aren't a JSON object the tool's parameter schema accepts".to_owned(),
        ),
        DecisionFallbackReason::Timeout => (
            "timeout",
            format!("no answer within {} ms", timeout.as_millis()),
        ),
        DecisionFallbackReason::BudgetSpent => (
            "budget_spent",
            "the engine's budget is spent (a gateway's refusal, not an engine's answer)".to_owned(),
        ),
        DecisionFallbackReason::Unreachable => (
            "unreachable",
            "no 2xx answer: the engine refused or could not be reached".to_owned(),
        ),
        _ => (
            "invalid",
            "not one decision/v1 decision this point accepts, over 64 KiB, or a selection of ids outside the \
             response's calls or one id twice"
                .to_owned(),
        ),
    };
    format!("the loop would fall back ({code}): {meaning}")
}

const fn level(exposure: DecisionExposure) -> &'static str {
    match exposure {
        DecisionExposure::Calls => "calls",
        DecisionExposure::Full => "full",
        _ => "none",
    }
}

/// Latency over the answered consultations (nearest rank), and the first consultation's on its own, since an engine
/// must answer without a warm-up request.
fn latency(answered: &[Duration], first: Option<Duration>) -> Check {
    let mut sorted = answered.to_vec();
    sorted.sort_unstable();
    let rank = |percent: usize| {
        let index = (sorted.len() * percent).div_ceil(100).saturating_sub(1);
        sorted.get(index).map_or(0, Duration::as_millis)
    };
    if sorted.is_empty() {
        return Check::new(
            "decision.latency",
            Status::Skip,
            "no consultation was answered",
        );
    }
    let first = first.map_or(0, |first| first.as_millis());
    let message = format!(
        "p50 {} ms, p95 {} ms over {} answers; the first consultation took {first} ms",
        rank(50),
        rank(95),
        sorted.len()
    );
    Check::new("decision.latency", Status::Pass, &message)
}
