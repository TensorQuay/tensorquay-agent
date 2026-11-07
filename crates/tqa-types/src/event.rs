//! Protocol-v1 lifecycle and streaming events.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Capability, ErrorCode, RunId, ToolCallId};

/// The run budget approaching its hard ceiling.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BudgetKind {
    /// Completed steps within the run.
    Steps,
    /// Active execution time in whole seconds.
    WallClock,
    /// Output tokens across all attempts, including estimated counts when usage is absent.
    OutputTokens,
    /// Consecutive completed steps without new tool-result content.
    NoProgress,
}

/// Lifecycle state shared by summaries and state events.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SandboxState {
    /// The sandbox is being provisioned.
    Creating,
    /// The sandbox is ready for work.
    Ready,
    /// A run is executing.
    Busy,
    /// No run is executing.
    Idle,
    /// A run is waiting for a free execution slot.
    Queued,
    /// Retired by ADR-0035 (a run parked for an approval): read from histories and summaries written before it,
    /// never sent.
    Parked,
    /// The sandbox has been removed.
    Destroyed,
    /// The sandbox cannot continue.
    Failed,
}

/// Terminal status of one tool execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ToolStatus {
    /// The tool completed successfully.
    Ok,
    /// The tool failed.
    Error,
    /// The user or supervisor cancelled the tool.
    Cancelled,
    /// Execution was interrupted before completion.
    Interrupted,
}

/// Provider-independent reason that a model response finished.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FinishReason {
    /// The model reached its normal stopping condition.
    Stop,
    /// The model yielded one or more tool calls.
    ToolCalls,
    /// The response reached an output limit.
    Length,
    /// A content filter stopped the response.
    ContentFilter,
    /// The provider reported another completion reason.
    Other,
}

