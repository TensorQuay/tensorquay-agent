//! TK-U-034 (E2E-DECISION-008): an attempt's arm and its decision tally from the event log. Each applied decision is
//! counted by kind and each fallback by its reason; the engine's calls are the decisions and the fallbacks that
//! followed a call, never `circuit_open` or `budget_spent` (no call was made); its latency is the decisions' sum.
use super::*;
use serde_json::json;
use tqa_types::{DecisionChoice, DecisionFallbackReason, DecisionPoint};

fn frame(seq: u64, event: Event) -> ServerFrame {
    ServerFrame::Event {
        seq: seq.into(),
        run_id: None,
        step: None,
        ts: "2026-09-29T00:00:00Z".into(),
        event,
    }
}
fn decision(choice: DecisionChoice, latency_ms: u64) -> Event {
    Event::Decision {
        point: DecisionPoint::BeforeStep,
        engine: "router".into(),
        choice,
        latency_ms,
    }
}
fn fallback(reason: DecisionFallbackReason) -> Event {
    Event::DecisionFallback {
        point: DecisionPoint::BeforeStep,
        engine: "router".into(),
        reason,
    }
}

// TEST: TK-U-034
#[test]
fn tk_u_034_decisions_by_kind_fallbacks_by_reason_calls_and_latency()
-> Result<(), Box<dyn std::error::Error>> {
    let events = [
        decision(
            DecisionChoice::CallModel {
                route: "cheap".into(),
                tools: vec![],
            },
            40,
        ),
        decision(
            DecisionChoice::CallModel {
                route: "local".into(),
                tools: vec![],
            },
            60,
        ),
        decision(DecisionChoice::Compact, 10),
        decision(
            DecisionChoice::Select {
                call_ids: vec!["c1".parse()?],
            },
            0,
        ),
        fallback(DecisionFallbackReason::Timeout),
        fallback(DecisionFallbackReason::Timeout),
        fallback(DecisionFallbackReason::Invalid),
        fallback(DecisionFallbackReason::CircuitOpen),
        fallback(DecisionFallbackReason::BudgetSpent),
    ];
    let frames: Vec<_> = events
        .into_iter()
        .enumerate()
        .map(|(seq, event)| frame(seq as u64 + 1, event))
        .collect();
    let tally = DecisionTally::from_events(&frames);
    assert_eq!(
        serde_json::to_value(&tally)?,
        json!({
            "by_kind": {"call_model": 2, "compact": 1, "select": 1},
            "fallbacks": {"timeout": 2, "invalid": 1, "circuit_open": 1, "budget_spent": 1},
            "engine_calls": 7,
            "engine_latency_ms": 110
        }),
        "four decisions and three fallbacks that followed a call"
    );
    assert_eq!(DecisionTally::from_events(&[]), DecisionTally::default());
    assert_eq!(serde_json::to_value(Arm::Model)?, json!("model"));
    assert_eq!(serde_json::to_value(Arm::Engine)?, json!("engine"));
    Ok(())
}
