//! Redacted gateway failures with stable HTTP and protocol classifications.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use tqa_types::{ErrorCode, ServerFrame};

#[path = "error_file.rs"]
mod file;
#[cfg(test)]
#[path = "error_tests.rs"]
mod tests;
pub use file::FileRefusal;

/// A bounded, secret-free gateway failure. Database and I/O internals are never exposed.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum GatewayError {
    /// A named configuration invariant failed.
    #[error("invalid gateway configuration: {0}")]
    Configuration(&'static str),
    /// As [`Self::Configuration`], for the TOML parse's refusal: the field's path and serde's reason, or a syntax
    /// error's position, built at run time and never holding a value from the file.
    #[error("invalid gateway configuration: {0}")]
    ConfigurationField(String),
    /// A request failed structural or boundary validation.
    #[error("invalid request: {0}")]
    BadRequest(&'static str),
    /// One request field failed validation; reported as `details` with a reason from the closed list. A field of a
    /// removed feature (reason `removed`) says what to do instead, since it is refused, never ignored.
    #[error("{}", invalid_message(field, *reason))]
    Invalid {
        /// The request field, query parameter or path segment, by its documented name.
        field: &'static str,
        /// Why it was refused.
        reason: InvalidReason,
    },
    /// As [`Self::Invalid`], for a field named only at run time: one a strict decoder refused, unknown fields included.
    #[error("invalid request field {field}")]
    InvalidField {
        /// The field, as the request spelled it.
        field: String,
        /// Why it was refused.
        reason: InvalidReason,
    },
    /// A field names a route, template, tool, server or connector that does not exist (ADR-0012); reported as
    /// `details` like [`Self::Invalid`].
    #[error("unknown reference: {field}")]
    UnknownReference {
        /// The request field that names it.
        field: &'static str,
    },
    /// A conversation cannot be made from a profile now: `field` names what the install lacks or refuses
    /// (ADR-0012 decision 5); reported as `details`.
    #[error("profile unusable: {field}")]
    ProfileUnusable {
        /// The profile field.
        field: &'static str,
    },
    /// A request sets or widens a field its conversation's profile sets (ADR-0012 decision 4); reported as `details`.
    #[error("set by profile: {field}")]
    SetByProfile {
        /// The request field.
        field: &'static str,
    },
    /// The request body exceeds the configured byte limit.
    #[error("request body exceeds its byte limit")]
    BodyTooLarge,
    /// No valid user or runtime credential was supplied.
    #[error("authentication required")]
    Unauthorized,
    /// The authenticated caller or origin is not permitted.
    #[error("access denied: {0}")]
    Forbidden(&'static str),
    /// A caller who may see the resource may not do this; the code names why (for example `not_member`, ADR-0014).
    #[error("not allowed: {0}")]
    Denied(&'static str),
    /// An invitation token is unknown, used, expired or revoked; one answer for all, so it reveals nothing.
    #[error("invitation is not valid")]
    InvitationInvalid,
    /// Token or user administration was attempted with a personal API token instead of a browser session.
    #[error("this operation requires a signed-in session")]
    SessionRequired,
    /// A stored screenshot the retention purge removed (ADR-0039 section 9); its event remains.
    #[error("the screenshot was removed after its retention period")]
    ScreenshotExpired,
    /// A workspace path is malformed, climbs out of the workspace or passes through a symbolic link.
    #[error("the workspace path is not valid")]
    PathInvalid,
    /// The file's type has no sandboxed view (`/files/view` serves only HTML pages); it can still be downloaded.
    #[error("this file type has no preview")]
    PreviewUnsupported,
    /// A spreadsheet expands past what a preview reads (its unzipped size, its entries, or the parse deadline).
    #[error("this spreadsheet is too large to preview")]
    PreviewTooLarge,
    /// A spreadsheet is damaged or is not really a spreadsheet.
    #[error("this spreadsheet is damaged")]
    PreviewDamaged,
    /// A spreadsheet is protected with a password.
    #[error("this spreadsheet is protected with a password")]
    PreviewProtected,
    /// An upload exceeds the per-file limit; nothing was stored.
    #[error("the upload exceeds its size limit")]
    UploadTooLarge,
    /// A message names an attachment that is not an uploaded file of the task's workspace.
    #[error("an attachment is not an uploaded file")]
    AttachmentMissing,
    /// A run has no workspace snapshot to compare with (it ran before changes were recorded, or on Firecracker).
    #[error("this run's changes were not recorded")]
    ChangesUnavailable,
    /// Workspace files live in a block image the gateway never reads (the microVM backend, ADR-0006); files, run changes
    /// and forks wait for the runtime file service.
    #[error("workspace files are not available on the microVM backend yet")]
    FilesUnavailable,
    /// No active account has that email; the same answer for unknown and inactive accounts.
    #[error("no account uses that email")]
    UserNotFound,
    /// A named thing this caller may address doesn't exist, under its own code (an agent team's `unknown_member`).
    #[error("{0}")]
    Missing(&'static str),
    /// The requested owned record does not exist.
    #[error("record not found")]
    NotFound,
    /// A stable conflict reason prevents this operation.
    #[error("operation conflicts with current state: {0}")]
    Conflict(&'static str),
    /// One of an agent team's handed-back files was refused (ADR-0034 point 6); `details: {path}` names it.
    #[error("{path}: {refusal}")]
    ForFile {
        /// The file, as the report names it.
        path: String,
        /// Why.
        refusal: FileRefusal,
    },
    /// The lead already has as many members as the effective cap allows, a done one among them until it is stopped
    /// (ADR-0034); `details: {limit, by}`. The message tells the lead how to free a seat.
    #[error(
        "the agent team already has {limit} helpers, the most allowed by {by}; a helper that is done still counts \
         until you stop it with stop_member, which frees its seat"
    )]
    MemberLimit {
        /// The effective cap.
        limit: u32,
        /// What set it: `max_members`, `slots` or `per_user`.
        by: &'static str,
    },
    /// The caller's agent team has ended (ADR-0034 point 10): team tools no longer work. For the lead, `details` is
    /// a read's answer (`messages`, `last_seq`, `more`) recorded for its call as a read is, so the end note is shown
    /// once by the read rule.
    #[error("the agent team has ended; its team tools no longer work")]
    TeamEnded(Option<Box<serde_json::Value>>),
    /// The administrator's acceptance check holds back the lead's `done` (ADR-0034 point 10, GW-I-198): the words say
    /// why and what to do; a check that ran and failed has `details: {exit_code, timed_out, output}`.
    #[error("{0}")]
    AcceptanceFailed(&'static str, Option<Box<serde_json::Value>>),
    /// A host key's name is already taken (ADR-0037 §1, Amendment 1): by the caller's own or a usable shared key, named
    /// by `taken_by` (`name_taken`); or, for `None`, by a member's own key, whom it never names
    /// (`name_taken_by_user`). Reported as `details` on the `name` field.
    #[error("host key name taken")]
    NameTaken {
        /// The key that has it, and whether it is shared; `None` when it is a member's own key.
        taken_by: Option<(uuid::Uuid, bool)>,
    },
    /// The container engine didn't answer a read of the images on its host (the built-in environments design).
    #[error("the container engine didn't answer")]
    EngineUnavailable,
    /// A previously accepted action used this key with a different canonical request body.
    #[error("idempotency key was already used with another request")]
    IdempotencyKeyReused,
    /// A bounded queue, rate window or concurrency budget is full.
    #[error("gateway capacity is temporarily unavailable")]
    Busy,
    /// A user's relay bucket requires a retry delay in seconds.
    #[error("relay request rate exceeded")]
    RateLimited(u64),
    /// A channel has had its day's messages (ADR-0014): a retry delay in seconds until the oldest of them ages out.
    #[error("the channel has reached its daily message limit")]
    ChannelRate(u64),
    /// The upstream model transport failed without exposing its URL or credentials.
    #[error("upstream model service is unavailable")]
    UpstreamUnavailable,
    /// A decision engine answered with something that isn't an answer, or refused the request the relay built (ADR-0013,
    /// the adapter amendment); the runtime records it as `invalid`.
    #[error("the decision engine's answer is not usable")]
    EngineInvalid,
    /// A workspace file changed while it was read in chunks (ADR-0030); reading it again may succeed.
    #[error("the file changed while it was read")]
    FileChanged,
    /// A sandbox runtime answered a workspace request outside its bounds (ADR-0030); the reply is not shown.
    #[error("the sandbox runtime sent an invalid reply")]
    UpstreamInvalid,
    /// `PostgreSQL` or migration state is unavailable.
    #[error("event or account store is unavailable")]
    StoreUnavailable,
    /// A secret could not be read, authenticated or persisted.
    #[error("secret store is unavailable")]
    SecretUnavailable,
    /// A key was read but its use could not be recorded, so it was not used (GW-I-147).
    #[error("The key could not be used: its use could not be recorded. Try again.")]
    KeyUseUnrecorded,
    /// Host storage could not complete an operation.
    #[error("host storage operation failed")]
    Io,
    /// An operation exceeded its documented bound.
    #[error("operation timed out")]
    Timeout,
    /// A named start-up step timed out; the gateway exits with it (GW-U-060).
    #[error("start-up step '{0}' timed out")]
    StartupTimeout(&'static str),
    /// The owner cancelled the operation during shutdown or replacement.
    #[error("gateway is restarting")]
    Cancelled,
    /// A typed wire invariant failed, without retaining the rejected payload.
    #[error("protocol rejected the frame: {0}")]
    Protocol(ErrorCode),
    /// An unsupported version was supplied; the supported set is explicit.
    #[error("{0}")]
    UnsupportedProtocol(tqa_types::UnsupportedProtocol),
    /// A sandbox lifecycle step failed; stage and cause are fixed, secret-free phrases.
    #[error("sandbox {stage} failed: {cause}")]
    Sandbox {
        /// The lifecycle step, for example `create` or `start`.
        stage: &'static str,
        /// The bounded failure class reported by the backend.
        cause: &'static str,
    },
    /// The requested environment template is not configured or not allowed.
    #[error("environment template is unknown or not allowed")]
    UnknownTemplate,
    /// The template exists but is not offered now: disabled, not yet verified, or refused by a check (ADR-0032).
    #[error("environment template is not available")]
    TemplateUnavailable,
    /// The template cannot be deleted: it is the default, a conversation binds it, or an MCP server lists it.
    #[error("the environment template is in use")]
    TemplateInUse(crate::settings::templates::InUse),
    /// The decision engine cannot be deleted: a shared or personal profile or an active conversation names it; disable
    /// it instead (ADR-0013 point 6). The body is the template's shape, with no default and no servers.
    #[error("the decision engine is in use")]
    EngineInUse(crate::settings::templates::InUse),
    /// The runtime's protocol revision is below the gateway's floor (ADR-0026); both revisions are reported.
    #[error("{0}")]
    RuntimeTooOld(tqa_types::RuntimeTooOld),
    /// The runtime process exited before it registered its link; only the exit code is kept.
    #[error("runtime exited before registration: code {code}")]
    RuntimeExited {
        /// The process exit code reported by the backend.
        code: i32,
    },
    /// The MCP proxy refused or could not complete a connector call (ADR-0031 decision 9).
    #[error("{}", .0.message())]
    Connector(ConnectorFailure),
}

/// Why a connector call through the MCP proxy failed; each has a fixed code and status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectorFailure {
    /// The connector is unknown, disabled, or not in the conversation's set (the same answer for all).
    NotAllowed,
    /// The connector requires a credential and neither the user nor the connector has one.
    CredentialMissing,
    /// The remote refused the calling person's own credential (401 or 403): wrong, expired or revoked.
    CredentialRejected,
    /// The remote answered with a redirect, which is never followed.
    Redirect,
    /// The remote could not be reached, refused, or answered badly.
    Unavailable,
    /// The remote's answer is larger than one message may be.
    AnswerTooLarge,
    /// The call exceeded the connector's timeout.
    Timeout,
    /// Too many calls to this connector from this conversation, or through the proxy, are in flight.
    Busy,
}

impl ConnectorFailure {
    /// The stable wire code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::NotAllowed => "connector_not_allowed",
            Self::CredentialMissing => "connector_credential_missing",
            Self::CredentialRejected => "connector_credential_rejected",
            Self::Redirect => "connector_redirect",
            Self::Unavailable => "connector_unavailable",
            Self::AnswerTooLarge => "connector_answer_too_large",
            Self::Timeout => "connector_timeout",
            Self::Busy => "connector_busy",
        }
    }

    const fn message(self) -> &'static str {
        match self {
            Self::NotAllowed => "this connector is not available to this conversation",
            Self::CredentialMissing => {
                "this connector needs a credential; add yours or ask an administrator"
            }
            Self::CredentialRejected => "the connector refused your own credential",
            Self::Redirect => "the connector answered with a redirect, which is not followed",
            Self::Unavailable => "the connector could not be reached",
            Self::AnswerTooLarge => "the connector's answer is too large",
            Self::Timeout => "the connector did not answer in time",
            Self::Busy => "too many calls to this connector are in flight; retry shortly",
        }
    }

    const fn status(self) -> StatusCode {
        match self {
            Self::NotAllowed => StatusCode::FORBIDDEN,
            Self::CredentialMissing | Self::CredentialRejected => StatusCode::FAILED_DEPENDENCY,
            Self::Timeout => StatusCode::GATEWAY_TIMEOUT,
            Self::Busy => StatusCode::TOO_MANY_REQUESTS,
            Self::Redirect | Self::Unavailable | Self::AnswerTooLarge => StatusCode::BAD_GATEWAY,
        }
    }
}

impl GatewayError {
    /// Whether the configuration is at fault, by a named invariant or a field the TOML parse refused: a start-up or a
    /// `check` exits 2 on one.
    #[must_use]
    pub const fn is_configuration(&self) -> bool {
        matches!(self, Self::Configuration(_) | Self::ConfigurationField(_))
    }

    /// Returns the stable error code used in HTTP responses.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::ForFile { refusal, .. } => refusal.code(),
            Self::Configuration(_) | Self::ConfigurationField(_) => "configuration",
            Self::BadRequest(_) | Self::Invalid { .. } | Self::InvalidField { .. } => {
                "invalid_request"
            }
            Self::UnknownReference { .. } => "unknown_reference",
            Self::ProfileUnusable { .. } => "profile_unusable",
            Self::SetByProfile { .. } => "set_by_profile",
            Self::BodyTooLarge => "body_too_large",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden(_) => "forbidden",
            Self::InvitationInvalid => "invitation_invalid",
            Self::SessionRequired => "session_required",
            Self::ScreenshotExpired => "screenshot_expired",
            Self::PathInvalid => "path_invalid",
            Self::PreviewUnsupported => "preview_unsupported",
            Self::PreviewTooLarge => "preview_too_large",
            Self::PreviewDamaged => "preview_damaged",
            Self::PreviewProtected => "preview_protected",
            Self::UploadTooLarge => "upload_too_large",
            Self::AttachmentMissing => "attachment_missing",
            Self::ChangesUnavailable => "changes_unavailable",
            Self::FilesUnavailable => "files_unavailable",
            Self::NotFound => "not_found",
            Self::UserNotFound => "user_not_found",
            Self::Missing(code) | Self::Denied(code) | Self::Conflict(code) => code,
            Self::MemberLimit { .. } => "member_limit",
            Self::TeamEnded(_) => "team_ended",
            Self::AcceptanceFailed(..) => "acceptance_failed",
            Self::NameTaken { taken_by: Some(_) } => "name_taken",
            Self::NameTaken { taken_by: None } => "name_taken_by_user",
            Self::IdempotencyKeyReused => "idempotency_key_reused",
            Self::Busy => "busy",
            Self::RateLimited(_) => "rate_limited",
            Self::ChannelRate(_) => "channel_rate",
            Self::UpstreamUnavailable => "upstream_unavailable",
            Self::UpstreamInvalid => "upstream_invalid",
            Self::EngineInvalid => "decision_engine_invalid",
            Self::FileChanged => "file_changed",
            Self::StoreUnavailable => "store_unavailable",
            Self::EngineUnavailable => "engine_unavailable",
            Self::SecretUnavailable => "secret_unavailable",
            Self::KeyUseUnrecorded => "key_use_unrecorded",
            Self::Io => "io",
            Self::Timeout | Self::StartupTimeout(_) => "timeout",
            Self::Cancelled => "restarting",
            Self::Protocol(code) => code.as_str(),
            Self::UnsupportedProtocol(_) => "unsupported_protocol",
            Self::RuntimeTooOld(_) => "runtime_too_old",
            Self::Sandbox { .. } => "sandbox_failed",
            Self::RuntimeExited { .. } => "runtime_start_failed",
            Self::UnknownTemplate => "unknown_template",
            Self::Connector(failure) => failure.code(),
            Self::TemplateUnavailable => "template_unavailable",
            Self::TemplateInUse(_) => "template_in_use",
            Self::EngineInUse(_) => "decision_engine_in_use",
        }
    }

    /// Reports whether an unchanged request can succeed after a transient failure.
    #[must_use]
    pub const fn retryable(&self) -> bool {
        match self {
            Self::ForFile { refusal, .. } => matches!(refusal, FileRefusal::FileChanged),
            Self::RateLimited(_)
            | Self::ChannelRate(_)
            | Self::UpstreamUnavailable
            | Self::Busy
            | Self::StoreUnavailable
            | Self::EngineUnavailable
            | Self::SecretUnavailable
            | Self::KeyUseUnrecorded
            | Self::Timeout
            | Self::StartupTimeout(_)
            | Self::Cancelled
            | Self::FileChanged
            | Self::Sandbox { .. }
            | Self::RuntimeExited { .. } => true,
            Self::Connector(failure) => matches!(
                failure,
                ConnectorFailure::Unavailable | ConnectorFailure::Timeout | ConnectorFailure::Busy
            ),
            Self::Protocol(code) => code.retryable(),
            _ => false,
        }
    }

    pub(crate) fn status(&self) -> StatusCode {
        match self {
            Self::ForFile { refusal, .. } => refusal.error().status(),
            Self::BadRequest(_)
            | Self::Invalid { .. }
            | Self::InvalidField { .. }
            | Self::Protocol(_)
            | Self::UnsupportedProtocol(_)
            | Self::InvitationInvalid
            | Self::PathInvalid
            | Self::AttachmentMissing => StatusCode::BAD_REQUEST,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::BodyTooLarge | Self::UploadTooLarge | Self::PreviewTooLarge => {
                StatusCode::PAYLOAD_TOO_LARGE
            }
            Self::PreviewUnsupported => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::Forbidden(_) | Self::Denied(_) | Self::SessionRequired => StatusCode::FORBIDDEN,
            Self::ScreenshotExpired => StatusCode::GONE,
            Self::NotFound | Self::ChangesUnavailable | Self::UserNotFound | Self::Missing(_) => {
                StatusCode::NOT_FOUND
            }
            Self::Conflict(_)
            | Self::MemberLimit { .. }
            | Self::TeamEnded(_)
            | Self::AcceptanceFailed(..)
            | Self::NameTaken { .. }
            | Self::FileChanged
            | Self::FilesUnavailable
            | Self::TemplateUnavailable
            | Self::TemplateInUse(_)
            | Self::EngineInUse(_)
            | Self::ProfileUnusable { .. } => StatusCode::CONFLICT,
            Self::IdempotencyKeyReused
            | Self::UnknownTemplate
            | Self::UnknownReference { .. }
            | Self::SetByProfile { .. }
            | Self::PreviewDamaged
            | Self::PreviewProtected => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Busy | Self::RateLimited(_) | Self::ChannelRate(_) => {
                StatusCode::TOO_MANY_REQUESTS
            }
            Self::UpstreamUnavailable | Self::UpstreamInvalid | Self::EngineInvalid => {
                StatusCode::BAD_GATEWAY
            }
            Self::StoreUnavailable
            | Self::EngineUnavailable
            | Self::SecretUnavailable
            | Self::KeyUseUnrecorded
            | Self::Cancelled
            | Self::Sandbox { .. }
            | Self::RuntimeExited { .. } => StatusCode::SERVICE_UNAVAILABLE,
            Self::Timeout | Self::StartupTimeout(_) => StatusCode::GATEWAY_TIMEOUT,
            Self::RuntimeTooOld(_) => StatusCode::UPGRADE_REQUIRED,
            Self::Configuration(_) | Self::ConfigurationField(_) | Self::Io => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
            Self::Connector(failure) => failure.status(),
        }
    }

    pub(crate) fn frame(&self) -> ServerFrame {
        let code = match self {
            Self::Protocol(code) => *code,
            Self::UnsupportedProtocol(_) | Self::RuntimeTooOld(_) => ErrorCode::UnsupportedProtocol,
            Self::Cancelled => ErrorCode::Restarting,
            Self::Busy | Self::RateLimited(_) | Self::ChannelRate(_) => ErrorCode::Busy,
            Self::UpstreamUnavailable
            | Self::UpstreamInvalid
            | Self::EngineInvalid
            | Self::EngineUnavailable
            | Self::Io => ErrorCode::Io,
            Self::StoreUnavailable
            | Self::SecretUnavailable
            | Self::KeyUseUnrecorded
            | Self::Timeout
            | Self::StartupTimeout(_) => ErrorCode::StoreUnavailable,
            Self::BadRequest(_)
            | Self::Invalid { .. }
            | Self::InvalidField { .. }
            | Self::UnknownReference { .. }
            | Self::ProfileUnusable { .. }
            | Self::SetByProfile { .. }
            | Self::BodyTooLarge
            | Self::Forbidden(_)
            | Self::Denied(_)
            | Self::InvitationInvalid
            | Self::SessionRequired
            | Self::ScreenshotExpired
            | Self::PathInvalid
            | Self::PreviewUnsupported
            | Self::PreviewTooLarge
            | Self::PreviewDamaged
            | Self::PreviewProtected
            | Self::UploadTooLarge
            | Self::AttachmentMissing
            | Self::ChangesUnavailable
            | Self::FilesUnavailable
            | Self::UserNotFound
            | Self::Missing(_)
            | Self::Unauthorized
            | Self::NotFound
            | Self::Conflict(_)
            | Self::MemberLimit { .. }
            | Self::TeamEnded(_)
            | Self::AcceptanceFailed(..)
            | Self::NameTaken { .. }
            | Self::FileChanged
            | Self::IdempotencyKeyReused
            | Self::UnknownTemplate
            | Self::Connector(_)
            | Self::TemplateUnavailable
            | Self::TemplateInUse(_)
            | Self::EngineInUse(_)
            | Self::ForFile { .. } => ErrorCode::InvalidFrame,
            Self::Configuration(_) | Self::ConfigurationField(_) => ErrorCode::Internal,
            Self::Sandbox { .. } => ErrorCode::SandboxFailed,
            Self::RuntimeExited { .. } => ErrorCode::RuntimeStartFailed,
        };
        ServerFrame::Error {
            seq: None,
            code,
            message: self.to_string(),
            retryable: self.retryable(),
            target: None,
        }
    }
}

