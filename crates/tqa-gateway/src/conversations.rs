//! Immutable model/backend bindings and owner-only conversation actions.

pub(crate) mod active;
mod cursor;
mod endpoints;
mod environment;
pub(crate) mod idempotent;
pub(crate) mod member;
mod patch;
pub(crate) use patch::route_changed;
mod service;
#[cfg(test)]
mod tests;
pub(crate) use endpoints::{create, delete, environment, get, list, patch};
pub(crate) use service::{authorize, create_owned, gone};

use crate::GatewayError;
use serde::Serialize;
use tqa_types::ConversationId;

/// Public metadata; host paths and runtime credentials are deliberately absent.
#[derive(Clone, Debug, Serialize)]
pub struct Conversation {
    /// Opaque application-generated UUID v7 conversation identifier.
    pub id: ConversationId,
    /// The owning account's UUID v7 identifier.
    pub owner_id: uuid::Uuid,
    /// The workspace this conversation is bound to for its whole life.
    pub workspace_id: uuid::Uuid,
    /// The bounded display title; absent when an administrator views another user's task (titles are content).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Where the title came from: `message` (the first message, until the task is named), `model` (named after its
    /// first completed run) or `person` (renamed, never replaced).
    pub title_source: String,
    /// The immutable logical model route.
    pub model_route: String,
    /// The immutable configured runtime backend.
    pub backend: tqa_types::SandboxBackend,
    /// Conversation lifecycle state, currently active or deleted.
    pub state: String,
    /// The environment template the next sandbox is built from.
    pub template: String,
    /// Whether the keep-alive pin holds its sandbox through idle periods.
    pub pinned: bool,
    /// UTC creation timestamp.
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: time::OffsetDateTime,
    /// UTC timestamp of the latest metadata change.
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: time::OffsetDateTime,
    /// UTC database time of the latest accepted message or terminal run event.
    #[serde(with = "time::serde::rfc3339")]
    pub last_activity_at: time::OffsetDateTime,
    /// The install catalogue's MCP servers this conversation uses; a change applies at the next run (ADR-0011).
    pub mcp_servers: Vec<String>,
    /// The connectors this conversation uses through the gateway's proxy, which admits no other (ADR-0031); a change
    /// applies at the next run.
    pub connectors: Vec<String>,
    /// The profile the conversation was made from, as it was then (ADR-0012 decision 5); absent without one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<ProfileOrigin>,
    /// The task's place in an agent team (ADR-0034, the slice S6 contract): null for a task in no team, and always
    /// while agent teams are off. The routes that answer a task fill it for their viewer (`teams::place::attach`).
    pub team: Option<crate::teams::place::TeamPlace>,
}

/// Which profile, at which version, a conversation keeps a copy of; the profile itself may since have changed or gone.
#[derive(Clone, Debug, Serialize)]
pub struct ProfileOrigin {
    /// The profile's id.
    pub id: uuid::Uuid,
    /// The version copied.
    pub version: i64,
    /// Its name then.
    pub name: String,
    /// Its display name then, for a page to show; the name when a copy has none.
    pub display_name: String,
    /// The conversation fields the profile sets, which a request may not change or widen: `model_route` and `template`
    /// when the profile names them, `mcp_servers` and `connectors` always (an empty set means none).
    pub sets: Vec<&'static str>,
}

pub(crate) struct Record {
    pub id: uuid::Uuid,
    pub owner_id: uuid::Uuid,
    pub title: String,
    pub title_source: String,
    pub model_route: String,
    pub backend: String,
    pub state: String,
    pub template: String,
    pub pinned: bool,
    pub created_at: time::OffsetDateTime,
    pub updated_at: time::OffsetDateTime,
    pub last_activity_at: time::OffsetDateTime,
    pub workspace_id: uuid::Uuid,
    pub mcp_servers: Vec<String>,
    pub connectors: Vec<String>,
    pub profile_id: Option<uuid::Uuid>,
    pub profile_version: Option<i64>,
    pub profile_name: Option<String>,
    pub profile_display_name: Option<String>,
    pub profile_sets_route: Option<bool>,
    pub profile_sets_template: Option<bool>,
}

impl TryFrom<Record> for Conversation {
    type Error = GatewayError;
    fn try_from(record: Record) -> Result<Self, Self::Error> {
        let backend = match record.backend.as_str() {
            "docker" => tqa_types::SandboxBackend::Docker,
            "firecracker" => tqa_types::SandboxBackend::Firecracker,
            _ => return Err(GatewayError::StoreUnavailable),
        };
        Ok(Self {
            id: record
                .id
                .to_string()
                .parse()
                .map_err(|_| GatewayError::StoreUnavailable)?,
            owner_id: record.owner_id,
            workspace_id: record.workspace_id,
            title: Some(record.title),
            title_source: record.title_source,
            model_route: record.model_route,
            backend,
            state: record.state,
            template: record.template,
            pinned: record.pinned,
            created_at: record.created_at,
            updated_at: record.updated_at,
            last_activity_at: record.last_activity_at,
            mcp_servers: record.mcp_servers,
            connectors: record.connectors,
            profile: match (
                record.profile_id,
                record.profile_version,
                record.profile_name,
            ) {
                (Some(id), Some(version), Some(name)) => Some(ProfileOrigin {
                    id,
                    version,
                    display_name: record.profile_display_name.unwrap_or_else(|| name.clone()),
                    name,
                    sets: [
                        ("model_route", record.profile_sets_route == Some(true)),
                        ("template", record.profile_sets_template == Some(true)),
                        ("mcp_servers", true),
                        ("connectors", true),
                    ]
                    .into_iter()
                    .filter_map(|(field, set)| set.then_some(field))
                    .collect(),
                }),
                _ => None,
            },
            team: None,
        })
    }
}

impl Conversation {
    /// The view for `viewer`: an administrator sees other users' task metadata, never their titles.
    #[must_use]
    pub(crate) fn for_viewer(mut self, viewer: uuid::Uuid) -> Self {
        if self.owner_id != viewer {
            self.title = None;
        }
        self
    }
}

pub(crate) fn uuid(id: &ConversationId) -> Result<uuid::Uuid, GatewayError> {
    let value = uuid::Uuid::parse_str(id.as_str())
        .map_err(|_| GatewayError::BadRequest("conversation id must be a UUID"))?;
    if id.as_str() != value.to_string() {
        return Err(GatewayError::BadRequest(
            "conversation id must use canonical UUID spelling",
        ));
    }
    Ok(value)
}
