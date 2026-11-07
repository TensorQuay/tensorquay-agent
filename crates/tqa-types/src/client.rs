//! Exact client action frames used by both protocol bindings.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    ConversationId, Environment, FrameError, NetworkSnapshot, RouteSnapshot, RunId, RunLimits, Seq,
    WorkspaceSnapshot,
};

/// Client inputs are exact: unknown fields and malformed bounded values are rejected.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
#[non_exhaustive]
#[allow(
    clippy::large_enum_variant,
    reason = "The link configuration carries the complete session snapshot as named fields."
)]
pub enum ClientFrame {
    /// Gateway-to-runtime configuration, rejected on every client binding.
    #[serde(rename = "session.config")]
    SessionConfig {
        /// The configured conversation.
        conversation_id: ConversationId,
        /// The durable gateway watermark for replay.
        last_stored_seq: Seq,
        /// A route without credentials or upstream settings.
        route: RouteSnapshot,
        /// The task's network allow-list, for the prompt (ADR-0035 §3); the gateway's proxy enforces it.
        network: NetworkSnapshot,
        /// Validated run budgets.
        limits: RunLimits,
        /// Workspace presentation facts.
        workspace: WorkspaceSnapshot,
        /// Declared environment; omission preserves the legacy Minimal shell environment.
        #[serde(default, skip_serializing_if = "Environment::is_legacy")]
        environment: Environment,
        /// Protocol version, negotiated separately from parsing.
        protocol: u16,
        /// Admin-declared fallback routes the run may switch to, in order; empty from older gateways.
        #[serde(
            default,
            skip_serializing_if = "Vec::is_empty",
            deserialize_with = "crate::validation::fallback_routes"
        )]
        #[schemars(length(max = 3))]
        fallback_routes: Vec<RouteSnapshot>,
        /// MCP servers, applied from the next run; empty from older gateways (revision 7).
        #[serde(
            default,
            skip_serializing_if = "Vec::is_empty",
            deserialize_with = "crate::mcp::servers"
        )]
        #[schemars(schema_with = "crate::mcp::servers_schema")]
        mcp: Vec<crate::McpServer>,
        /// The agent profile the session runs under; absent from older gateways and without a profile (revision 8).
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::profile::profile"
        )]
        #[schemars(schema_with = "crate::profile::profile_schema")]
        profile: Option<crate::ProfileSnapshot>,
        /// The decision engine the session consults; absent means the model decides (revision 9, ADR-0013). The
        /// gateway sends it only to a runtime announcing revision 9 or later.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::decision::engine_snapshot"
        )]
        #[schemars(schema_with = "crate::decision::engine_snapshot_schema")]
        decision_engine: Option<crate::DecisionEngineSnapshot>,
        /// The gateway's protocol revision; absent from older gateways. The gateway announces it so a runtime can
        /// tell which additions the gateway reads.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gateway_revision: Option<u16>,
        /// The install CA the sandbox trusts for keyed hosts, PEM, the current one first (revision 11, ADR-0037 §6);
        /// absent from older gateways and without a CA.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::host_key::host_ca"
        )]
        #[schemars(schema_with = "crate::host_key::host_ca_schema")]
        host_ca: Option<String>,
        /// The keys of the run in progress, with placeholders minted for this sandbox, for the runtime to give that
        /// run when it resumes (revision 11, ADR-0037 §3); empty, and not written, when no run with keys is going.
        #[serde(
            default,
            skip_serializing_if = "Vec::is_empty",
            deserialize_with = "crate::host_key::grants"
        )]
        #[schemars(schema_with = "crate::host_key::grants_schema")]
        run_host_keys: Vec<crate::HostKeyGrant>,
        /// The `computer` tool's per-run caps, for an environment that declares `desktop` (revision 14, ADR-0039
        /// section 4); absent means their defaults.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        computer_limits: Option<crate::ComputerLimits>,
    },
    /// Gateway-to-runtime route replacement for subsequent runs, rejected on client bindings.
    #[serde(rename = "session.update")]
    SessionUpdate {
        /// A route without credentials or upstream settings.
        route: RouteSnapshot,
        /// The new route's fallback routes, in order.
        #[serde(
            default,
            skip_serializing_if = "Vec::is_empty",
            deserialize_with = "crate::validation::fallback_routes"
        )]
        #[schemars(length(max = 3))]
        fallback_routes: Vec<RouteSnapshot>,
    },
    /// Gateway-to-runtime workspace request (revision 7), rejected on every client binding.
    #[serde(rename = "workspace.request")]
    WorkspaceRequest {
        /// Correlates the reply; never reused on a link.
        request_id: u64,
        /// The operation.
        op: crate::WorkspaceOp,
    },
    /// Gateway-to-runtime cancellation of a workspace request, rejected on every client binding.
    #[serde(rename = "workspace.cancel")]
    WorkspaceCancel {
        /// The request to stop.
        request_id: u64,
    },
    /// Open a conversation and request replay.
    #[serde(rename = "session.init")]
    SessionInit {
        /// The conversation to open.
        conversation_id: ConversationId,
        /// The last received sequence, or zero before any event.
        last_seq: Seq,
        /// The requested protocol version, checked during negotiation.
        protocol: u16,
    },
    /// Submit a nonempty user message.
    #[serde(rename = "user.message")]
    UserMessage {
        /// Message text containing at most 64 KiB of UTF-8 bytes.
        #[serde(deserialize_with = "crate::validation::message")]
        #[schemars(length(min = 1, max = 65536))]
        text: String,
        /// At most 32 distinct upload paths (`uploads/<name>`), in submission order; may be empty.
        #[serde(deserialize_with = "crate::validation::attachments")]
        #[schemars(schema_with = "crate::validation::attachment_schema")]
        attachments: Vec<String>,
        /// The run's host keys, which only the gateway adds as it delivers the turn to the runtime (revision 11,
        /// ADR-0037 §3); empty, and not written, everywhere else.
        #[serde(
            default,
            skip_serializing_if = "Vec::is_empty",
            deserialize_with = "crate::host_key::grants"
        )]
        #[schemars(schema_with = "crate::host_key::grants_schema")]
        host_keys: Vec<crate::HostKeyGrant>,
        /// On an agent team member's first turn, the lead's brief (revision 12, ADR-0034); absent, and not written,
        /// on every other message.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lead_brief: Option<crate::LeadBrief>,
        /// On the gateway's wake of an agent team reader, whose messages wait (revision 13, ADR-0034); absent, and
        /// not written, on every other message.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        team_wake: Option<crate::TeamWake>,
    },
    /// Request cooperative cancellation of a run.
    #[serde(rename = "run.cancel")]
    RunCancel {
        /// The run to cancel.
        run_id: RunId,
    },
    /// Sign out everywhere (revision 14, ADR-0039 section 6): the runtime closes the desktop's browser, empties its
    /// profile, and answers `screen.sign_out.done`; the next action starts the browser fresh. Sent only when no run is
    /// active.
    #[serde(rename = "screen.sign_out")]
    ScreenSignOut {},
    /// The gateway's answer to a screenshot upload (revision 14, ADR-0039 section 9): kept, or why not. The runtime
    /// journals the capture's `screen.capture` only after `ok`.
    #[serde(rename = "screen.stored")]
    ScreenStored {
        /// The upload's id, its PNG's SHA-256.
        #[serde(deserialize_with = "crate::screen::sha256_hex")]
        #[schemars(regex(pattern = r"^[0-9a-f]{64}$"))]
        id: String,
        /// Whether the store holds it now.
        ok: bool,
        /// Why not, when `ok` is false; absent otherwise.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<crate::StoreRefusal>,
    },
}