/// `Retry-After` for transient refusals: a busy sandbox or an unavailable store. Callers such as the runtime's
/// model client treat 503 with this header as retryable, never as an authentication failure.
const RETRY_AFTER_SECS: u64 = 1;

impl IntoResponse for GatewayError {
    fn into_response(self) -> Response {
        let mut error = serde_json::json!({
            "code": self.code(), "message": self.to_string(), "retryable": self.retryable()
        });
        if let Self::UnsupportedProtocol(version) = &self {
            error["supported"] = serde_json::json!(version.supported);
        }
        if let Self::RuntimeTooOld(too_old) = &self {
            error["needs"] = serde_json::json!(too_old.needs);
            error["has"] = serde_json::json!(too_old.has);
        }
        if let Self::InvalidField { field, reason } = &self {
            error["details"] = serde_json::json!([{"field": field, "reason": reason.as_str()}]);
        }
        if let Self::Invalid { field, reason } = &self {
            error["details"] = serde_json::json!([{"field": field, "reason": reason.as_str()}]);
        }
        if let Self::UnknownReference { field } = &self {
            let reason = InvalidReason::UnknownReference.as_str();
            error["details"] = serde_json::json!([{"field": field, "reason": reason}]);
        }
        if let Self::ProfileUnusable { field } = &self {
            let reason = InvalidReason::UnknownReference.as_str();
            error["details"] = serde_json::json!([{"field": field, "reason": reason}]);
        }
        if let Self::MemberLimit { limit, by } = &self {
            error["details"] = serde_json::json!({"limit": limit, "by": by});
        }
        if let Self::ForFile { path, .. } = &self {
            error["details"] = serde_json::json!({ "path": path });
        }
        if let Self::TeamEnded(Some(answer)) | Self::AcceptanceFailed(_, Some(answer)) = &self {
            error["details"] = (**answer).clone();
        }
        if let Self::NameTaken { taken_by } = &self {
            let mut detail = serde_json::json!({"field": "name", "reason": self.code()});
            if let Some((id, shared)) = taken_by {
                detail["taken_by"] = serde_json::json!({"id": id, "shared": shared});
            }
            error["details"] = serde_json::json!([detail]);
        }
        if let Self::SetByProfile { field } = &self {
            let reason = InvalidReason::SetByProfile.as_str();
            error["details"] = serde_json::json!([{"field": field, "reason": reason}]);
        }
        if let Self::TemplateInUse(in_use) | Self::EngineInUse(in_use) = &self {
            error["in_use"] = serde_json::json!({
                "default": in_use.default,
                "conversations": in_use.conversations,
                "mcp_servers": in_use.mcp_servers,
                "profiles": in_use.profiles,
                "personal_profiles": in_use.personal_profiles,
            });
        }
        let mut response =
            (self.status(), Json(serde_json::json!({"error": error}))).into_response();
        response.extensions_mut().insert(Refusal(self.code()));
        if let Self::RateLimited(seconds) | Self::ChannelRate(seconds) = self {
            response.headers_mut().insert(
                axum::http::header::RETRY_AFTER,
                axum::http::HeaderValue::from(seconds),
            );
        } else if matches!(self, Self::Busy | Self::StoreUnavailable) {
            response.headers_mut().insert(
                axum::http::header::RETRY_AFTER,
                axum::http::HeaderValue::from(RETRY_AFTER_SECS),
            );
        }
        response
    }
}

