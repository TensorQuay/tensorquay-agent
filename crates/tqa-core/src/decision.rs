//! Narrow, deterministic decision hooks over journal-derived run state (ADR-0013).
use async_trait::async_trait;
use tqa_llm::{Completion, ToolCall};
use tqa_types::{DecisionExposure, DecisionPoint, RunId, ToolStatus};

/// An immutable description of the step being considered.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunState {
    /// The active run identifier.
    pub run_id: RunId,
    /// One-based step number.
    pub step: u32,
    /// One-based authoritative attempt number.
    pub attempt: u32,
    /// The session's own logical route name.
    pub route: String,
    /// The routes a decision may choose: the session's own first, then its declared fallbacks in order.
    pub routes: Vec<String>,
    /// Available registered tool names in stable order.
    pub tools: Vec<String>,
    /// The run's use of its budgets so far, against their limits.
    pub budget: BudgetUse,
    /// The previous step's calls in journal order, by name with their terminal status.
    pub previous_calls: Vec<CallSummary>,
    /// The conversation as the model sees it, at most its last 64 KiB; only with exposure `full`.
    pub transcript: Option<Vec<TranscriptLine>>,
}
/// One message of the transcript an engine with exposure `full` sees.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TranscriptLine {
    /// `user`, `assistant`, `tool` or `system`.
    pub role: String,
    /// The message's text.
    pub text: String,
}
/// The run's budget use when a step is considered.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BudgetUse {
    /// Steps completed in this run.
    pub steps_used: u32,
    /// The run's step limit.
    pub max_steps: u32,
    /// Output tokens spent in this run.
    pub output_tokens_used: u64,
    /// The run's output-token limit.
    pub max_output_tokens: u64,
}
/// One call of the previous step, as a decision may see it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallSummary {
    /// The registered tool name.
    pub name: String,
    /// The call's terminal status; `None` when it has none.
    pub status: Option<ToolStatus>,
    /// The call's arguments; only with exposure `calls` or `full`.
    pub arguments: Option<serde_json::Value>,
}
/// Why an engine gave no usable answer. The loop uses the model's own decisions for the step, and records the reason
/// once the decision events exist (ADR-0013 point 4).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EngineFailure {
    /// The engine could not be reached.
    Unreachable,
    /// The engine did not answer in time.
    Timeout,
    /// The engine answered with something that isn't a decision.
    Invalid,
    /// The engine's budget is spent: the gateway refused before anything was sent.
    BudgetSpent,
}
/// A complete bounded model response, retaining raw tool arguments.
pub type ModelResponse = Completion;
/// An action selected from the original response by identifier.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedAction {
    /// The original model call; invented or duplicate identifiers invalidate the entire plan.
    pub call_id: String,
}
/// The decision vocabulary reserved for the full engine contract.
#[derive(Clone, Debug, PartialEq)]
pub enum Decision {
    /// Call the current session route with a valid subset of registered tools.
    CallModel {
        /// Logical route name.
        route: String,
        /// Selected tool names; the loop restores stable registry order.
        tool_subset: Vec<String>,
    },
    /// Execute an engine-supplied call; reserved for a later version.
    Act(ToolCall),
    /// Compact context; reserved for a later version.
    Compact,
    /// Stop for an engine-supplied reason; reserved for a later version.
    Stop(String),
}
/// The decision hooks: an engine may narrow the step's tools or stop, never widen them (ADR-0013).
#[async_trait]
pub trait DecisionEngine: Send + Sync {
    /// The engine's name, its route name, for the `decision` and `decision.fallback` events. `None`, the default,
    /// journals nothing: [`ModelDecides`] leaves the journal as it was before the engine existed (ADR-0013 point 1).
    fn name(&self) -> Option<&str> {
        None
    }
    /// How much of the conversation the engine may see; the loop filters what it passes (ADR-0013 point 5). `none`, the
    /// default, shows calls by id and name only.
    fn exposure(&self) -> DecisionExposure {
        DecisionExposure::None
    }
    /// Whether the engine answers at `point`. At a point it doesn't, the loop never consults it and journals nothing,
    /// as without an engine there (the default: both).
    fn answers(&self, _point: DecisionPoint) -> bool {
        true
    }
    /// Chooses the step's model route and tool subset, once per step; the step's retries and failover reuse it.
    ///
    /// # Errors
    /// An [`EngineFailure`] when the engine has no usable answer; the step then uses the model's own decisions.
    async fn before_step(&self, state: &RunState) -> Result<Decision, EngineFailure>;
    /// Selects original calls; omitted calls still receive error results.
    ///
    /// # Errors
    /// An [`EngineFailure`] when the engine has no usable answer; every call of the response then runs.
    async fn after_response(
        &self,
        state: &RunState,
        response: &ModelResponse,
    ) -> Result<Vec<PlannedAction>, EngineFailure>;
}
/// The identity engine: the model selects all actions on the configured route.
#[derive(Clone, Copy, Debug, Default)]
pub struct ModelDecides;
#[async_trait]
impl DecisionEngine for ModelDecides {
    async fn before_step(&self, state: &RunState) -> Result<Decision, EngineFailure> {
        Ok(Decision::CallModel {
            route: state.route.clone(),
            tool_subset: state.tools.clone(),
        })
    }
    async fn after_response(
        &self,
        _: &RunState,
        response: &ModelResponse,
    ) -> Result<Vec<PlannedAction>, EngineFailure> {
        Ok(response
            .tool_calls
            .iter()
            .map(|call| PlannedAction {
                call_id: call.id.clone(),
            })
            .collect())
    }
}
