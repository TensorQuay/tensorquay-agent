//! Wire protocol frames, event types, JSON Schemas, identifiers and error codes shared
//! across the workspace.

mod action_bytes;
mod catalog;
mod channel;
mod chunk;
mod client;
mod compaction;
mod context_loaded;
mod decision;
pub mod decision_v1;
mod environment;
mod error;
mod error_target;
mod event;
mod fallback_problem;
mod gateway_error;
mod git_push;
mod host_key;
mod ids;
mod image;
mod key_shapes;
mod limits;
mod mcp;
mod network;
mod parse;
mod permission;
mod profile;
mod queue;
mod reasoning_state;
mod redact;
mod revision;
mod run_limits;
mod schema;
mod screen;
mod screen_notice;
mod screen_upload;
mod secret_names;
mod seq;
mod server;
mod session;
mod strict_schema;
mod team;
mod time;
mod tool_failure;
mod tool_name;
mod validation;
mod version;
mod workspace;
mod workspace_link;

pub use action_bytes::canonical_action_bytes;
pub use catalog::{
    MAX_TOOL_CATALOG_BYTES, MAX_TOOL_SCHEMA_BYTES, MAX_TOOLS, ToolCatalogBudget, ToolCatalogEntry,
    ToolCatalogLimit,
};
pub use channel::{
    Author, AuthorKind, ChannelFilter, ChannelMessage, ChannelPost, ChannelRole, ClaimLimit,
    DEFAULT_PROPOSAL_WINDOW_SECONDS, DecisionRule, MAX_CHANNEL_BODY_BYTES, MAX_CHANNEL_MEMBERS,
    MAX_CHANNEL_MESSAGES_PER_DAY, MAX_CHANNEL_READ, MAX_CHANNEL_WAIT_SECONDS, MessageBody,
    MessageType, Outcome, PROPOSAL_WINDOW_SECONDS, ReportStatus, VoteChoice,
};
pub use chunk::{ChunkError, ToolOutputAssembler, chunk_tool_output};
pub use client::ClientFrame;
pub use compaction::{CompactionSource, CompactionSummary, InvalidCompactionSummary};
pub use context_loaded::{
    ContextLoaded, LoadedInstruction, LoadedSkill, MAX_CONTEXT_LOADED_ITEMS, SkillSkipReason,
    SkillTier, SkippedSkill,
};
pub use decision::{
    DECISION_ENGINE_REVISION, DecisionChoice, DecisionEngineSnapshot, DecisionExposure,
    DecisionFallbackReason, DecisionPoint, MAX_DECISION_ACT_BYTES, MAX_DECISION_CALLS,
    MAX_DECISION_ENGINE_NAME_BYTES, MAX_DECISION_STOP_REASON_CHARS, MAX_DECISION_TIMEOUT_MS,
    MAX_DECISION_TOOLS, valid_decision_engine_name,
};
pub use environment::{Capability, Environment};
pub use error::{ErrorCode, FrameError};
pub use error_target::ErrorTarget;
pub use event::{BudgetKind, Event, FinishReason, McpServerState, SandboxState, ToolStatus};
pub use fallback_problem::{
    FallbackProblem, JsonType, MAX_MODEL_NAME_BYTES, MAX_PROBLEM_NAME_BYTES, MAX_PROBLEM_NAMES,
    MAX_PROBLEM_PATH_SEGMENTS, ProblemName, ProblemPath,
};
pub use gateway_error::{GATEWAY_ERROR_HEADER, gateway_error_code};
pub use git_push::GitPushTarget;
pub use git_push::relay::{
    GIT_PUSH_OPERATIONS_PATH, GitPushAuthorization, GitPushFailure, GitPushOperationId,
    GitPushOperationRequest, GitPushReceipt, GitPushRelayError, MAX_GIT_PUSH_CONTROL_BYTES,
    MAX_GIT_PUSH_PACK_BYTES, MAX_GIT_PUSH_START_MS,
};
pub use host_key::{
    HostKeyGrant, HostKeyOutcome, HostKeyScheme, MAX_HOST_CA_BYTES, MAX_RUN_HOST_KEYS,
    PLACEHOLDER_PREFIX,
};
pub use ids::{ConversationId, InvalidId, RunId, Seq, ToolCallId, WorkspaceId};
pub use image::{IMAGE_HEADER_BYTES, ImageMediaType};
pub use key_shapes::{KEY_SHAPES_JSON, is_key_shaped_name};
pub use limits::*;
pub use mcp::{McpServer, McpToolFilter, McpTransport, valid_mcp_name};
pub use network::{MAX_NETWORK_ENTRIES, MAX_NETWORK_MORE, NetworkEntry, NetworkSnapshot};
pub use permission::{ActionHash, PermissionContext};
pub use profile::{ProfileSnapshot, SkillsFilter, valid_profile_name, valid_skill_name};
pub use queue::{MAX_QUEUE_AHEAD, QueuePosition, QueueReason};
pub use reasoning_state::{InvalidReasoningItem, ReasoningItem};
pub use redact::redact;
pub use revision::{
    InvalidRevision, PROTOCOL_REVISIONS, ProtocolRevision, RuntimeTooOld, check_runtime_revision,
    parse_revision,
};
pub use run_limits::RunLimits;
pub use schema::{decision_schema, protocol_schema};
pub use screen::{
    CapturePhase, ComputerLimits, DEFAULT_MAX_ACTIONS_PER_RUN, DEFAULT_MAX_SCREENSHOTS_PER_RUN,
    MAX_SCREEN_NAME_CHARS, MAX_SCREENSHOT_BYTES, MAX_SCREENSHOT_SIDE, ScreenEvidence, ScreenRole,
    ScreenTarget,
};
pub use screen_notice::{
    ScreenEnded, ScreenNoticeKind, ScreenNoticeReason, ScreenPerson, SignOutFailure,
};
pub use screen_upload::{MAX_UPLOAD_CHUNK_BYTES, MAX_UPLOAD_CHUNK_CHARS, StoreRefusal};
pub use secret_names::{
    MAX_SECRET_NAME_BYTES, PER_USER_SECRET_PREFIXES, is_per_user_secret, is_secret_reference,
};
pub use seq::{SeqChecker, SeqError};
pub use server::{SandboxBackend, SandboxRecovery, SandboxSummary, ServerFrame};
pub use session::{CacheHint, ReasoningSnapshot, RouteSnapshot, WireProtocol, WorkspaceSnapshot};
pub use strict_schema::strict_compatible;
pub use team::{
    Brief, CheckReport, EndFile, EndReason, EndReport, EndTokens, FilesTaken, FromKind, HandedFile,
    LeadBrief, MAX_BRIEF_BYTES, MAX_BRIEF_PART_BYTES, MAX_CHECK_OUTPUT_BYTES, MAX_ROLE_BYTES,
    MAX_TEAM_REQUEST_BYTES, MemberTokens, NoticeKind, PlacedFile, StartMember, TakeFiles, TeamFrom,
    TeamMessage, TeamMessageBody, TeamMessageType, TeamPost, TeamTo, TeamWake, ToKind,
    WorkspaceMode, last_bytes,
};
pub use time::{Clock, Sleeper};
pub use tool_failure::ToolFailure;
pub use tool_name::{split_mcp_tool_name, valid_builtin_tool_name, valid_tool_name};
pub use validation::{InvalidAttachments, valid_provider_message, validate_attachments};
pub use version::{UnsupportedProtocol, check_protocol};
pub use workspace::{WorkspaceClone, WorkspaceCloneStatus};
pub use workspace_link::{
    EntryKind, ListItem, ManifestEntry, StatItem, UploadedFile, WorkspaceErrorCode, WorkspaceOp,
    WorkspaceReply, WorkspaceResult,
};

/// Returns this crate's package name.
#[must_use]
pub fn crate_name() -> &'static str {
    "tqa-types"
}

#[cfg(test)]
mod tests {
    #[test]
    fn returns_crate_name() {
        assert_eq!(super::crate_name(), "tqa-types");
    }
}