/// Marks a response as the gateway's own refusal, with its code.
#[derive(Clone, Copy)]
struct Refusal(&'static str);

/// Sets [`tqa_types::GATEWAY_ERROR_HEADER`] (equal to `error.code`) on a refusal the gateway made itself on the relays
/// and the MCP proxy, so the runtime can tell it from a provider's error, and removes it from any other response, such
/// as an upstream answer being forwarded (the gateway-refusal contract).
pub(crate) fn name_refusal(response: &mut Response) {
    response
        .headers_mut()
        .remove(tqa_types::GATEWAY_ERROR_HEADER);
    let code = response.extensions().get::<Refusal>().copied();
    if let Some(value) = code.and_then(|Refusal(code)| axum::http::HeaderValue::from_str(code).ok())
    {
        response
            .headers_mut()
            .insert(tqa_types::GATEWAY_ERROR_HEADER, value);
    }
}

/// The closed list of field-refusal reasons (090 contract amendment 6). Clients map each to their own text, so a
/// code is added here, in `openapi.json` and in the clients together, and never renamed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvalidReason {
    /// A query or body parameter is unknown or repeated.
    ParameterInvalid,
    /// A date is not a `YYYY-MM-DD` calendar date.
    DateInvalid,
    /// `from` is after `to`.
    RangeReversed,
    /// The range spans more days than allowed.
    RangeTooLong,
    /// A date is later than tomorrow (UTC).
    DateFuture,
    /// The grouping is not one of the documented values.
    GroupInvalid,
    /// A secret name is empty, too long or has characters outside `[A-Za-z0-9_-]`.
    NameInvalid,
    /// A secret name uses a prefix reserved for per-user credentials.
    ReservedPrefix,
    /// A secret value is empty.
    ValueEmpty,
    /// A secret value is longer than the store allows.
    ValueTooLong,
    /// A secret value contains a NUL character.
    ValueNul,
    /// A version is missing where one is required, or not a positive integer.
    VersionInvalid,
    /// The body is not the documented JSON object.
    BodyInvalid,
    /// A list cursor is not one this gateway issued.
    CursorInvalid,
    /// An egress host is not an exact name or address.
    HostInvalid,
    /// An egress wildcard is not `*.` followed by a domain.
    WildcardInvalid,
    /// `internal` set on an egress rule that isn't one exact host name (ADR-0035 amendment).
    InternalNameOnly,
    /// Egress ports are missing, zero, repeated or more than 16.
    PortsInvalid,
    /// A route name is empty, longer than 128 bytes, or has control characters.
    RouteNameInvalid,
    /// A route's model is empty.
    ModelEmpty,
    /// A route's base URL is not plain HTTP(S) without credentials, query or fragment.
    BaseUrlInvalid,
    /// An authenticated route names no secret reference.
    KeyRefRequired,
    /// A route's key reference is not `secrets/<name>` (SEC-033); the value is never echoed.
    KeyRefNotReference,
    /// A route's key reference names a user's own credential (FR-19, FR-24).
    KeyRefReserved,
    /// A route's credential header name is not a valid HTTP header name.
    AuthHeaderInvalid,
    /// A route's credential query parameter name is invalid.
    AuthQueryInvalid,
    /// A route's reasoning field or default is invalid.
    ReasoningInvalid,
    /// A route's thinking budgets do not match its reasoning values.
    BudgetInvalid,
    /// The route's protocol requires `max_output`.
    MaxOutputRequired,
    /// A route limit is zero, or `max_output` exceeds `context_window`.
    LimitOutOfRange,
    /// An `extra_body` field would override a field the harness sets.
    ExtraBodyReserved,
    /// An extra header's name or value is not valid HTTP.
    ExtraHeaderInvalid,
    /// An extra header would set a header the protocol pins.
    ExtraHeaderReserved,
    /// An extra header would carry a credential.
    ExtraHeaderCredential,
    /// A named adapter quirk is not implemented.
    QuirkUnsupported,
    /// A route's `strict` is not `auto`, `on` or `off`.
    StrictInvalid,
    /// A route's `schema_keywords` is not `full` or `basic`.
    SchemaKeywordsInvalid,
    /// A route's `instructions_role` is not a value its protocol accepts.
    InstructionsRoleInvalid,
    /// A route's `first_response_secs` is outside 5 to 900 seconds.
    FirstResponseInvalid,
    /// A fallback list is too long, names the route itself or an unknown or repeated route, or forms a cycle.
    FallbackInvalid,
    /// A route is refused for a reason this gateway does not name yet.
    RouteInvalid,
    /// A display name is empty, longer than 80 characters, or has control characters or surrounding spaces.
    DisplayNameInvalid,
    /// A limit name is not one of the overridable limits.
    LimitUnknown,
    /// `admission.per_user` would exceed `admission.slots`.
    LimitOrder,
    /// A connector's auth does not fit together: a header without the header scheme, or a credential with no scheme.
    AuthInvalid,
    /// A secret reference is not a valid secret name.
    SecretRefInvalid,
    /// A connector's shared credential names a user's own credential (FR-19, FR-24).
    SecretRefReserved,
    /// A rotation reminder is not 1 through 3650 days, or `null`.
    RotateAfterInvalid,
    /// A secret's consumer list is not route or connector entries with valid names, or has too many.
    ConsumersInvalid,
    /// A tool filter is not exactly one bounded list of distinct names.
    ToolFilterInvalid,
    /// The field may only be set in the configuration file, never through the API.
    FileOnly,
    /// A template image is a tag or carries credentials instead of a digest pin (ADR-0032).
    ImageNotPinned,
    /// Template capabilities repeat or are more than five.
    CapabilitiesInvalid,
    /// A list of names repeats a name, is longer than allowed, or holds a name that is not well formed (ADR-0012).
    ListInvalid,
    /// A name refers to a route, template, tool, server or connector that does not exist (ADR-0012).
    UnknownReference,
    /// The conversation's profile sets the field; a request may not change or widen it (ADR-0012).
    SetByProfile,
    /// Only a shared profile may set the field.
    SharedOnly,
    /// A channel's decision rule is not `human` or `majority` (ADR-0014).
    RuleInvalid,
    /// A channel message's field is out of bounds or not allowed; the field is its path, e.g. `body.title`
    /// (ADR-0014).
    MessageInvalid,
    /// The current password given to change it is not the account's password.
    PasswordIncorrect,
    /// A new password breaks the rules the first administrator's password met: at least 12 characters, not the
    /// email, not a common password.
    PasswordWeak,
    /// The new password is the current one.
    PasswordUnchanged,
    /// A description is empty, longer than 160 characters, not one line, or has surrounding spaces (GW-I-132).
    DescriptionInvalid,
    /// The name is reserved: `new` is the address of the Add page, so no environment may take it (GW-U-093); or a host
    /// key's name is one the task's environment relies on (ADR-0037 Amendment 1, GW-U-103).
    NameReserved,
    /// A host key's address is not an `https://` address (GW-U-102).
    AddressNotHttps,
    /// A host key's address has credentials, a query, a fragment, or a path over 512 bytes (GW-U-102).
    AddressNotPlain,
    /// A `basic` host key needs its user name here (ADR-0037 §5).
    UserNameRequired,
    /// A `header` host key needs the header's name (ADR-0037 §5).
    HeaderNameRequired,
    /// This way of sending a key is not available yet (the `header` scheme, ADR-0037's second slice).
    SchemeNotSupported,
    /// A value holds a character no header can carry (a line break).
    ValueInvalid,
    /// A host key's name is the caller's own or a usable shared key's (the `409 name_taken` detail).
    NameTaken,
    /// A shared key's name is a member's own key's, whom it never names (the `409 name_taken_by_user` detail).
    NameTakenByUser,
    /// The field belonged to a feature this release removed (ADR-0037 §7: `git_url`, `repositories`).
    Removed,
    /// A secret's name, or a reference's, looks like a key rather than a name (SEC-033, `tqa_types::is_key_shaped_name`):
    /// most likely a key pasted where its name belongs. The value is never echoed.
    NameKeyShaped,
}