impl ClientFrame {
    /// Parses a bounded JSON action without applying transport or negotiation policy.
    ///
    /// # Errors
    /// Rejects oversized, malformed, unknown or invalid client frames with a field path when available.
    pub fn parse(bytes: &[u8]) -> Result<Self, FrameError> {
        let frame: Self = crate::parse::parse(
            bytes,
            &[
                "session.init",
                "session.config",
                "session.update",
                "user.message",
                "run.cancel",
                "screen.sign_out",
                "screen.stored",
                "workspace.request",
                "workspace.cancel",
            ],
        )?;
        if let Self::WorkspaceRequest { op, .. } = &frame {
            op.check()?;
        }
        if let Self::ScreenStored { ok, reason, .. } = &frame
            && *ok == reason.is_some()
        {
            return Err(FrameError {
                code: crate::ErrorCode::InvalidFrame,
                path: "reason".into(),
                detail: "reason comes with ok: false only, and always with it".into(),
            });
        }
        if let Self::SessionConfig {
            route,
            fallback_routes,
            ..
        }
        | Self::SessionUpdate {
            route,
            fallback_routes,
        } = &frame
            && fallback_routes
                .iter()
                .any(|fallback| fallback.name == route.name)
        {
            return Err(FrameError {
                code: crate::ErrorCode::InvalidFrame,
                path: "$.fallback_routes".into(),
                detail: "a fallback route must differ from the route".into(),
            });
        }
        Ok(frame)
    }
}
