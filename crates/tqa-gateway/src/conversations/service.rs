use super::{Conversation, Record};
use crate::{
    GatewayError,
    app::State,
    auth::{Role, User},
};
use tqa_types::{ConversationId, SandboxBackend};
use uuid::Uuid;

pub(crate) async fn create_owned(
    state: &State,
    owner: Uuid,
    title: &str,
    route: &str,
) -> Result<Conversation, GatewayError> {
    let creation = Creation::new(state, owner, title, route)?.with_template(state, None)?;
    let id = Uuid::now_v7();
    // On the microVM backend the workspace is an image the backend creates; nothing is created on the host here.
    let path = if state.config.uses_block_images() {
        None
    } else {
        Some(state.storage.workspace(id).await?)
    };
    let created = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        let workspace = crate::workspaces::bind(
            &mut transaction,
            owner,
            (None, creation.workspace_quota()),
            id,
            creation.title(),
        )
        .await?;
        let conversation = creation.insert(&mut transaction, id, workspace).await?;
        transaction.commit().await?;
        Ok(conversation)
    })
    .await;
    match created {
        Ok(conversation) => Ok(conversation),
        Err(error) => {
            if let Some(path) = &path {
                state.storage.remove_workspace(path).await?;
            }
            Err(error)
        }
    }
}

/// Validated immutable bindings shared by direct fixtures and the idempotent HTTP create path.
pub(super) struct Creation<'a> {
    owner: Uuid,
    title: &'a str,
    route: &'a str,
    backend: &'static str,
    template: String,
    digest: String,
    /// The install catalogue's enabled-by-default MCP servers; a profile's set once profiles exist (GW-I-025).
    mcp_servers: Vec<String>,
    /// The enabled connectors marked `enabled_by_default`, within what the servers leave of the runtime's bound.
    connectors: Vec<String>,
    /// The profile's id, version and copy, when the conversation is made from one (ADR-0012 decision 5).
    profile: Option<(Uuid, i64, serde_json::Value)>,
    /// The disk of a workspace created for this conversation: its environment's (the environment sizes design).
    workspace_quota: u64,
}

impl<'a> Creation<'a> {
    pub(super) fn new(
        state: &State,
        owner: Uuid,
        title: &'a str,
        route: &'a str,
    ) -> Result<Self, GatewayError> {
        let title = valid_title(title)?;
        allowed_route(state, route)?;
        let backend = match state.config.backend {
            SandboxBackend::Docker => "docker",
            SandboxBackend::Firecracker => "firecracker",
            _ => return Err(GatewayError::Configuration("unsupported runtime backend")),
        };
        let mcp_servers = state.config.mcp.defaults();
        let connectors = default_connectors(state, mcp_servers.len());
        Ok(Self {
            owner,
            title,
            route,
            backend,
            template: String::new(),
            digest: String::new(),
            mcp_servers,
            connectors,
            profile: None,
            workspace_quota: state.config.workspaces.quota_bytes,
        })
    }

    /// Makes the conversation from `chosen`: its servers and connectors are the profile's, and it keeps a copy.
    ///
    /// # Errors
    /// A profile that cannot be serialised.
    pub(super) fn with_profile(
        mut self,
        chosen: Option<crate::profiles::Chosen>,
    ) -> Result<Self, GatewayError> {
        if let Some(chosen) = chosen {
            self.mcp_servers.clone_from(&chosen.profile.mcp_servers);
            self.connectors.clone_from(&chosen.profile.connectors);
            let copy = serde_json::to_value(&chosen.profile)?;
            self.profile = Some((chosen.id, chosen.version, copy));
        }
        Ok(self)
    }

    /// Takes another conversation's servers, connectors and profile copy as they are (an agent team's member without
    /// a profile takes its lead's, ADR-0034).
    #[must_use]
    pub(super) fn with_settings(
        mut self,
        mcp_servers: Vec<String>,
        connectors: Vec<String>,
        profile: Option<(Uuid, i64, serde_json::Value)>,
    ) -> Self {
        self.mcp_servers = mcp_servers;
        self.connectors = connectors;
        self.profile = profile;
        self
    }

    /// Binds the chosen template, or the default, with the image pin it resolves to now.
    ///
    /// # Errors
    /// `unknown_template` when the name is not configured.
    pub(super) fn with_template(
        mut self,
        state: &State,
        name: Option<&str>,
    ) -> Result<Self, GatewayError> {
        let snapshot = state.settings.current();
        let template = crate::environments::choose(&snapshot, name)?;
        template.name().clone_into(&mut self.template);
        self.digest.clone_from(&template.environment().digest);
        self.workspace_quota = crate::workspaces::quota_for(&state.config, template);
        Ok(self)
    }