impl InvalidReason {
    /// Every reason, in documentation order.
    pub const ALL: [Self; 74] = [
        Self::ParameterInvalid,
        Self::DateInvalid,
        Self::RangeReversed,
        Self::RangeTooLong,
        Self::DateFuture,
        Self::GroupInvalid,
        Self::NameInvalid,
        Self::ReservedPrefix,
        Self::ValueEmpty,
        Self::ValueTooLong,
        Self::ValueNul,
        Self::VersionInvalid,
        Self::BodyInvalid,
        Self::CursorInvalid,
        Self::HostInvalid,
        Self::WildcardInvalid,
        Self::InternalNameOnly,
        Self::PortsInvalid,
        Self::RouteNameInvalid,
        Self::ModelEmpty,
        Self::BaseUrlInvalid,
        Self::KeyRefRequired,
        Self::KeyRefNotReference,
        Self::KeyRefReserved,
        Self::AuthHeaderInvalid,
        Self::AuthQueryInvalid,
        Self::ReasoningInvalid,
        Self::BudgetInvalid,
        Self::MaxOutputRequired,
        Self::LimitOutOfRange,
        Self::ExtraBodyReserved,
        Self::ExtraHeaderInvalid,
        Self::ExtraHeaderReserved,
        Self::ExtraHeaderCredential,
        Self::QuirkUnsupported,
        Self::StrictInvalid,
        Self::SchemaKeywordsInvalid,
        Self::InstructionsRoleInvalid,
        Self::FirstResponseInvalid,
        Self::FallbackInvalid,
        Self::RouteInvalid,
        Self::DisplayNameInvalid,
        Self::LimitUnknown,
        Self::LimitOrder,
        Self::AuthInvalid,
        Self::SecretRefInvalid,
        Self::SecretRefReserved,
        Self::RotateAfterInvalid,
        Self::ConsumersInvalid,
        Self::ToolFilterInvalid,
        Self::FileOnly,
        Self::ImageNotPinned,
        Self::CapabilitiesInvalid,
        Self::ListInvalid,
        Self::UnknownReference,
        Self::SetByProfile,
        Self::SharedOnly,
        Self::RuleInvalid,
        Self::MessageInvalid,
        Self::PasswordIncorrect,
        Self::PasswordWeak,
        Self::PasswordUnchanged,
        Self::DescriptionInvalid,
        Self::NameReserved,
        Self::AddressNotHttps,
        Self::AddressNotPlain,
        Self::UserNameRequired,
        Self::HeaderNameRequired,
        Self::SchemeNotSupported,
        Self::ValueInvalid,
        Self::NameTaken,
        Self::NameTakenByUser,
        Self::Removed,
        Self::NameKeyShaped,
    ];

