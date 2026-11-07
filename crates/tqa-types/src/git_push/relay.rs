//! Versioned, credential-free control messages for the gateway's Git relay.
use super::{GitPushTarget, invalid};
use crate::{ActionHash, FrameError, PermissionContext};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

/// Fixed control and repository route prefix on the trusted internal proxy listener.
pub const GIT_PUSH_OPERATIONS_PATH: &str = "/_tqa/git/v1/operations";
/// Maximum uncompressed control body and command-header length.
pub const MAX_GIT_PUSH_CONTROL_BYTES: usize = 16 * 1024;
/// Maximum pack payload accepted by the relay, independently of process output.
pub const MAX_GIT_PUSH_PACK_BYTES: usize = 64 * 1024 * 1024;
/// An operation's authorization may start it within at most two minutes.
pub const MAX_GIT_PUSH_START_MS: u64 = 120_000;

/// The exact call identity submitted to the gateway, which binds one operation to it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GitPushOperationRequest {
    /// SHA-256 of the canonical call envelope, recomputed by the gateway.
    pub action_hash: ActionHash,
    /// Original execution identity, which the operation is bound to.
    pub context: PermissionContext,
    /// The original call's arguments, never an upstream override or secret reference.
    pub arguments: GitPushTarget,
}

/// An opaque operation name; the source binding remains mandatory.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct GitPushOperationId(
    #[serde(deserialize_with = "operation_id")]
    #[schemars(length(min = 32, max = 32), regex(pattern = "^[0-9a-f]{32}$"))]
    String,
);
fn operation_id<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    GitPushOperationId::new(String::deserialize(deserializer)?)
        .map(|operation| operation.0)
        .map_err(D::Error::custom)
}
impl GitPushOperationId {
    /// Parses exactly 32 lowercase hexadecimal characters without interpreting authority.
    ///
    /// # Errors
    /// Rejects malformed operation names before they can enter a URL path.
    pub fn new(value: String) -> Result<Self, FrameError> {
        if value.len() != 32
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(invalid(
                "operation_id",
                "expected 32 lowercase hexadecimal characters",
            ));
        }
        Ok(Self(value))
    }
    /// Borrows the path-safe opaque operation name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for GitPushOperationId {
    type Error = FrameError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<GitPushOperationId> for String {
    fn from(value: GitPushOperationId) -> Self {
        value.0
    }
}

/// Discovery either authorizes one new-ref upload or confirms an identical existing ref.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum GitPushAuthorization {
    /// A single unused operation; the runtime derives its URL from the trusted proxy origin.
    Ready {
        /// Opaque operation ID, not a provider credential.
        operation_id: GitPushOperationId,
        /// Remaining start lifetime; reading it never extends the gateway's deadline.
        #[serde(deserialize_with = "lifetime")]
        #[schemars(range(min = 1, max = 120_000))]
        expires_in_ms: u64,
    },
    /// Discovery confirmed the requested commit; no Git upload is needed.
    AlreadyAtCommit {},
}
fn lifetime<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    let value = u64::deserialize(deserializer)?;
    if !(1..=MAX_GIT_PUSH_START_MS).contains(&value) {
        return Err(D::Error::custom(
            "expected start lifetime from 1 through 120000 milliseconds",
        ));
    }
    Ok(value)
}

/// Durable relay state; a successful Git exit alone is never a completion receipt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum GitPushReceipt {
    /// No upload has started.
    Ready {},
    /// The sole upload was consumed and is still in progress.
    Uploading {},
    /// The upstream acknowledged the requested ref and commit.
    Pushed {},
    /// The upstream outcome is not known; automatic retry is forbidden.
    Unknown {},
    /// A definitive refusal, without upstream diagnostics.
    Rejected {
        /// Fixed error vocabulary shared with control responses.
        code: GitPushFailure,
    },
}

/// A content-free error response; unknown fields and codes fail parsing closed.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GitPushRelayError {
    /// A stable code, never an upstream message or token.
    pub code: GitPushFailure,
}

/// Fixed push failures suitable for model-visible output, without remote diagnostics.
#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, thiserror::Error,
)]
pub enum GitPushFailure {
    /// The local repository no longer matches the prepared tuple.
    #[serde(rename = "git_push_snapshot_changed")]
    #[error("The repository changed. Push again from its current state.")]
    SnapshotChanged,
    /// Credential entry belongs in settings, not chat or tool arguments.
    #[serde(rename = "git_push_credential_required")]
    #[error("Add a Git credential in settings, then try again.")]
    CredentialRequired,
    /// The server did not accept the gateway-held credential.
    #[serde(rename = "git_push_authentication")]
    #[error("The Git server rejected this credential. Update it in settings.")]
    Authentication,
    /// The remote definitively rejected the update.
    #[serde(rename = "git_push_rejected")]
    #[error("The Git server refused this push.")]
    Rejected,
    /// Some upload bytes may have reached the server without a confirmed outcome.
    #[serde(rename = "git_push_outcome_unknown")]
    #[error("Push outcome is unknown. Check the remote before trying again.")]
    OutcomeUnknown,
    /// The one-use operation cannot start after its deadline.
    #[serde(rename = "git_push_expired")]
    #[error("The push authorization expired. Push again.")]
    Expired,
    /// Malformed input was refused before forwarding.
    #[serde(rename = "invalid_args")]
    #[error("Invalid push request.")]
    InvalidArgs,
    /// Runtime credential or source binding failed.
    #[serde(rename = "proxy_authentication")]
    #[error("Runtime proxy authentication failed.")]
    ProxyAuthentication,
    /// The gateway did not accept a matching one-use authorization.
    #[serde(rename = "denied")]
    #[error("This push is not authorized.")]
    Denied,
    /// The destination was refused by the network allow-list.
    #[serde(rename = "egress_denied")]
    #[error("The network allow-list does not allow this Git server.")]
    EgressDenied,
    /// A protocol or transfer bound was exceeded.
    #[serde(rename = "output_too_large")]
    #[error("The push exceeded its byte limit.")]
    OutputTooLarge,
    /// A deadline elapsed before a possible upload.
    #[serde(rename = "timeout")]
    #[error("The push deadline elapsed.")]
    Timeout,
    /// The push's key was not used, because its use could not be recorded (D4); retryable.
    #[serde(rename = "key_use_unrecorded")]
    #[error("The push didn't run: the key's use couldn't be recorded. Try again in a moment.")]
    KeyUseUnrecorded,
}
impl GitPushFailure {
    /// Returns the stable wire/tool error code without serializing a diagnostic.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::SnapshotChanged => "git_push_snapshot_changed",
            Self::CredentialRequired => "git_push_credential_required",
            Self::Authentication => "git_push_authentication",
            Self::Rejected => "git_push_rejected",
            Self::OutcomeUnknown => "git_push_outcome_unknown",
            Self::Expired => "git_push_expired",
            Self::InvalidArgs => "invalid_args",
            Self::ProxyAuthentication => "proxy_authentication",
            Self::Denied => "denied",
            Self::EgressDenied => "egress_denied",
            Self::OutputTooLarge => "output_too_large",
            Self::Timeout => "timeout",
            Self::KeyUseUnrecorded => "key_use_unrecorded",
        }
    }
}
