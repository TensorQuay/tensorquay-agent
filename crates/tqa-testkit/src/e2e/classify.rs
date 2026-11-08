//! Failure classification (TK-U-004). The first matching rule wins; anything the rules do
//! not recognise is a harness failure, so an unexplained outcome can never count as the model's fault.
use super::{integrity::IntegrityFault, integrity::seq};
use serde::Serialize;
use tqa_types::{ErrorCode, Event, Seq, ServerFrame, ToolStatus};

/// Tool error codes that come from the tool host, not from the model's request: a host I/O failure and
/// the sandbox's own proxy credential being refused. Every other code answers what the model asked for.
const INFRASTRUCTURE_TOOL_CODES: [&str; 2] = ["io", "proxy_authentication"];

/// The class of one attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum Class {
    /// The run completed and the check passed.
    #[serde(rename = "pass")]
    Pass,
    /// The API failed or answered with something undecodable.
    #[serde(rename = "harness.driver")]
    HarnessDriver,
    /// The event log has a gap or duplicate, or the stream and the replay differ.
    #[serde(rename = "harness.lost_event")]
    HarnessLostEvent,
    /// No terminal event arrived before the driver's deadline.
    #[serde(rename = "harness.timeout")]
    HarnessTimeout,
    /// The run ended for a reason that is not the model's.
    #[serde(rename = "harness.run")]
    HarnessRun,
    /// A tool failed in its host, not because of its arguments.
    #[serde(rename = "harness.tool")]
    HarnessTool,
    /// The model route failed after the product's own retries.
    #[serde(rename = "harness.provider")]
    HarnessProvider,
    /// The check could not run or did not report a verdict.
    #[serde(rename = "harness.check")]
    HarnessCheck,
    /// A request the task's approval policy denied stopped the task. Nothing produces it since ADR-0035 removed
    /// approvals; it stays so older results still read.
    #[serde(rename = "harness.task_design")]
    HarnessTaskDesign,
    /// The model changed a protected path.
    #[serde(rename = "model.tampered")]
    ModelTampered,
    /// The run completed but the check failed.
    #[serde(rename = "model.wrong_work")]
    ModelWrongWork,
    /// The run ended on our own tool refusing the model's arguments, the same call failing until the run stopped. It
    /// counts as the model's, so a model that really sends bad arguments doesn't block a gate; but the evidence keeps
    /// the refused arguments and our refusal, and every one is reviewed by hand: a refusal that was our strictness
    /// (an optional field sent as `""`, 2 Oct 2026) is reclassified as the harness's and fixed (TEST-PLAN §16).
    #[serde(rename = "model.tool_args")]
    ModelToolArgs,
    /// The model went round in circles: the same call failing until the run stopped (`repeated_failure`, other than
    /// our own refusal above), or steps with nothing new (`no_progress`). Before 7 Oct 2026 these counted as
    /// `model.budget`, which read as a budget stop; they are none.
    #[serde(rename = "model.stuck")]
    ModelStuck,
    /// The model's replies could not be read as a tool call, even after the retry prompt (`fallback_parse`, on a
    /// text-protocol route). Also `model.budget` before 7 Oct 2026.
    #[serde(rename = "model.unreadable")]
    ModelUnreadable,
    /// The model exhausted a budget (steps, tokens or time; our own over-budget cancel), or the agent's decision
    /// engine stopped the run (ADR-0013).
    #[serde(rename = "model.budget")]
    ModelBudget,
}
impl Class {
    /// Whether the harness, not the model, caused this failure.
    #[must_use]
    pub fn is_harness(self) -> bool {
        !matches!(
            self,
            Self::Pass
                | Self::ModelTampered
                | Self::ModelWrongWork
                | Self::ModelToolArgs
                | Self::ModelStuck
                | Self::ModelUnreadable
                | Self::ModelBudget
        )
    }
}

/// The check's verdict on a copy of the workspace.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckOutcome {
    /// Exit 0.
    Passed,
    /// Exit 1.
    Failed,
    /// A protected path differs from the seed.
    Tampered,
    /// Any other exit, a timeout, or a container failure.
    Broken,
}

/// Everything the driver observed about one attempt.
#[derive(Debug)]
pub struct Observation<'a> {
    /// The API failed or a frame could not be decoded.
    pub driver_failed: bool,
    /// The result of [`super::integrity::verify`].
    pub integrity: Result<(), IntegrityFault>,
    /// The driver cancelled the run because a task budget was exceeded.
    pub over_budget: bool,
    /// The driver's deadline passed before a terminal event; it then cancelled the run to stop spending.
    pub deadline_passed: bool,
    /// The replayed event log.
    pub events: &'a [ServerFrame],
    /// The check verdict; absent when the check did not run.
    pub check: Option<CheckOutcome>,
}

/// A class and the sequence number of the frame that decided it, when one did.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct Verdict {
    /// The class.
    pub class: Class,
    /// The deciding frame.
    pub evidence_seq: Option<Seq>,
}