    /// The wire code.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ParameterInvalid => "parameter_invalid",
            Self::DateInvalid => "date_invalid",
            Self::RangeReversed => "range_reversed",
            Self::RangeTooLong => "range_too_long",
            Self::DateFuture => "date_future",
            Self::GroupInvalid => "group_invalid",
            Self::NameInvalid => "name_invalid",
            Self::ReservedPrefix => "reserved_prefix",
            Self::ValueEmpty => "value_empty",
            Self::ValueTooLong => "value_too_long",
            Self::ValueNul => "value_nul",
            Self::VersionInvalid => "version_invalid",
            Self::BodyInvalid => "body_invalid",
            Self::CursorInvalid => "cursor_invalid",
            Self::HostInvalid => "host_invalid",
            Self::WildcardInvalid => "wildcard_invalid",
            Self::InternalNameOnly => "internal_name_only",
            Self::PortsInvalid => "ports_invalid",
            Self::RouteNameInvalid => "route_name_invalid",
            Self::ModelEmpty => "model_empty",
            Self::BaseUrlInvalid => "base_url_invalid",
            Self::KeyRefRequired => "key_ref_required",
            Self::KeyRefNotReference => "key_ref_not_reference",
            Self::KeyRefReserved => "key_ref_reserved",
            Self::AuthHeaderInvalid => "auth_header_invalid",
            Self::AuthQueryInvalid => "auth_query_invalid",
            Self::ReasoningInvalid => "reasoning_invalid",
            Self::BudgetInvalid => "budget_invalid",
            Self::MaxOutputRequired => "max_output_required",
            Self::LimitOutOfRange => "limit_out_of_range",
            Self::ExtraBodyReserved => "extra_body_reserved",
            Self::ExtraHeaderInvalid => "extra_header_invalid",
            Self::ExtraHeaderReserved => "extra_header_reserved",
            Self::ExtraHeaderCredential => "extra_header_credential",
            Self::QuirkUnsupported => "quirk_unsupported",
            Self::StrictInvalid => "strict_invalid",
            Self::SchemaKeywordsInvalid => "schema_keywords_invalid",
            Self::InstructionsRoleInvalid => "instructions_role_invalid",
            Self::FirstResponseInvalid => "first_response_invalid",
            Self::FallbackInvalid => "fallback_invalid",
            Self::RouteInvalid => "route_invalid",
            Self::DisplayNameInvalid => "display_name_invalid",
            Self::LimitUnknown => "limit_unknown",
            Self::LimitOrder => "limit_order",
            Self::AuthInvalid => "auth_invalid",
            Self::SecretRefInvalid => "secret_ref_invalid",
            Self::SecretRefReserved => "secret_ref_reserved",
            Self::RotateAfterInvalid => "rotate_after_invalid",
            Self::ConsumersInvalid => "consumers_invalid",
            Self::ToolFilterInvalid => "tool_filter_invalid",
            Self::FileOnly => "file_only",
            Self::ImageNotPinned => "image_not_pinned",
            Self::CapabilitiesInvalid => "capabilities_invalid",
            Self::ListInvalid => "list_invalid",
            Self::UnknownReference => "unknown_reference",
            Self::SetByProfile => "set_by_profile",
            Self::SharedOnly => "shared_only",
            Self::RuleInvalid => "rule_invalid",
            Self::MessageInvalid => "message_invalid",
            Self::PasswordIncorrect => "password_incorrect",
            Self::PasswordWeak => "password_weak",
            Self::PasswordUnchanged => "password_unchanged",
            Self::DescriptionInvalid => "description_invalid",
            Self::NameReserved => "name_reserved",
            Self::AddressNotHttps => "address_not_https",
            Self::AddressNotPlain => "address_not_plain",
            Self::UserNameRequired => "user_name_required",
            Self::HeaderNameRequired => "header_name_required",
            Self::SchemeNotSupported => "scheme_not_supported",
            Self::ValueInvalid => "value_invalid",
            Self::NameTaken => "name_taken",
            Self::NameTakenByUser => "name_taken_by_user",
            Self::Removed => "removed",
            Self::NameKeyShaped => "name_key_shaped",
        }
    }
}

/// [`GatewayError::Invalid`]'s message: the field and its reason, or for a removed Git field (ADR-0037 §7) what to do.
fn invalid_message(field: &str, reason: InvalidReason) -> String {
    match reason {
        InvalidReason::Removed => format!(
            "invalid request: {field} was removed in this release: clone with plain git and a key under Keys"
        ),
        _ => format!("invalid request: {field} ({reason})"),
    }
}

impl std::fmt::Display for InvalidReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl From<sqlx::Error> for GatewayError {
    fn from(error: sqlx::Error) -> Self {
        crate::db::report(&error);
        Self::StoreUnavailable
    }
}

impl From<std::io::Error> for GatewayError {
    fn from(_: std::io::Error) -> Self {
        Self::Io
    }
}

impl From<serde_json::Error> for GatewayError {
    fn from(_: serde_json::Error) -> Self {
        Self::Protocol(ErrorCode::Internal)
    }
}

impl From<tqa_types::SeqError> for GatewayError {
    fn from(error: tqa_types::SeqError) -> Self {
        Self::Protocol(error.code())
    }
}
