//! Pure recovery decisions from the same validated history used for projection.
use crate::{
    ProjectionError,
    history::{History, invalid},
};
use tqa_types::{Event, RunId, Seq, ServerFrame, ToolCallId};

/// The work needed before another model request can be made safely.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResumePoint {
    /// Start the deterministic run for an already journalled, unanswered user message.
    StartRun {
        /// The message's run identifier, derived from its durable sequence.
        run_id: RunId,
    },
    /// Begin the next step of the active run.
    NextStep {
        /// The active run.
        run_id: RunId,
        /// The next step number.
        step: u32,
    },
    /// Continue the authoritative response's tools without repeating its model call.
    ToolPhase {
        /// The active run.
        run_id: RunId,
        /// The unfinished step.
        step: u32,
        /// Calls already answered, in model order.
        done: Vec<ToolCallId>,
        /// Started calls needing interrupted results, in model order.
        interrupted: Vec<ToolCallId>,
        /// Calls to dispatch through the permission broker, in model order.
        not_started: Vec<ToolCallId>,
    },
    /// Discard the partial highest attempt and request a new model response.
    RetryModel {
        /// The active run.
        run_id: RunId,
        /// The unfinished step.
        step: u32,
        /// The next model attempt number.
        attempt: u32,
    },
    /// No run is active; close any unanswered calls before starting another run.
    Idle {
        /// Ended runs' unanswered authoritative calls, in step and model order.
        dangling: Vec<ToolCallId>,
    },
}

/// Determines the next safe action without mutating history or producing synthetic results.
/// Supply every sequenced frame in contiguous order, including permission requests and errors.
/// The first sequenced frame sets the base; unsequenced frames do not advance it.
///
/// # Errors
/// Rejects inconsistent histories and exhausted step/attempt counters.
pub fn resume_point(frames: &[ServerFrame]) -> Result<ResumePoint, ProjectionError> {
    let history = History::parse(frames)?;
    let Some(run_id) = history.active.as_ref() else {
        if let Some(seq) = unanswered_message(frames) {
            if !history.dangling().is_empty() {
                return Err(invalid(
                    seq,
                    "user message precedes unfinished call recovery",
                ));
            }
            return Ok(ResumePoint::StartRun {
                run_id: format!("r-{seq}")
                    .parse()
                    .map_err(|_| invalid(seq, "invalid message run identifier"))?,
            });
        }
        return Ok(ResumePoint::Idle {
            dangling: history.dangling(),
        });
    };
    let Some(step) = history
        .steps
        .iter()
        .rev()
        .find(|step| &step.run_id == run_id)
    else {
        return Ok(ResumePoint::NextStep {
            run_id: run_id.clone(),
            step: 1,
        });
    };
    let exhausted = || ProjectionError::Exhausted {
        seq: history.last_seq,
    };
    if step.completed {
        return Ok(ResumePoint::NextStep {
            run_id: run_id.clone(),
            step: step.number.checked_add(1).ok_or_else(exhausted)?,
        });
    }
    let attempt = step
        .attempts
        .last()
        .ok_or_else(|| invalid(history.last_seq, "missing attempt"))?;
    if !attempt.complete {
        return Ok(ResumePoint::RetryModel {
            run_id: run_id.clone(),
            step: step.number,
            attempt: attempt.number.checked_add(1).ok_or_else(exhausted)?,
        });
    }
    let (mut done, mut interrupted, mut not_started) = (Vec::new(), Vec::new(), Vec::new());
    for call in &attempt.calls {
        let destination = if call.result.is_some() {
            &mut done
        } else if call.started {
            &mut interrupted
        } else {
            &mut not_started
        };
        destination.push(call.record.call_id.clone());
    }
    Ok(ResumePoint::ToolPhase {
        run_id: run_id.clone(),
        step: step.number,
        done,
        interrupted,
        not_started,
    })
}

fn unanswered_message(frames: &[ServerFrame]) -> Option<Seq> {
    for frame in frames.iter().rev() {
        match frame {
            ServerFrame::Event {
                event: Event::MessageUser { .. },
                seq,
                ..
            } => return Some(*seq),
            ServerFrame::Event {
                event: Event::RunStarted { .. },
                ..
            } => return None,
            _ => {}
        }
    }
    None
}

/// The frames of the last `turns` turns, for a client that shows recent history first and loads the rest on
/// demand (ARCHITECTURE §7, CTX-U-009). A turn starts at each user message, so consecutive user messages are
/// separate turns; frames before the first user message belong to no turn and are left out. Fewer turns than
/// requested give every turn; zero gives nothing.
#[must_use]
pub fn recent_turns(frames: &[ServerFrame], turns: usize) -> &[ServerFrame] {
    let starts: Vec<usize> = frames
        .iter()
        .enumerate()
        .filter(|(_, frame)| {
            matches!(
                frame,
                ServerFrame::Event {
                    event: Event::MessageUser { .. },
                    ..
                }
            )
        })
        .map(|(index, _)| index)
        .collect();
    if turns == 0 || starts.is_empty() {
        return &[];
    }
    &frames[starts[starts.len().saturating_sub(turns)]..]
}