    /// The disk of a workspace created for this conversation.
    pub(super) const fn workspace_quota(&self) -> u64 {
        self.workspace_quota
    }

    /// Inserts the conversation bound to `workspace` (the caller may bind it: see `crate::workspaces::bind`).
    pub(super) async fn insert(
        &self,
        connection: &mut sqlx::PgConnection,
        id: Uuid,
        workspace: Uuid,
    ) -> Result<Conversation, GatewayError> {
        let (profile_id, profile_version, profile) = self
            .profile
            .clone()
            .map_or((None, None, None), |(id, version, copy)| {
                (Some(id), Some(version), Some(copy))
            });
        let record = sqlx::query_as!(Record,
            "INSERT INTO conversations (id, owner_id, title, model_route, backend, template, template_digest, workspace_id, mcp_servers, connectors, profile_id, profile_version, profile) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13) RETURNING id,owner_id,title,title_source,model_route,backend,state,template,pinned,created_at,updated_at,last_activity_at,workspace_id,mcp_servers,connectors,profile_id,profile_version,profile->>'name' AS profile_name,profile->>'display_name' AS profile_display_name,(profile->>'model_route') IS NOT NULL AS \"profile_sets_route?\",(profile->>'template') IS NOT NULL AS \"profile_sets_template?\"",
            id, self.owner, self.title, self.route, self.backend, self.template, self.digest, workspace, &self.mcp_servers, &self.connectors, profile_id, profile_version, profile).fetch_one(connection).await?;
        record.try_into()
    }

    pub(super) const fn owner(&self) -> Uuid {
        self.owner
    }

    pub(super) const fn title(&self) -> &str {
        self.title
    }
}

/// A conversation title: at most 200 characters, not blank; returned trimmed.
///
/// # Errors
/// Rejects a blank or oversized title.
pub(super) fn valid_title(title: &str) -> Result<&str, GatewayError> {
    if title.trim().is_empty() || title.chars().count() > 200 {
        return Err(GatewayError::BadRequest(
            "conversation title or route exceeds its bound",
        ));
    }
    // PostgreSQL text cannot hold U+0000: refuse it here instead of failing the store.
    if title.contains('\0') {
        return Err(GatewayError::BadRequest(
            "conversation title contains a NUL character",
        ));
    }
    Ok(title.trim())
}

/// The MCP servers a conversation may use: distinct names from the install catalogue, at most the runtime's bound.
///
/// # Errors
/// `invalid_request` for an unknown, repeated or excess name.
pub(super) fn allowed_mcp_servers(state: &State, servers: &[String]) -> Result<(), GatewayError> {
    let distinct = servers
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        == servers.len();
    if servers.len() > crate::config::MAX_MCP_SERVERS
        || !distinct
        || !servers
            .iter()
            .all(|name| state.config.mcp.get(name).is_some())
    {
        return Err(GatewayError::BadRequest(
            "mcp_servers must name distinct catalogue servers (at most 8)",
        ));
    }
    Ok(())
}

/// A new conversation's connectors: the enabled ones marked `enabled_by_default`, by name, within the `room` the MCP
/// servers leave of the runtime's bound (a profile's set once profiles exist, GW-I-025).
fn default_connectors(state: &State, servers: usize) -> Vec<String> {
    let mut names: Vec<String> = state
        .settings
        .current()
        .connectors
        .iter()
        .filter(|document| document.enabled && document.enabled_by_default)
        .map(|document| document.name.clone())
        .collect();
    names.sort();
    names.truncate(crate::config::MAX_MCP_SERVERS.saturating_sub(servers));
    names
}

/// The connectors a conversation may use: distinct names of connectors that exist now (a disabled one may be kept in
/// the set; it is left out of the session and refused by the proxy until it is enabled again).
///
/// # Errors
/// `invalid_request` for an unknown, repeated or excess name.
pub(super) fn allowed_connectors(state: &State, connectors: &[String]) -> Result<(), GatewayError> {
    let snapshot = state.settings.current();
    let distinct = connectors
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        == connectors.len();
    if connectors.len() > crate::config::MAX_MCP_SERVERS
        || !distinct
        || !connectors.iter().all(|name| {
            snapshot
                .connectors
                .iter()
                .any(|document| &document.name == name)
        })
    {
        return Err(GatewayError::BadRequest(
            "connectors must name distinct existing connectors (at most 8)",
        ));
    }
    Ok(())
}

