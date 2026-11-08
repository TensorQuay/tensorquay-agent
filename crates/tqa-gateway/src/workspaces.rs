//! Workspaces as owned entities (FR-30, ARCHITECTURE §4.6). A conversation binds to one workspace for
//! its whole life; the sandbox mounts that workspace's directory. The owner may share it `read` (see, read files,
//! fork) or `write` (also bind one's own conversations and run on it). Administrators see metadata only.
mod copy;
mod endpoints;
pub(crate) mod fork;
mod image_copy;
mod listing;
pub(crate) mod share_rate;
mod upkeep;

pub(crate) use endpoints::{
    create, delete, get, list, share, share_by_email, shares, unshare, update,
};
pub(crate) use fork::fork;
pub(crate) use upkeep::{charged_within, measure_later, recover, retention, start as start_upkeep};

use crate::GatewayError;
use sqlx::PgConnection;
use uuid::Uuid;

/// What the caller may do with a workspace.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Access {
    Owner,
    Write,
    Read,
}

impl Access {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Write => "write",
            Self::Read => "read",
        }
    }

    const fn may_bind(self) -> bool {
        matches!(self, Self::Owner | Self::Write)
    }
}

/// The caller's access to a live workspace.
///
/// # Errors
/// `404` for an unknown or deleted workspace; `403` without ownership or a share; a store failure.
pub(crate) async fn access(
    connection: &mut PgConnection,
    user: Uuid,
    workspace: Uuid,
) -> Result<Access, GatewayError> {
    let row = sqlx::query!(
        "SELECT w.owner_id, s.access AS \"access?\" FROM workspaces w LEFT JOIN workspace_shares s ON s.workspace_id=w.id AND s.user_id=$2 WHERE w.id=$1 AND w.state<>'deleted'",
        workspace,
        user
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(GatewayError::NotFound)?;
    if row.owner_id == user {
        return Ok(Access::Owner);
    }
    match row.access.as_deref() {
        Some("write") => Ok(Access::Write),
        Some("read") => Ok(Access::Read),
        _ => Err(GatewayError::Forbidden("workspace access is required")),
    }
}

/// Binds a new conversation to a workspace inside its creation transaction. Without a requested workspace, an
/// implicit one is created with the conversation's own id, so its directory is `workspaces/<conversation id>`.
///
/// # Errors
/// `404` or `403` from [`access`] (a read share cannot bind), or a store failure.
pub(crate) async fn bind(
    connection: &mut PgConnection,
    owner: Uuid,
    (requested, quota): (Option<Uuid>, u64),
    conversation: Uuid,
    title: &str,
) -> Result<Uuid, GatewayError> {
    if let Some(workspace) = requested {
        if !access(connection, owner, workspace).await?.may_bind() {
            return Err(GatewayError::Forbidden(
                "a write share is required to bind a task",
            ));
        }
        // A fork still being copied, or one that failed, holds no usable files (066c).
        let state = sqlx::query_scalar!("SELECT state FROM workspaces WHERE id=$1", workspace)
            .fetch_one(&mut *connection)
            .await?;
        if state != "ready" {
            return Err(GatewayError::Conflict("workspace_not_ready"));
        }
        return Ok(workspace);
    }
    let name: String = title.chars().take(100).collect();
    sqlx::query!(
        "INSERT INTO workspaces (id, owner_id, name, quota_bytes) VALUES ($1,$2,$3,$4)",
        conversation,
        owner,
        name,
        stored_quota(quota)?
    )
    .execute(&mut *connection)
    .await?;
    Ok(conversation)
}

/// The disk a workspace created for `template` gets: the environment's own, or the installation's default, within the
/// ceiling. It then stays with the workspace (the environment sizes design).
pub(crate) fn quota_for(config: &crate::GatewayConfig, template: &crate::config::Template) -> u64 {
    template
        .workspace_bytes()
        .unwrap_or(config.workspaces.quota_bytes)
        .min(config.sandbox.ceiling.workspace_bytes())
}

/// A workspace's own quota as stored, or the installation's default for one created before quotas were its own.
pub(crate) fn quota(config: &crate::GatewayConfig, stored: Option<i64>) -> u64 {
    stored
        .and_then(|bytes| u64::try_from(bytes).ok())
        .unwrap_or(config.workspaces.quota_bytes)
}

/// A quota as its column stores it.
///
/// # Errors
/// A quota too large to store, which the configuration bounds prevent.
pub(crate) fn stored_quota(bytes: u64) -> Result<i64, GatewayError> {
    i64::try_from(bytes)
        .map_err(|_| GatewayError::Configuration("a workspace quota exceeds its bound"))
}

/// The workspace a conversation is bound to, its git source for the first sandbox's clone, and its own quota.
pub(crate) struct Binding {
    pub id: Uuid,
    pub source_url: Option<String>,
    pub quota_bytes: Option<i64>,
}

/// Looks up a conversation's workspace and records its use.
///
/// # Errors
/// `404` for an unknown or deleted conversation (a deleted task reaches no workspace); a store failure.
pub(crate) async fn of_conversation(
    pool: &sqlx::PgPool,
    conversation: Uuid,
) -> Result<Binding, GatewayError> {
    let row = sqlx::query!(
        "UPDATE workspaces w SET last_used_at=CURRENT_TIMESTAMP FROM conversations c WHERE c.id=$1 AND c.state<>'deleted' AND w.id=c.workspace_id RETURNING w.id, w.source_url, w.quota_bytes",
        conversation
    )
    .fetch_optional(pool)
    .await?
    .ok_or(GatewayError::NotFound)?;
    Ok(Binding {
        id: row.id,
        source_url: row.source_url,
        quota_bytes: row.quota_bytes,
    })
}

/// A workspace or share name: 1-100 printable characters, returned trimmed.
///
/// # Errors
/// `400 invalid_request` otherwise.
pub(crate) fn valid_name(name: &str) -> Result<&str, GatewayError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 100 || name.chars().any(char::is_control) {
        return Err(GatewayError::BadRequest(
            "a workspace name is 1-100 printable characters",
        ));
    }
    Ok(name)
}

/// What a workspace is (migration 0081): a client words it in the reader's language. A rename keeps it: it says how the
/// workspace was made.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Kind {
    /// A person's workspace, or a person's fork of one.
    Standard,
    /// An agent team member's own copy of its lead's workspace (ADR-0034 point 6).
    HelperCopy,
}

impl Kind {
    /// The stored value.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::HelperCopy => "helper_copy",
        }
    }

    /// The stored value read back; the column's check allows only these two.
    pub(crate) fn stored(value: &str) -> Self {
        if value == Self::HelperCopy.as_str() {
            Self::HelperCopy
        } else {
            Self::Standard
        }
    }
}
