//! Read-only views rebuilt from emitted events; execution caches never determine recovery.
use crate::EventContext;
use serde_json::Value;
use std::collections::BTreeMap;
use tqa_types::{Event, RunId, Seq, ServerFrame, ToolCallId, ToolStatus};

#[derive(Clone)]
pub(super) struct Call {
    pub id: ToolCallId,
    pub name: String,
    pub arguments: Value,
    pub context: EventContext,
    pub started: bool,
    pub ended: bool,
    pub result: Option<ToolStatus>,
}
pub(super) struct Attempt {
    pub context: EventContext,
    pub number: u32,
    pub complete: bool,
    pub calls: Vec<Call>,
}
pub(super) fn sequence(frame: &ServerFrame) -> Option<Seq> {
    match frame {
        ServerFrame::Event { seq, .. }
        | ServerFrame::PermissionRequest { seq, .. }
        | ServerFrame::Error { seq: Some(seq), .. } => Some(*seq),
        _ => None,
    }
}
/// Whether the history ends waiting for an approval that ADR-0035 removed: its last frame, sandbox states aside, is the
/// retired `run.parked` marker or an unanswered `permission.request`.
pub(super) fn ends_parked(frames: &[ServerFrame]) -> bool {
    frames
        .iter()
        .rev()
        .find(|frame| {
            !matches!(
                frame,
                ServerFrame::Event {
                    event: Event::SandboxState { .. },
                    ..
                }
            ) && sequence(frame).is_some()
        })
        .is_some_and(|frame| {
            matches!(
                frame,
                ServerFrame::PermissionRequest { .. }
                    | ServerFrame::Event {
                        event: Event::RunParked {},
                        ..
                    }
            )
        })
}
pub(super) fn attempts(frames: &[ServerFrame]) -> Vec<Attempt> {
    let mut attempts: Vec<Attempt> = Vec::new();
    // Steps where a decision engine acted: an attempt there is complete once its one call is journaled (ADR-0013).
    let mut acts: Vec<EventContext> = Vec::new();
    for frame in frames {
        let ServerFrame::Event {
            event,
            run_id,
            step,
            ..
        } = frame
        else {
            continue;
        };
        let context = EventContext {
            run_id: run_id.clone(),
            step: *step,
        };
        let index = attempts
            .iter()
            .position(|attempt| attempt.context == context);
        if matches!(
            event,
            Event::Decision {
                point: tqa_types::DecisionPoint::BeforeStep,
                choice: tqa_types::DecisionChoice::Act { .. },
                ..
            }
        ) {
            acts.push(context.clone());
        }
        if let Event::StepStarted { attempt, .. } = event {
            let next = Attempt {
                context,
                number: *attempt,
                complete: false,
                calls: vec![],
            };
            if let Some(index) = index {
                attempts[index] = next;
            } else {
                attempts.push(next);
            }
        } else if let Some(index) = index {
            apply(&mut attempts[index], event);
            if matches!(event, Event::ToolCall { .. }) && acts.contains(&attempts[index].context) {
                attempts[index].complete = true;
            }
        }
    }
    attempts
}
fn apply(attempt: &mut Attempt, event: &Event) {
    match event {
        Event::ToolCall {
            call_id,
            name,
            arguments,
        } => attempt.calls.push(Call {
            id: call_id.clone(),
            name: name.clone(),
            arguments: arguments.clone(),
            context: attempt.context.clone(),
            started: false,
            ended: false,
            result: None,
        }),
        Event::ModelCompleted { .. } => attempt.complete = true,
        Event::ToolStart { call_id }
        | Event::ToolEnd { call_id, .. }
        | Event::ToolResult { call_id, .. } => {
            if let Some(call) = attempt.calls.iter_mut().find(|call| &call.id == call_id) {
                match event {
                    Event::ToolStart { .. } => call.started = true,
                    Event::ToolEnd { .. } => call.ended = true,
                    Event::ToolResult { status, .. } => call.result = Some(*status),
                    _ => {}
                }
            }
        }
        _ => {}
    }
}
/// Failures of one call (its tool and canonical arguments) in a run (CORE-E-012, CORE-E-041).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct Failures {
    /// Since the run's last successful call of a tool that changes the workspace (`changes_workspace`).
    pub recent: u32,
    /// In the whole run.
    pub total: u32,
}
/// Failures of one call with no change between that stop the run.
pub(super) const REPEATED_FAILURES: u32 = 3;
/// Failures of one call in a run that stop it whatever changed between (backlog #18).
pub(super) const RUN_FAILURES: u32 = 6;
/// Why a run stops when one call has failed too often: three times with no change between, or six times in all.
/// `changes` says whether a tool's successful call may have changed the workspace.
pub(super) fn repeated(
    frames: &[ServerFrame],
    run: &RunId,
    changes: impl Fn(&str) -> bool,
) -> Option<&'static str> {
    let counts = failure_counts(frames, run, changes);
    if counts
        .values()
        .any(|failures| failures.recent >= REPEATED_FAILURES)
    {
        Some("the same tool call failed three times with no change between")
    } else if counts
        .values()
        .any(|failures| failures.total >= RUN_FAILURES)
    {
        Some("the same tool call failed six times in this run")
    } else {
        None
    }
}
pub(super) fn workspace_full_reported(frames: &[ServerFrame], run: &RunId) -> bool {
    frames.iter().any(|frame| {
        matches!(frame, ServerFrame::Event { run_id: Some(id), event: Event::WorkspaceFull { .. }, .. } if id == run)
    })
}
pub(super) fn failure_count(
    frames: &[ServerFrame],
    run: &RunId,
    call: &Call,
    changes: impl Fn(&str) -> bool,
) -> Failures {
    failure_counts(frames, run, changes)
        .get(&call_key(&call.name, &call.arguments))
        .copied()
        .unwrap_or_default()
}
fn call_key(name: &str, arguments: &Value) -> (String, String) {
    let mut canonical = arguments.clone();
    canonical.sort_all_objects();
    (name.into(), canonical.to_string())
}
/// One result counted: a failure adds to its call's counts; a successful change starts every recent count again.
fn counted(
    failures: &mut BTreeMap<(String, String), Failures>,
    key: &(String, String),
    status: ToolStatus,
    changes: impl Fn(&str) -> bool,
) {
    match status {
        ToolStatus::Error => {
            let count = failures.entry(key.clone()).or_default();
            count.recent += 1;
            count.total += 1;
        }
        ToolStatus::Ok if changes(&key.0) => {
            for count in failures.values_mut() {
                count.recent = 0;
            }
        }
        _ => {}
    }
}
/// Each call's failures in the run, in journal order: a successful call of a tool that changes the workspace starts
/// every call's recent count again; a failed one, or any other tool's, changes nothing.
fn failure_counts(
    frames: &[ServerFrame],
    run: &RunId,
    changes: impl Fn(&str) -> bool,
) -> BTreeMap<(String, String), Failures> {
    let mut calls = BTreeMap::new();
    let mut failures: BTreeMap<(String, String), Failures> = BTreeMap::new();
    for frame in frames {
        let ServerFrame::Event {
            run_id: Some(id),
            event,
            ..
        } = frame
        else {
            continue;
        };
        if id != run {
            continue;
        }
        match event {
            Event::StepStarted { .. } => calls.clear(),
            Event::ToolCall {
                call_id,
                name,
                arguments,
            } => {
                calls.insert(call_id.as_str(), call_key(name, arguments));
            }
            Event::ToolResult {
                call_id, status, ..
            } => {
                if let Some(key) = calls.get(call_id.as_str()) {
                    counted(&mut failures, key, *status, &changes);
                }
            }
            _ => {}
        }
    }
    failures
}
