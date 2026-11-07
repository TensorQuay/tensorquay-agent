//! Host-independent orchestration over durable events and injected services.
mod brief;
mod captures;
mod compaction;
mod journal;
mod model;
mod output;
mod preflight;
mod prepare;
mod progress;
mod recovery;
mod routes;
mod session;
mod tools;
mod watch;

use crate::{CoreError, DecisionEngine, EventSink, RunOutcome};
pub use preflight::{PreRunFailure, fail_before_run};
pub use routes::RouteProvider;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio_util::sync::CancellationToken;
use tqa_llm::{Jitter, ModelProvider};
use tqa_tools::{ToolLimits, ToolRegistry, WorkspaceRoot};
use tqa_types::{Clock, ConversationId, RouteSnapshot, RunLimits, ServerFrame, Sleeper};

/// Fixed prompt slots assembled by the prompt crate and runtime before a run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PromptInputs {
    /// The versioned system prompt.
    pub system: String,
    /// Deterministically rendered repository instructions and the skills index.
    pub instructions: String,
    /// What the instruction slot was built from, emitted as `context.loaded` when a run starts or resumes; absent,
    /// no event is emitted.
    pub loaded: Option<tqa_types::ContextLoaded>,
}
/// A new user turn; the loop validates the wire text and attachment limits.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserTurn {
    /// Nonempty user text within the protocol bound.
    pub text: String,
    /// Upload paths (`uploads/<name>`) in submission order.
    pub attachments: Vec<String>,
    /// On an agent team member's first turn, the lead's brief (revision 12): journalled with the turn and given to
    /// the model after the text inside the untrusted-content marker.
    pub lead_brief: Option<Box<tqa_types::LeadBrief>>,
    /// On the gateway's wake of an agent team reader (revision 13): journalled with the turn, whose text the model
    /// gets as it is.
    pub team_wake: Option<tqa_types::TeamWake>,
}
/// Explicit dependencies; none of these services is discovered from the host by the loop.
pub struct RunnerDeps {
    /// Trusted conversation identity included in every prepared action.
    pub conversation_id: ConversationId,
    /// The configured model route adapter.
    pub provider: Arc<dyn ModelProvider>,
    /// Immutable registered tools and schemas.
    pub tools: Arc<ToolRegistry>,
    /// Validated decision hooks.
    pub decision: Arc<dyn DecisionEngine>,
    /// The durable append boundary.
    pub sink: Arc<dyn EventSink>,
    /// Shared monotonic clock.
    pub clock: Arc<dyn Clock>,
    /// Shared sleeper driven by that clock.
    pub sleeper: Arc<dyn Sleeper>,
    /// Fixed request prefix.
    pub prompt: PromptInputs,
    /// Trusted `.tqa/<conversation>/spill` directory supplied by the runtime.
    pub spill_dir: PathBuf,
    /// Explicit run ceilings.
    pub limits: RunLimits,
    /// Validated, secret-free session route.
    pub route: RouteSnapshot,
    /// Already opened workspace confinement boundary.
    pub workspace: WorkspaceRoot,
    /// Environment names permitted for tool execution.
    pub env_allow: Vec<String>,
    /// Explicit tool input, output and execution bounds.
    pub tool_limits: ToolLimits,
    /// Retry jitter; production supplies `ThreadRngJitter` from the provider crate.
    pub jitter: Box<dyn Jitter>,
    /// Admin-declared fallback routes, in order; empty keeps every run on `route` (ADR-0025).
    pub fallback_routes: Vec<RouteProvider>,
    /// The runtime's link to the gateway, settled after a model attempt's transport failure; `None` treats the gateway
    /// as always reachable (CORE-U-062).
    pub gateway_link: Option<Arc<dyn crate::GatewayLink>>,
    /// Where the run's `computer` screenshots are kept for the model, in the runtime's state (ADR-0039 section 3);
    /// `None` keeps none, and every screenshot goes to the model as its fixed line.
    pub screenshots: Option<crate::Screenshots>,
    /// The conversation's screenshot store, for a `computer` call's captures (ADR-0039 section 9); `None` uploads
    /// nothing, so no `screen.capture` is journaled and no `tool.result` carries `evidence`.
    pub screen_store: Option<Arc<dyn crate::ScreenStore>>,
}
/// Reusable services; every run's semantic state is rebuilt from its journal.
/// The supervisor serializes invocations for a conversation and always awaits shutdown cleanup.
pub struct Runner {
    conversation_id: ConversationId,
    provider: Arc<dyn ModelProvider>,
    tools: Arc<ToolRegistry>,
    /// The model-facing names of this run's fixed catalogue (ADR-0011 decision 4).
    names: tqa_tools::ModelNames,
    decision: Arc<dyn DecisionEngine>,
    sink: Arc<dyn EventSink>,
    clock: Arc<dyn Clock>,
    sleeper: Arc<dyn Sleeper>,
    prompt: PromptInputs,
    spill_dir: PathBuf,
    limits: RunLimits,
    route: RouteSnapshot,
    workspace: WorkspaceRoot,
    env_allow: Vec<String>,
    tool_limits: ToolLimits,
    jitter: Mutex<Box<dyn Jitter>>,
    fallbacks: Vec<RouteProvider>,
    gateway_link: Option<Arc<dyn crate::GatewayLink>>,
    screenshots: Option<crate::Screenshots>,
    screen_store: Option<Arc<dyn crate::ScreenStore>>,
}
impl Runner {
    /// Validates injected settings without accessing the host.
    ///
    /// # Errors
    /// Rejects invalid run/tool limits, route snapshots or a mismatched provider route.
    pub fn new(deps: RunnerDeps) -> Result<Self, CoreError> {
        deps.limits
            .validate()
            .map_err(|_| CoreError::InvalidConfiguration("run limits"))?;
        deps.route
            .validate()
            .map_err(|_| CoreError::InvalidConfiguration("route snapshot"))?;
        deps.tool_limits
            .validate()
            .map_err(|_| CoreError::InvalidConfiguration("tool limits"))?;
        routes::validate(&deps.route, &deps.fallback_routes)?;
        if deps.provider.route().name != deps.route.name
            || deps.provider.route().model != deps.route.model
        {
            return Err(CoreError::InvalidConfiguration(
                "provider route does not match session",
            ));
        }
        Ok(Self {
            conversation_id: deps.conversation_id,
            provider: deps.provider,
            names: tqa_tools::ModelNames::new(deps.tools.specs().map(|spec| &spec.name)),
            tools: deps.tools,
            decision: deps.decision,
            sink: deps.sink,
            clock: deps.clock,
            sleeper: deps.sleeper,
            prompt: deps.prompt,
            spill_dir: deps.spill_dir,
            limits: deps.limits,
            route: deps.route,
            workspace: deps.workspace,
            env_allow: deps.env_allow,
            tool_limits: deps.tool_limits,
            jitter: Mutex::new(deps.jitter),
            fallbacks: deps.fallback_routes,
            gateway_link: deps.gateway_link,
            screenshots: deps.screenshots,
            screen_store: deps.screen_store,
        })
    }
    /// Advances a journal through recovery, model attempts and tools to an idle or terminal outcome.
    /// User cancellation closes calls; shutdown stops writes and leaves the run resumable.
    /// An in-progress sink append completes under either stop token.
    ///
    /// # Errors
    /// Rejects invalid history, busy input and malformed turns; sink failure stops all further writes.
    pub async fn run(
        &self,
        history: &[ServerFrame],
        message: Option<UserTurn>,
        cancel: CancellationToken,
        shutdown: CancellationToken,
    ) -> Result<RunOutcome, CoreError> {
        let point = tqa_context::resume_point(history)?;
        if message.is_some() && !matches!(point, tqa_context::ResumePoint::Idle { .. }) {
            return Err(CoreError::Busy);
        }
        if let Some(message) = &message {
            validate_turn(message)?;
        }
        // A new run first removes the screenshots earlier runs left (ended, or cut by a crash); a resumed run keeps its
        // own. Done here, never after a run's last event: the next turn may arrive the moment run.completed is out.
        if message.is_some()
            && let Some(screenshots) = &self.screenshots
        {
            screenshots.forget_all_but(None).await;
        }
        let mut execution = session::Execution::new(self, history, cancel, shutdown);
        let result = execution.execute(point, message).await;
        let outcome = execution.resolve(result).await;
        execution.join_workers().await;
        outcome
    }
}
fn validate_turn(message: &UserTurn) -> Result<(), CoreError> {
    let frame = tqa_types::ClientFrame::UserMessage {
        lead_brief: None,
        team_wake: None,
        text: message.text.clone(),
        attachments: message.attachments.clone(),
        host_keys: Vec::new(),
    };
    let bytes =
        serde_json::to_vec(&frame).map_err(|_| CoreError::Internal("cannot encode user turn"))?;
    tqa_types::ClientFrame::parse(&bytes)?;
    Ok(())
}

fn run_id_for_message(seq: tqa_types::Seq) -> Result<tqa_types::RunId, CoreError> {
    format!("r-{seq}")
        .parse()
        .map_err(|_| CoreError::Internal("invalid generated run identifier"))
}
