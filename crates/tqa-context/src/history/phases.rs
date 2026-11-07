//! Step and call transitions; a new attempt supersedes previous response data.
use super::{Attempt, Call, Entry, Envelope, History, Step, invalid};
use crate::{CallRecord, Message, ProjectionError, Role};
use tqa_types::ToolCallId;

impl History {
    pub(super) fn begin_step(
        &mut self,
        number: u32,
        attempt: u32,
        envelope: Envelope<'_>,
    ) -> Result<(), ProjectionError> {
        let run = self
            .active
            .as_ref()
            .ok_or_else(|| invalid(envelope.seq, "step outside a run"))?;
        if envelope.run != Some(run) || envelope.step != Some(number) || number == 0 || attempt == 0
        {
            return Err(invalid(envelope.seq, "invalid step context"));
        }
        if let Some(index) = self.current {
            let previous = &mut self.steps[index];
            if previous.number == number {
                return retry(previous, attempt, envelope);
            }
            if !previous.completed || number <= previous.number {
                return Err(invalid(
                    envelope.seq,
                    "previous step is unfinished or reordered",
                ));
            }
        }
        self.current = Some(self.steps.len());
        let run = run.clone();
        let act = self
            .pending_act
            .take()
            .is_some_and(|(pending, step)| pending == run && step == number);
        self.push_entry(Entry::Step(self.steps.len()), envelope.seq);
        self.steps.push(Step {
            run_id: run,
            number,
            first_seq: envelope.seq,
            last_seq: envelope.seq,
            attempts: vec![Attempt::new(attempt)],
            completed: false,
            ended: false,
            nudge: None,
            act,
        });
        Ok(())
    }
    pub(super) fn finish_model(
        &mut self,
        number: u32,
        attempt: u32,
        envelope: Envelope<'_>,
    ) -> Result<(), ProjectionError> {
        if envelope.step != Some(number) {
            return Err(invalid(envelope.seq, "model completion step mismatch"));
        }
        let current = self.streaming(envelope)?;
        if current.number != attempt {
            return Err(invalid(envelope.seq, "model completion attempt mismatch"));
        }
        current.complete = true;
        Ok(())
    }
    pub(super) fn finish_step(
        &mut self,
        number: u32,
        envelope: Envelope<'_>,
    ) -> Result<(), ProjectionError> {
        if envelope.step != Some(number) {
            return Err(invalid(envelope.seq, "step completion mismatch"));
        }
        let current = self.active_step(envelope)?;
        let ready = current.attempts.last().is_some_and(|attempt| {
            attempt.complete && attempt.calls.iter().all(|call| call.result.is_some())
        });
        if !ready {
            return Err(invalid(
                envelope.seq,
                "step has an unfinished model or tool",
            ));
        }
        current.completed = true;
        Ok(())
    }
    pub(super) fn add_call(
        &mut self,
        id: &ToolCallId,
        name: &str,
        arguments: &serde_json::Value,
        envelope: Envelope<'_>,
    ) -> Result<(), ProjectionError> {
        let act = self.current.is_some_and(|index| self.steps[index].act);
        let attempt = self.streaming(envelope)?;
        if attempt.calls.iter().any(|call| &call.record.call_id == id) {
            return Err(invalid(envelope.seq, "duplicate tool call"));
        }
        // An engine action's one call is its whole response.
        if act {
            attempt.complete = true;
        }
        attempt.calls.push(Call {
            record: CallRecord {
                call_id: id.clone(),
                name: name.into(),
                arguments: arguments.to_string(),
            },
            started: false,
            result: None,
            failed: false,
        });
        Ok(())
    }
    pub(super) fn start_call(
        &mut self,
        id: &ToolCallId,
        envelope: Envelope<'_>,
    ) -> Result<(), ProjectionError> {
        let attempt = self
            .active_step(envelope)?
            .attempts
            .last_mut()
            .ok_or_else(|| invalid(envelope.seq, "missing attempt"))?;
        if !attempt.complete {
            return Err(invalid(
                envelope.seq,
                "tool started before model completion",
            ));
        }
        let call = attempt
            .calls
            .iter_mut()
            .find(|call| &call.record.call_id == id)
            .ok_or_else(|| invalid(envelope.seq, "unknown tool call"))?;
        if call.started || call.result.is_some() {
            return Err(invalid(envelope.seq, "tool already started or finished"));
        }
        call.started = true;
        Ok(())
    }
    pub(super) fn result(
        &mut self,
        id: &ToolCallId,
        content: &str,
        failed: bool,
        envelope: Envelope<'_>,
    ) -> Result<(), ProjectionError> {
        let step = self
            .steps
            .iter_mut()
            .rev()
            .find(|step| envelope.run == Some(&step.run_id) && envelope.step == Some(step.number))
            .ok_or_else(|| invalid(envelope.seq, "result outside its step"))?;
        let attempt = step
            .attempts
            .last_mut()
            .ok_or_else(|| invalid(envelope.seq, "missing attempt"))?;
        let call = attempt
            .calls
            .iter_mut()
            .find(|call| &call.record.call_id == id)
            .ok_or_else(|| invalid(envelope.seq, "unknown tool call"))?;
        if call.result.is_some() {
            return Err(invalid(envelope.seq, "duplicate tool result"));
        }
        let mut message = Message::new(Role::Tool, content);
        message.tool_call_id = Some(id.clone());
        call.result = Some((envelope.seq, message));
        call.failed = failed;
        let notes = (!failed && call.record.name == super::WRITE_NOTES)
            .then(|| serde_json::from_str::<serde_json::Value>(&call.record.arguments).ok())
            .flatten()
            .and_then(|arguments| arguments.get("text")?.as_str().map(str::to_owned));
        step.last_seq = envelope.seq;
        if notes.is_some() {
            self.notes = notes;
        }
        Ok(())
    }
}

fn retry(previous: &mut Step, attempt: u32, envelope: Envelope<'_>) -> Result<(), ProjectionError> {
    if previous.completed
        || previous
            .attempts
            .last()
            .is_some_and(|old| old.number >= attempt)
    {
        return Err(invalid(
            envelope.seq,
            "attempt must increase within an unfinished step",
        ));
    }
    previous.attempts.push(Attempt::new(attempt));
    previous.last_seq = envelope.seq;
    Ok(())
}
