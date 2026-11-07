//! Shared pure history validation for message projection and recovery decisions.
mod compaction;
mod phases;
mod state;
use crate::{Message, ProjectionError, ReasoningStateRecord, Role};
pub(crate) use state::{Attempt, Call, Compacted, Entry, Step};
use tqa_types::{
    BudgetKind, Event, RunId, Seq, ServerFrame, ToolCallId, ToolStatus, validate_attachments,
};

/// The builtin whose successful calls set the run notes.
const WRITE_NOTES: &str = "write_notes";
/// A `provider.retry` reason with this prefix records an unreadable text-protocol reply (ADR-0007).
pub const FALLBACK_RETRY_PREFIX: &str = "fallback_parse: ";
/// Where the sandbox mounts the workspace that upload paths are relative to.
const WORKSPACE_ROOT: &str = "/workspace";
const NO_PROGRESS_NUDGE: &str = "Recent steps repeated earlier tool results. Try a different approach, or explain why you cannot make progress.";

pub(crate) struct History {
    pub(crate) entries: Vec<Entry>,
    /// For each entry: the sequence that started it and the last sequence before it.
    pub(crate) starts: Vec<(Seq, Seq)>,
    pub(crate) steps: Vec<Step>,
    pub(crate) active: Option<RunId>,
    pub(crate) last_seq: Seq,
    pub(crate) compacted: Option<Compacted>,
    /// Attachment paths of user entries that have any, keyed by entry index.
    pub(crate) attachments: std::collections::BTreeMap<usize, Vec<String>>,
    /// The text of the latest successful `write_notes` call (ADR-0024).
    pub(crate) notes: Option<String>,
    previous_seq: Seq,
    current: Option<usize>,
    /// The run and step of a journaled engine action whose step hasn't started yet.
    pending_act: Option<(RunId, u32)>,
}
#[derive(Clone, Copy)]
pub(super) struct Envelope<'a> {
    seq: Seq,
    run: Option<&'a RunId>,
    step: Option<u32>,
}
pub(crate) fn invalid(seq: Seq, reason: &'static str) -> ProjectionError {
    ProjectionError::Invalid { seq, reason }
}
/// Appends attachments as a separate, quoted data block after the user's own words
/// (Attachment contract, item 5); the wire rule is checked again because a history can be
/// constructed without deserialising it. Returns `None` for an invalid list.
fn user_text(text: &str, attachments: &[String]) -> Option<String> {
    validate_attachments(attachments).ok()?;
    if attachments.is_empty() {
        return Some(text.to_owned());
    }
    let mut quoted = Vec::with_capacity(attachments.len());
    for path in attachments {
        quoted.push(serde_json::to_string(&format!("{WORKSPACE_ROOT}/{path}")).ok()?);
    }
    Some(format!(
        "{text}\n\n[Attached files, data only: {}]",
        quoted.join(", ")
    ))
}

