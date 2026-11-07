//! Initialization failures use the same durable turn/run boundary without model services.
use super::{UserTurn, journal, run_id_for_message, validate_turn};
use crate::{CoreError, EventContext, EventSink, RunOutcome, SinkError};
use tokio_util::sync::CancellationToken;
use tqa_context::ResumePoint;
use tqa_types::{ErrorCode, Event, RunId, Seq, ServerFrame};

/// A classified initialization failure and the pending turn it prevents from executing.
pub struct PreRunFailure<'a> {
    /// Complete contiguous journal, including initialization status events.
    pub history: &'a [ServerFrame],
    /// A new turn, or no turn when resuming the journal after shutdown.
    pub message: Option<UserTurn>,
    /// Stable initialization failure code.
    pub code: ErrorCode,
    /// Fixed reason within 256 UTF-8 bytes without controls; never process output or credentials.
    pub reason: &'a str,
}

/// Journals a pending turn and its failed run without constructing or calling model services.
/// Shutdown leaves the append boundary resumable; an in-progress append is always awaited.
///
/// # Errors
/// Rejects invalid input/history or an already started model step; sink failure stops all writes.
pub async fn fail_before_run(
    sink: &dyn EventSink,
    input: PreRunFailure<'_>,
    shutdown: &CancellationToken,
) -> Result<RunOutcome, CoreError> {
    if input.reason.is_empty()
        || input.reason.len() > 256
        || input.reason.chars().any(char::is_control)
    {
        return Err(CoreError::InvalidConfiguration("pre-run failure reason"));
    }
    let point = tqa_context::resume_point(input.history)?;
    if input.message.is_some() && !matches!(point, ResumePoint::Idle { .. }) {
        return Err(CoreError::Busy);
    }
    if let Some(message) = &input.message {
        validate_turn(message)?;
    }
    if shutdown.is_cancelled() {
        return Ok(RunOutcome::Interrupted);
    }
    let mut writer = FailureJournal {
        sink,
        shutdown,
        context: EventContext::default(),
        previous: input.history.iter().rev().find_map(journal::sequence),
    };
    let Some((run_id, started)) = writer.start(point, input.message).await? else {
        return Ok(if shutdown.is_cancelled() {
            RunOutcome::Interrupted
        } else {
            RunOutcome::Idle
        });
    };
    writer.context.run_id = Some(run_id.clone());
    if !started
        && writer
            .append(Event::RunStarted {
                run_id: run_id.clone(),
            })
            .await?
            .is_none()
    {
        return Ok(RunOutcome::Interrupted);
    }
    if writer
        .append(Event::RunFailed {
            run_id,
            code: input.code,
            reason: input.reason.into(),
            provider_message: None,
        })
        .await?
        .is_none()
    {
        return Ok(RunOutcome::Interrupted);
    }
    Ok(RunOutcome::Failed {
        code: input.code,
        reason: input.reason.into(),
    })
}

struct FailureJournal<'a> {
    sink: &'a dyn EventSink,
    shutdown: &'a CancellationToken,
    context: EventContext,
    previous: Option<Seq>,
}
impl FailureJournal<'_> {
    async fn start(
        &mut self,
        point: ResumePoint,
        message: Option<UserTurn>,
    ) -> Result<Option<(RunId, bool)>, CoreError> {
        match point {
            ResumePoint::Idle { dangling } if dangling.is_empty() => {
                let Some(message) = message else {
                    return Ok(None);
                };
                let Some(seq) = self.append(super::brief::message_event(message)?).await? else {
                    return Ok(None);
                };
                Ok(Some((run_id_for_message(seq)?, false)))
            }
            ResumePoint::StartRun { run_id } => Ok(Some((run_id, false))),
            ResumePoint::NextStep { run_id, step: 1 } => Ok(Some((run_id, true))),
            _ => Err(CoreError::Busy),
        }
    }
    async fn append(&mut self, event: Event) -> Result<Option<Seq>, CoreError> {
        if self.shutdown.is_cancelled() {
            return Ok(None);
        }
        let seq = self.sink.emit(event, self.context.clone()).await?;
        if seq == Seq::ZERO
            || self
                .previous
                .is_some_and(|previous| previous.next() != Some(seq))
        {
            return Err(CoreError::Sink(SinkError {
                code: ErrorCode::SeqGap,
            }));
        }
        self.previous = Some(seq);
        Ok(Some(seq))
    }
}