/// The skeleton event vocabulary; unknown fields are ignored for additive compatibility.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum Event {
    /// Repository initialization progress, journaled before the first model run.
    #[serde(rename = "workspace.clone")]
    WorkspaceClone {
        /// Whether initialization started, completed, was skipped or failed.
        status: crate::WorkspaceCloneStatus,
        /// Stable failure classification; omitted for progress and success.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<ErrorCode>,
        /// Fixed status text within 256 UTF-8 bytes; never a URL or process output.
        #[serde(deserialize_with = "crate::workspace::status_message")]
        #[schemars(schema_with = "crate::workspace::status_message_schema")]
        message: String,
    },
    /// Retired by ADR-0035 (an approval's answer): read from histories written before it, never sent. The old
    /// stored payload parses; none of its fields is kept.
    #[serde(rename = "permission.resolved")]
    PermissionResolved {},
    /// Retired by ADR-0035 (an approval that expired): read from histories written before it, never sent.
    #[serde(rename = "permission.expired")]
    PermissionExpired {},
    /// The environment's template or declared capability set changed.
    #[serde(rename = "environment.changed")]
    EnvironmentChanged {
        /// Validated template slug; the image digest is not part of event identity.
        #[serde(deserialize_with = "crate::environment::template")]
        #[schemars(schema_with = "crate::environment::template_schema")]
        template: String,
        /// Unique capabilities, normalized into canonical enum order.
        #[serde(deserialize_with = "crate::environment::capabilities")]
        #[schemars(schema_with = "crate::environment::capabilities_schema")]
        capabilities: Vec<Capability>,
        /// The template version now running, for an image-only update; absent otherwise (revision 8, ADR-0032).
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::environment::version"
        )]
        #[schemars(schema_with = "crate::environment::version_schema")]
        version: Option<u64>,
    },
    /// A validated user message enters the conversation journal.
    #[serde(rename = "message.user")]
    MessageUser {
        /// Nonempty text containing at most 64 KiB of UTF-8 bytes.
        #[serde(deserialize_with = "crate::validation::message")]
        #[schemars(length(min = 1, max = 65536))]
        text: String,
        /// Up to 32 distinct upload paths (`uploads/<name>`), in submission order.
        #[serde(deserialize_with = "crate::validation::attachments")]
        #[schemars(schema_with = "crate::validation::attachment_schema")]
        attachments: Vec<String>,
        /// The lead's brief, when this is an agent team member's first turn (revision 12, ADR-0034): shown as the
        /// brief from the lead, never as the person's words.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lead_brief: Option<crate::LeadBrief>,
        /// The gateway's wake of an agent team reader (revision 13, ADR-0034): shown as the gateway's notice of
        /// waiting messages, never as the person's words.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        team_wake: Option<crate::TeamWake>,
    },
    /// A run begins.
    #[serde(rename = "run.started")]
    RunStarted {
        /// The run that started.
        run_id: RunId,
    },
    /// What the runtime loaded into the model context when the run started or resumed: instruction files, skills
    /// and skipped skill folders (FR-11). For the context panel only; it never enters the model context.
    #[serde(rename = "context.loaded")]
    ContextLoaded {
        /// Instruction files in prompt order, at most 64.
        #[serde(deserialize_with = "crate::context_loaded::instructions")]
        #[schemars(length(max = 64))]
        instructions: Vec<crate::LoadedInstruction>,
        /// Skills in index order, at most 64.
        #[serde(deserialize_with = "crate::context_loaded::skills")]
        #[schemars(length(max = 64))]
        skills: Vec<crate::LoadedSkill>,
        /// Skill folders that were not loaded, with their reasons, at most 64.
        #[serde(deserialize_with = "crate::context_loaded::skipped")]
        #[schemars(length(max = 64))]
        skipped: Vec<crate::SkippedSkill>,
    },
    /// A numbered model-and-tool step begins.
    #[serde(rename = "step.started")]
    StepStarted {
        /// The step number within the run.
        step: u32,
        /// The attempt number; older frames default to the first attempt.
        #[serde(default = "first_attempt")]
        attempt: u32,
    },
    /// A numbered step and its selected attempt complete.
    #[serde(rename = "step.completed")]
    StepCompleted {
        /// The step number within the run.
        step: u32,
        /// Active time for this step; older frames contribute zero duration.
        #[serde(default)]
        duration_ms: u64,
    },
    /// Older turns were replaced by a structured summary before the next model call.
    #[serde(rename = "compaction")]
    Compaction {
        /// The last replaced sequence; later events and the pinned user messages are kept.
        through_seq: crate::Seq,
        /// Whether the model or the deterministic fallback wrote the summary.
        source: crate::CompactionSource,
        /// The summary format and prompt version; exactly 1.
        #[serde(deserialize_with = "crate::compaction::version")]
        #[schemars(schema_with = "crate::compaction::version_schema")]
        version: u32,
        /// The bounded summary, shown to the model as quoted data.
        #[serde(deserialize_with = "crate::compaction::summary")]
        summary: crate::CompactionSummary,
    },
    /// A run budget reached its warning threshold.
    #[serde(rename = "budget.warning")]
    BudgetWarning {
        /// The measured budget.
        budget: BudgetKind,
        /// Consumed steps or whole active seconds.
        used: u64,
        /// Configured ceiling in the same unit.
        limit: u64,
    },
    /// The run switched to another route after its route failed; later steps of the run stay there.
    #[serde(rename = "route.fallback")]
    RouteFallback {
        /// The route that failed, or a fallback that is no longer configured.
        #[serde(deserialize_with = "crate::validation::route_name")]
        #[schemars(length(min = 1, max = 256))]
        from: String,
        /// The route the run continues on.
        #[serde(deserialize_with = "crate::validation::route_name")]
        #[schemars(length(min = 1, max = 256))]
        to: String,
        /// Error code and optional HTTP status, at most 128 bytes; never a response body.
        #[serde(deserialize_with = "crate::validation::retry_reason")]
        #[schemars(length(max = 128))]
        reason: String,
    },
    /// Opaque provider reasoning state for the response being streamed, recorded in stream order
    /// before that response's calls (ADR-0027). It is replayed only to the same route and model.
    /// `item` is absent when the state exceeded its bounds, and always absent in the client form.
    #[serde(rename = "reasoning.state")]
    ReasoningState {
        /// The protocol of the route that produced the item.
        protocol: crate::WireProtocol,
        /// The route that produced the item.
        #[serde(deserialize_with = "crate::validation::route_name")]
        #[schemars(length(min = 1, max = 256))]
        route: String,
        /// The route's model when the item was produced.
        #[serde(deserialize_with = "crate::validation::model_name")]
        #[schemars(length(min = 1, max = 256))]
        model: String,
        /// The item's size in bytes, kept when the item itself is not.
        bytes: u32,
        /// The provider item as received; never logged or shown.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[schemars(length(max = 131_072))]
        item: Option<crate::ReasoningItem>,
    },
    /// A failed model attempt will retry after bounded backoff.
    #[serde(rename = "provider.retry")]
    ProviderRetry {
        /// The model attempt that failed.
        attempt: u32,
        /// The delay before the next attempt, in milliseconds.
        delay_ms: u64,
        /// Error code and optional HTTP status, at most 128 bytes; never a response body.
        #[serde(deserialize_with = "crate::validation::retry_reason")]
        #[schemars(length(max = 128))]
        reason: String,
        /// The provider's own words about the failure, sanitised and bounded; absent when there are none.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::validation::provider_message"
        )]
        #[schemars(schema_with = "crate::validation::provider_message_schema")]
        provider_message: Option<String>,
        /// For a `fallback_parse` retry, what exactly made the reply unreadable, named without any of its content
        /// (ADR-0007); absent on every other retry and from older runtimes.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fallback_problem: Option<crate::FallbackProblem>,
    },
    /// All model deltas and prepared tool calls for an attempt have arrived.
    #[serde(rename = "model.completed")]
    ModelCompleted {
        /// The step number within the run.
        step: u32,
        /// The completed model attempt.
        attempt: u32,
        /// The reason the response finished.
        finish_reason: FinishReason,
    },
    /// A visible text fragment arrives.
    #[serde(rename = "text.delta")]
    TextDelta {
        /// The text fragment, which may be empty.
        text: String,
    },
    /// A reasoning fragment arrives.
    #[serde(rename = "reasoning.delta")]
    ReasoningDelta {
        /// The reasoning fragment, which may be empty.
        text: String,
    },
    /// A tool call has been prepared.
    #[serde(rename = "tool.call")]
    ToolCall {
        /// The tool-call identifier.
        call_id: ToolCallId,
        /// The registered tool name.
        name: String,
        /// The tool's JSON arguments.
        arguments: serde_json::Value,
    },
    /// Tool execution begins.
    #[serde(rename = "tool.start")]
    ToolStart {
        /// The tool call being executed.
        call_id: ToolCallId,
    },
    /// An ordered UTF-8 output chunk arrives.
    #[serde(rename = "tool.output")]
    ToolOutput {
        /// The call whose output is being streamed.
        call_id: ToolCallId,
        /// The zero-based chunk index for this call.
        chunk: u32,
        /// Whether this is the final chunk.
        last: bool,
        /// The UTF-8 output bytes before JSON escaping.
        data: String,
    },
    /// Tool execution reaches a terminal state.
    #[serde(rename = "tool.end")]
    ToolEnd {
        /// The completed tool call.
        call_id: ToolCallId,
        /// The terminal execution status.
        status: ToolStatus,
        /// A process exit status when the tool exposes one.
        exit_code: Option<i32>,
        /// Why the call failed, as data for clients (revision 10, C1); absent when it didn't fail or from older
        /// runtimes.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        failure: Option<crate::ToolFailure>,
    },
    /// The bounded, authoritative tool response used for conversation history.
    #[serde(rename = "tool.result")]
    ToolResult {
        /// The tool call to which this result belongs.
        call_id: ToolCallId,
        /// The terminal execution status.
        status: ToolStatus,
        /// UTF-8 content capped at 256 KiB before JSON escaping.
        #[serde(deserialize_with = "crate::validation::tool_result")]
        #[schemars(length(max = 262_144))]
        content: String,
        /// On a `computer` call that acted on an element by ref, that element (revision 14, ADR-0039), for a client's
        /// step line; absent, and not written, on every other result. Never part of the model's input.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<crate::ScreenTarget>,
        /// On a `computer` action that takes evidence screenshots, which of them were kept, and on a `screenshot` call's
        /// Ok result whether its screenshot was (`after` only) (revision 14, ADR-0039); absent, and not written, on
        /// every other result.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        evidence: Option<crate::ScreenEvidence>,
    },
    /// A decision the engine made and the loop applied (ADR-0013 point 4), journaled before it takes effect. Only a
    /// non-default engine emits it, and it reaches the wire with the protocol revision of ADR-0013 point 7.
    #[serde(rename = "decision")]
    Decision {
        /// Where the engine was consulted.
        point: crate::DecisionPoint,
        /// The engine, by its name.
        #[serde(deserialize_with = "crate::decision::engine")]
        #[schemars(length(min = 1, max = 256))]
        engine: String,
        /// The decision as applied.
        choice: crate::DecisionChoice,
        /// How long the engine took to answer.
        latency_ms: u64,
    },
    /// The loop set an engine's answer aside and used the model's own decisions for the step (ADR-0013 point 4).
    #[serde(rename = "decision.fallback")]
    DecisionFallback {
        /// Where the engine was consulted.
        point: crate::DecisionPoint,
        /// The engine, by its name.
        #[serde(deserialize_with = "crate::decision::engine")]
        #[schemars(length(min = 1, max = 256))]
        engine: String,
        /// Why.
        reason: crate::DecisionFallbackReason,
    },
    /// An MCP server's state for this session (ADR-0011), emitted around a run's start; additive, so no revision.
    #[serde(rename = "mcp.server")]
    McpServer {
        /// The server's name under the MCP name rule.
        #[serde(deserialize_with = "crate::mcp::name")]
        #[schemars(schema_with = "crate::mcp::name_schema")]
        name: String,
        /// Where the server stands.
        state: McpServerState,
        /// The tools it registered, at most 64.
        #[serde(deserialize_with = "crate::mcp::tool_count")]
        #[schemars(range(max = 64))]
        tools: u32,
        /// A stable reason code for a disabled or restarted server, 1 through 64 bytes of `[a-z_]`.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::mcp::reason"
        )]
        #[schemars(schema_with = "crate::mcp::reason_schema")]
        reason: Option<String>,
    },
    /// The workspace had no space left: a tool failed with `workspace_full`, or a command failed while the workspace's
    /// filesystem had no free blocks (SBX-C-015). At most once per run, just before that call's `tool.result`.
    #[serde(rename = "workspace.full")]
    WorkspaceFull {
        /// The first call that failed for lack of space.
        call_id: ToolCallId,
    },
    /// Token usage is reported without narrowing counters.
    #[serde(rename = "usage")]
    Usage {
        /// Input tokens charged by the route.
        prompt_tokens: u64,
        /// Output tokens charged by the route.
        completion_tokens: u64,
        /// Cached input tokens when reported.
        cached_tokens: Option<u64>,
        /// Reasoning tokens when reported.
        reasoning_tokens: Option<u64>,
        /// Whether the counts are estimates because the provider omitted usage.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        estimated: bool,
        /// The runner's byte estimate of the request this call sent, exactly as it was built: after the older replayed
        /// states it dropped, with its tools and its latest turn's states, without images (CORE-U-084). With
        /// `prompt_tokens`, it gives the route's density. Absent for a compaction's summary call and in older journals.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        estimated_input: Option<u64>,
        /// The image tokens the runner counted for the images this call sent, which `prompt_tokens` also holds
        /// (CORE-U-084). Absent when `estimated_input` is.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        estimated_images: Option<u64>,
        /// The route this call was counted on, by its immutable name (ADR-0028: the key conversations store, not the
        /// editable display name): a density holds only on the route that measured it (CORE-U-084). Absent when
        /// `estimated_input` is.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        estimated_route: Option<String>,
    },
    /// A run completes successfully.
    #[serde(rename = "run.completed")]
    RunCompleted {
        /// The completed run.
        run_id: RunId,
    },
    /// A run stops after cancellation.
    #[serde(rename = "run.cancelled")]
    RunCancelled {
        /// The cancelled run.
        run_id: RunId,
    },
    /// A run stops with a classified failure.
    #[serde(rename = "run.failed")]
    RunFailed {
        /// The failed run.
        run_id: RunId,
        /// The human-readable failure detail.
        reason: String,
        /// The stable failure classification.
        code: ErrorCode,
        /// The provider's own words about the failure, sanitised and bounded; absent when there are none.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::validation::provider_message"
        )]
        #[schemars(schema_with = "crate::validation::provider_message_schema")]
        provider_message: Option<String>,
    },
    /// A journalled run resumes at a complete step.
    #[serde(rename = "run.resumed")]
    RunResumed {
        /// The resumed run.
        run_id: RunId,
        /// The step from which execution resumes.
        from_step: u32,
    },
    /// Retired by ADR-0035 (a run parked for an approval): read from histories written before it, never sent.
    #[serde(rename = "run.parked")]
    RunParked {},
    /// One screenshot a `computer` call took, for the evidence or for the model (revision 14, ADR-0039), journaled
    /// only once the conversation's screenshot store holds it; a client fetches it by `id`.
    #[serde(rename = "screen.capture")]
    ScreenCapture {
        /// The `computer` call it belongs to.
        call_id: ToolCallId,
        /// When it was taken.
        phase: crate::CapturePhase,
        /// Its id in the screenshot store: 1 to 128 ASCII letters, digits, `-` and `_`.
        #[serde(deserialize_with = "crate::screen::screenshot_id")]
        #[schemars(length(min = 1, max = 128), regex(pattern = r"^[A-Za-z0-9_-]+$"))]
        id: String,
        /// Its width in pixels.
        #[serde(deserialize_with = "crate::screen::screenshot_side")]
        #[schemars(range(min = 1, max = 8192))]
        width: u32,
        /// Its height in pixels.
        #[serde(deserialize_with = "crate::screen::screenshot_side")]
        #[schemars(range(min = 1, max = 8192))]
        height: u32,
        /// The stored file's size in bytes, at most 512 KiB.
        #[serde(deserialize_with = "crate::screen::screenshot_bytes")]
        #[schemars(range(min = 1, max = 524_288))]
        bytes: u64,
        /// The stored file's SHA-256, in lowercase hex.
        #[serde(deserialize_with = "crate::screen::sha256_hex")]
        #[schemars(length(min = 64, max = 64), regex(pattern = r"^[0-9a-f]{64}$"))]
        sha256: String,
        /// Whether the model was sent it.
        sent_to_model: bool,
        /// Whether it carries the snapshot's marks: only the model's own screenshot does; evidence is unmarked.
        marked: bool,
    },
    /// The sandbox lifecycle state changes.
    #[serde(rename = "sandbox.state")]
    SandboxState {
        /// The new sandbox state.
        state: SandboxState,
    },
}

fn first_attempt() -> u32 {
    1
}

/// An MCP server's state in a session (ADR-0011).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum McpServerState {
    /// Starting: spawned or connecting, handshake pending.
    Starting,
    /// Ready: its tools are registered for the run.
    Ready,
    /// Restarted once after a crash.
    Restarted,
    /// Disabled for the rest of the session.
    Disabled,
}