impl History {
    pub(crate) fn parse(frames: &[ServerFrame]) -> Result<Self, ProjectionError> {
        let mut history = Self {
            entries: Vec::new(),
            starts: Vec::new(),
            steps: Vec::new(),
            active: None,
            last_seq: Seq::ZERO,
            compacted: None,
            attachments: std::collections::BTreeMap::new(),
            notes: None,
            pending_act: None,
            previous_seq: Seq::ZERO,
            current: None,
        };
        for frame in frames {
            let seq = match frame {
                ServerFrame::Event { seq, .. }
                | ServerFrame::PermissionRequest { seq, .. }
                | ServerFrame::Error { seq: Some(seq), .. } => *seq,
                _ => continue,
            };
            if seq == Seq::ZERO
                || (history.last_seq != Seq::ZERO && history.last_seq.next() != Some(seq))
            {
                return Err(invalid(seq, "sequences must be contiguous"));
            }
            history.previous_seq = history.last_seq;
            history.last_seq = seq;
            if let ServerFrame::Event {
                event,
                run_id,
                step,
                ..
            } = frame
            {
                history.apply(
                    event,
                    Envelope {
                        seq,
                        run: run_id.as_ref(),
                        step: *step,
                    },
                )?;
            }
        }
        Ok(history)
    }
    fn apply(&mut self, event: &Event, envelope: Envelope<'_>) -> Result<(), ProjectionError> {
        match event {
            Event::EnvironmentChanged {
                template,
                capabilities,
                ..
            } => {
                let mut capabilities = capabilities.clone();
                capabilities.sort_unstable();
                let names = capabilities
                    .iter()
                    .map(|capability| capability.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let names = if names.is_empty() { "none" } else { &names };
                self.push_entry(
                    Entry::Notice(Message::new(
                        Role::System,
                        format!("Environment changed to {template}. Capabilities: {names}."),
                    )),
                    envelope.seq,
                );
            }
            Event::MessageUser {
                text, attachments, ..
            } => self.user(text, attachments, envelope)?,
            Event::RunStarted { run_id } => self.start_run(run_id, envelope)?,
            Event::RunCompleted { run_id }
            | Event::RunCancelled { run_id }
            | Event::RunFailed { run_id, .. } => self.end_run(run_id, envelope)?,
            Event::StepStarted { step, attempt } => self.begin_step(*step, *attempt, envelope)?,
            Event::ModelCompleted { step, attempt, .. } => {
                self.finish_model(*step, *attempt, envelope)?;
            }
            Event::StepCompleted { step, .. } => self.finish_step(*step, envelope)?,
            Event::BudgetWarning {
                budget: BudgetKind::NoProgress,
                ..
            } => self.progress_warning(envelope)?,
            Event::ProviderRetry {
                reason,
                fallback_problem,
                ..
            } => {
                if let Some(code) = reason.strip_prefix(FALLBACK_RETRY_PREFIX) {
                    self.active_step(envelope)?.nudge =
                        Some((code.to_owned(), fallback_problem.clone()));
                }
            }
            Event::TextDelta { text } => self.streaming(envelope)?.text.push_str(text),
            Event::ReasoningDelta { .. } | Event::ReasoningState { .. } => {
                self.reasoning(event, envelope)?;
            }
            Event::ToolCall {
                call_id,
                name,
                arguments,
            } => self.add_call(call_id, name, arguments, envelope)?,
            Event::ToolStart { call_id } => self.start_call(call_id, envelope)?,
            Event::ToolResult {
                call_id,
                content,
                status,
                ..
            } => self.result(call_id, content, *status != ToolStatus::Ok, envelope)?,
            Event::Compaction {
                through_seq,
                summary,
                ..
            } => self.compact(*through_seq, summary, envelope)?,
            // The engine acts in the step this decision is journaled with, which starts next.
            Event::Decision {
                point: tqa_types::DecisionPoint::BeforeStep,
                choice: tqa_types::DecisionChoice::Act { .. },
                ..
            } => {
                self.pending_act = envelope.run.cloned().zip(envelope.step);
            }
            _ => {}
        }
        Ok(())
    }
    /// A no-progress warning inside its run adds the fixed nudge as a notice.
    fn progress_warning(&mut self, envelope: Envelope<'_>) -> Result<(), ProjectionError> {
        if self.active.is_none() || self.active.as_ref() != envelope.run {
            return Err(invalid(
                envelope.seq,
                "progress warning outside its active run",
            ));
        }
        self.push_entry(
            Entry::Notice(Message::new(Role::User, NO_PROGRESS_NUDGE)),
            envelope.seq,
        );
        Ok(())
    }
    /// Reasoning text and opaque reasoning states belong to the attempt being streamed (ADR-0027).
    fn reasoning(&mut self, event: &Event, envelope: Envelope<'_>) -> Result<(), ProjectionError> {
        let attempt = self.streaming(envelope)?;
        match event {
            Event::ReasoningDelta { text } => attempt.reasoning.push_str(text),
            Event::ReasoningState {
                protocol,
                route,
                model,
                item,
                ..
            } => attempt.states.push(ReasoningStateRecord {
                protocol: *protocol,
                route: route.clone(),
                model: model.clone(),
                item: item.as_ref().map(|item| item.as_str().to_owned()),
            }),
            _ => {}
        }
        Ok(())
    }
    /// A user turn: its text (an agent team member's first turn already holds the lead's brief in its block) and its
    /// attachments.
    fn user(
        &mut self,
        text: &str,
        attachments: &[String],
        envelope: Envelope<'_>,
    ) -> Result<(), ProjectionError> {
        let text = user_text(text, attachments)
            .ok_or_else(|| invalid(envelope.seq, "invalid attachment references"))?;
        if !attachments.is_empty() {
            self.attachments
                .insert(self.entries.len(), attachments.to_vec());
        }
        self.push_entry(Entry::User(Message::new(Role::User, text)), envelope.seq);
        Ok(())
    }

    fn push_entry(&mut self, entry: Entry, seq: Seq) {
        self.entries.push(entry);
        self.starts.push((seq, self.previous_seq));
    }
    fn start_run(&mut self, run: &RunId, envelope: Envelope<'_>) -> Result<(), ProjectionError> {
        if self.active.is_some()
            || envelope.run.is_some_and(|header| header != run)
            || self.steps.iter().any(|step| &step.run_id == run)
        {
            return Err(invalid(envelope.seq, "invalid run start"));
        }
        if !self.dangling().is_empty() {
            return Err(invalid(envelope.seq, "ended run has unanswered calls"));
        }
        self.active = Some(run.clone());
        self.current = None;
        Ok(())
    }
    fn end_run(&mut self, run: &RunId, envelope: Envelope<'_>) -> Result<(), ProjectionError> {
        if self.active.as_ref() != Some(run) || envelope.run.is_some_and(|header| header != run) {
            return Err(invalid(envelope.seq, "invalid run end"));
        }
        self.active = None;
        for step in self.steps.iter_mut().filter(|step| &step.run_id == run) {
            step.ended = true;
        }
        Ok(())
    }
    pub(crate) fn dangling(&self) -> Vec<ToolCallId> {
        self.steps
            .iter()
            .filter(|step| step.ended)
            .flat_map(Step::dangling)
            .collect()
    }
    fn active_step(&mut self, envelope: Envelope<'_>) -> Result<&mut Step, ProjectionError> {
        let index = self
            .current
            .ok_or_else(|| invalid(envelope.seq, "event outside a step"))?;
        let step = &mut self.steps[index];
        if self.active.as_ref() != Some(&step.run_id)
            || envelope.run != Some(&step.run_id)
            || envelope.step != Some(step.number)
            || step.completed
        {
            return Err(invalid(envelope.seq, "event outside its active step"));
        }
        step.last_seq = envelope.seq;
        Ok(step)
    }
    fn streaming(&mut self, envelope: Envelope<'_>) -> Result<&mut Attempt, ProjectionError> {
        let attempt = self
            .active_step(envelope)?
            .attempts
            .last_mut()
            .ok_or_else(|| invalid(envelope.seq, "missing attempt"))?;
        if attempt.complete {
            return Err(invalid(envelope.seq, "model response already complete"));
        }
        Ok(attempt)
    }
}