enum Terminal {
    Completed,
    Cancelled,
    Failed(ErrorCode),
}

/// Classifies one attempt.
#[must_use]
pub fn classify(observed: &Observation<'_>) -> Verdict {
    let verdict = |class, evidence_seq| Verdict {
        class,
        evidence_seq,
    };
    if observed.driver_failed {
        return verdict(Class::HarnessDriver, None);
    }
    if let Err(fault) = observed.integrity {
        return verdict(Class::HarnessLostEvent, Some(fault.seq()));
    }
    if observed.deadline_passed {
        return verdict(Class::HarnessTimeout, None);
    }
    if let Some(at) = infrastructure_tool_error(observed.events) {
        return verdict(Class::HarnessTool, Some(at));
    }
    let Some((at, terminal)) = terminal(observed.events) else {
        return verdict(Class::HarnessTimeout, None);
    };
    let class = match (terminal, observed.check) {
        (Terminal::Failed(ErrorCode::ProviderError), _) => Class::HarnessProvider,
        (_, Some(CheckOutcome::Broken)) | (Terminal::Completed, None) => Class::HarnessCheck,
        (_, Some(CheckOutcome::Tampered)) => Class::ModelTampered,
        (Terminal::Completed, Some(CheckOutcome::Passed)) => Class::Pass,
        (Terminal::Completed, Some(CheckOutcome::Failed)) => Class::ModelWrongWork,
        (Terminal::Failed(ErrorCode::RepeatedFailure), _) if refused_arguments(observed.events) => {
            Class::ModelToolArgs
        }
        (Terminal::Failed(ErrorCode::RepeatedFailure | ErrorCode::NoProgress), _) => {
            Class::ModelStuck
        }
        (Terminal::Failed(ErrorCode::FallbackParse), _) => Class::ModelUnreadable,
        (Terminal::Failed(ErrorCode::Budget | ErrorCode::DecisionStop), _) => Class::ModelBudget,
        (Terminal::Cancelled, _) if observed.over_budget => Class::ModelBudget,
        _ => Class::HarnessRun,
    };
    verdict(class, Some(at))
}

fn terminal(events: &[ServerFrame]) -> Option<(Seq, Terminal)> {
    events.iter().rev().find_map(|frame| {
        let ServerFrame::Event { event, .. } = frame else {
            return None;
        };
        let terminal = match event {
            Event::RunCompleted { .. } => Terminal::Completed,
            Event::RunCancelled { .. } => Terminal::Cancelled,
            Event::RunFailed { code, .. } => Terminal::Failed(*code),
            _ => return None,
        };
        Some((seq(frame)?, terminal))
    })
}

/// The evidence of a [`Class::ModelToolArgs`] attempt, for its hand review: the last refused call's tool and
/// arguments, and our refusal, as `<tool> <arguments> refused: <refusal>`.
#[must_use]
pub fn refused_call(events: &[ServerFrame]) -> Option<String> {
    let (call_id, refusal) = events.iter().rev().find_map(|frame| match frame {
        ServerFrame::Event {
            event:
                Event::ToolResult {
                    call_id,
                    status: ToolStatus::Error,
                    content,
                    ..
                },
            ..
        } if content.starts_with("invalid_args:") => Some((call_id, content)),
        _ => None,
    })?;
    let (name, arguments) = events.iter().find_map(|frame| match frame {
        ServerFrame::Event {
            event:
                Event::ToolCall {
                    call_id: id,
                    name,
                    arguments,
                },
            ..
        } if id == call_id => Some((name.as_str(), arguments.to_string())),
        _ => None,
    })?;
    Some(format!("{name} {arguments} refused: {refusal}"))
}

/// Whether the failed tool results before the run's end were all our tools refusing the model's arguments
/// (`invalid_args: ...`), as a repeated failure ends: the last three, or as many as there are.
fn refused_arguments(events: &[ServerFrame]) -> bool {
    let failures: Vec<&str> = events
        .iter()
        .rev()
        .filter_map(|frame| match frame {
            ServerFrame::Event {
                event:
                    Event::ToolResult {
                        status: ToolStatus::Error,
                        content,
                        ..
                    },
                ..
            } => Some(content.as_str()),
            _ => None,
        })
        .take(3)
        .collect();
    !failures.is_empty()
        && failures
            .iter()
            .all(|content| content.starts_with("invalid_args:"))
}

/// A tool result reports `<code>: <reason>` (the tool crate's model message).
fn infrastructure_tool_error(events: &[ServerFrame]) -> Option<Seq> {
    events.iter().find_map(|frame| match frame {
        ServerFrame::Event {
            seq,
            event:
                Event::ToolResult {
                    status: ToolStatus::Error,
                    content,
                    ..
                },
            ..
        } if INFRASTRUCTURE_TOOL_CODES.iter().any(|code| {
            content
                .strip_prefix(code)
                .is_some_and(|rest| rest.starts_with(':'))
        }) =>
        {
            Some(*seq)
        }
        _ => None,
    })
}

#[cfg(test)]
mod tests;
