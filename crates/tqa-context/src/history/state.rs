//! In-memory state derived exclusively from the supplied history.
use crate::{CallRecord, Message, ReasoningStateRecord};
use tqa_types::{CompactionSummary, FallbackProblem, RunId, Seq, ToolCallId};

pub(crate) enum Entry {
    User(Message),
    Notice(Message),
    Step(usize),
}
pub(crate) struct Step {
    pub(crate) run_id: RunId,
    pub(crate) number: u32,
    /// The sequences of the step's first and latest step-scoped events.
    pub(crate) first_seq: Seq,
    pub(crate) last_seq: Seq,
    pub(crate) attempts: Vec<Attempt>,
    pub(crate) completed: bool,
    pub(crate) ended: bool,
    /// The failure code of an unreadable text-protocol reply that this step retried, and what exactly was wrong when
    /// the retry says (ADR-0007).
    pub(crate) nudge: Option<(String, Option<FallbackProblem>)>,
    /// The decision engine acted in this step instead of the model (ADR-0013 point 2): its attempt is complete once its
    /// call is journaled, and it reaches the model as a user-role note.
    pub(crate) act: bool,
}
pub(crate) struct Attempt {
    pub(crate) number: u32,
    pub(crate) complete: bool,
    pub(crate) text: String,
    pub(crate) reasoning: String,
    pub(crate) states: Vec<ReasoningStateRecord>,
    pub(crate) calls: Vec<Call>,
}
impl Attempt {
    pub(crate) fn new(number: u32) -> Self {
        Self {
            number,
            complete: false,
            text: String::new(),
            reasoning: String::new(),
            states: Vec::new(),
            calls: Vec::new(),
        }
    }
}
pub(crate) struct Call {
    pub(crate) record: CallRecord,
    pub(crate) started: bool,
    pub(crate) result: Option<(Seq, Message)>,
    /// The result's status was not `ok`.
    pub(crate) failed: bool,
}
/// The latest applied compaction: entries starting at or before `through` are replaced,
/// except the pinned first and current user messages.
pub(crate) struct Compacted {
    pub(crate) through: Seq,
    pub(crate) summary: CompactionSummary,
    /// The entry index of the latest user message at the compaction event.
    pub(crate) current_user: Option<usize>,
}
impl Step {
    pub(crate) fn dangling(&self) -> Vec<ToolCallId> {
        self.attempts
            .last()
            .filter(|attempt| attempt.complete)
            .map_or_else(Vec::new, |attempt| {
                attempt
                    .calls
                    .iter()
                    .filter(|call| call.result.is_none())
                    .map(|call| call.record.call_id.clone())
                    .collect()
            })
    }
}