/// A model route the caller may use: currently every registered route.
///
/// # Errors
/// Rejects an oversized name and a route that is not registered.
pub(super) fn allowed_route(state: &State, route: &str) -> Result<(), GatewayError> {
    if route.len() > 256 {
        return Err(GatewayError::BadRequest(
            "conversation title or route exceeds its bound",
        ));
    }
    if state.settings.current().registry.get(route).is_none() {
        return Err(GatewayError::Forbidden("model route is not allowed"));
    }
    Ok(())
}

pub(crate) async fn authorize(
    state: &State,
    user: &User,
    id: &ConversationId,
    metadata: bool,
) -> Result<Conversation, GatewayError> {
    let id = super::uuid(id)?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let record = sqlx::query_as!(Record,
            "SELECT id,owner_id,title,title_source,model_route,backend,state,template,pinned,created_at,updated_at,last_activity_at,workspace_id,mcp_servers,connectors,profile_id,profile_version,profile->>'name' AS profile_name,profile->>'display_name' AS profile_display_name,(profile->>'model_route') IS NOT NULL AS \"profile_sets_route?\",(profile->>'template') IS NOT NULL AS \"profile_sets_template?\" FROM conversations WHERE id=$1", id)
            .fetch_optional(&state.pool).await?.ok_or(GatewayError::NotFound)?;
        // Delete means gone: a deleted task answers like one that never was, for every read, stream and action.
        if record.state == "deleted" {
            return Err(GatewayError::NotFound);
        }
        if record.owner_id != user.id && !(metadata && user.role == Role::Admin) {
            return Err(GatewayError::Forbidden("conversation ownership is required"));
        }
        record.try_into()
    }).await
}

/// Deletes `user`'s task: it is gone for every reader at once, its open streams end, and the purge removes its data
/// within minutes. One content-free audit row records who deleted it. Deleting a task already deleted answers as the
/// first delete did (a retry after a timeout sees no error) and records nothing more; once purged, it is not found.
///
/// # Errors
/// `404` for an unknown or purged task, `403` for someone else's, or a store failure.
pub(super) async fn remove(
    state: &State,
    user: &User,
    id: &ConversationId,
) -> Result<(), GatewayError> {
    let value = super::uuid(id)?;
    let deleted = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        let row = sqlx::query!(
            "SELECT owner_id,state FROM conversations WHERE id=$1 FOR UPDATE",
            value
        )
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(GatewayError::NotFound)?;
        if row.owner_id != user.id {
            return Err(GatewayError::Forbidden(
                "conversation ownership is required",
            ));
        }
        if row.state == "deleted" {
            return Ok(false);
        }
        sqlx::query!(
            "UPDATE conversations SET state='deleted',updated_at=CURRENT_TIMESTAMP WHERE id=$1",
            value
        )
        .execute(&mut *transaction)
        .await?;
        // A helper's own copy goes with its task (GW-I-108), whichever delete removes the task: its own, its lead's,
        // or a retry of either. The purge removes the copy's files and row once the task is gone. A copy another task
        // still works in is that task's person's now, and stays.
        sqlx::query!(
            "UPDATE workspaces w SET state='deleted',updated_at=CURRENT_TIMESTAMP \
             FROM conversations c JOIN agent_team_members m ON m.conversation_id=c.id \
             WHERE c.id=$1 AND m.workspace='own_copy' AND w.id=c.workspace_id AND w.kind='helper_copy' \
             AND w.state<>'deleted' \
             AND NOT EXISTS(SELECT 1 FROM conversations o WHERE o.workspace_id=w.id AND o.id<>c.id AND o.state<>'deleted')",
            value
        )
        .execute(&mut *transaction)
        .await?;
        sqlx::query!("DELETE FROM runtime_tokens WHERE conversation_id=$1", value)
            .execute(&mut *transaction)
            .await?;
        crate::host_keys::end_tasks(&mut transaction, &[value]).await?;
        crate::settings::audit_user(
            &mut transaction,
            user.id,
            "tasks",
            &value.to_string(),
            "delete",
        )
        .await?;
        transaction.commit().await?;
        Ok(true)
    })
    .await?;
    if deleted {
        gone(state, id)?;
    }
    Ok(())
}

/// What a deleted task leaves in memory goes with the delete: its runtime link, its open streams, its title request and
/// its screen's unclean mark; then the purge is woken.
///
/// # Errors
/// The link registry's lock is poisoned.
pub(crate) fn gone(state: &State, id: &ConversationId) -> Result<(), GatewayError> {
    state.links.remove(id)?;
    state.events.close(id);
    crate::relay::title::interrupt(state, id);
    crate::screen::task_deleted(id);
    state.purge.wake();
    Ok(())
}
