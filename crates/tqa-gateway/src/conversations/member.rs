//! An agent team's member task (ADR-0034 point 3, slice 2): a normal task of the lead's person, made in the caller's
//! transaction. Without a profile it takes the lead's effective settings (environment, servers, connectors and profile
//! copy) and the helpers' route when the file sets one; with a profile, the profile's, narrowed to what the lead's
//! task may use and never wider. Network rules and keys are the person's own, so a member always has its lead's.
use super::service::Creation;
use crate::{GatewayError, app::State, profiles::Chosen};
use serde_json::Value;
use sqlx::PgConnection;
use tqa_types::WorkspaceMode;
use uuid::Uuid;

/// The member task made, and what it has to do once its transaction commits.
pub(crate) struct MemberTask {
    pub(crate) conversation: Uuid,
    pub(crate) workspace: Uuid,
    /// The profile's tools, servers and connectors the lead may not use, left out (sorted).
    pub(crate) left_out: Vec<String>,
    /// An own copy being made: its source and quota, copied by `workspaces::fork::copy` after the commit.
    pub(crate) fork: Option<(Uuid, u64)>,
    /// For an own copy, whether it is consistent: false when the lead's sandbox had its files mounted, as it does
    /// while the lead runs, so files it was changing may be partial. `None` on the lead's own files.
    pub(crate) consistent: Option<bool>,
}

/// What the member takes from its lead's task.
struct Lead {
    owner_id: Uuid,
    model_route: String,
    template: String,
    mcp_servers: Vec<String>,
    connectors: Vec<String>,
    profile_id: Option<Uuid>,
    profile_version: Option<i64>,
    profile: Option<Value>,
    workspace_id: Uuid,
}

impl Lead {
    /// The tools its profile copy allows; `None` for every tool.
    fn tools(&self) -> Option<Vec<String>> {
        self.profile
            .as_ref()
            .and_then(|copy| copy.get("tools"))
            .and_then(Value::as_array)
            .map(|tools| {
                tools
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
    }

    /// Its profile copy as a conversation keeps it.
    fn copy(&self) -> Option<(Uuid, i64, Value)> {
        self.profile_id
            .zip(self.profile_version)
            .zip(self.profile.clone())
            .map(|((id, version), copy)| (id, version, copy))
    }
}

/// Makes the member task titled `title` for `lead`, working where `workspace` says.
///
/// # Errors
/// `404` for an unknown lead; the creation refusals (a route or environment the installation no longer has); the
/// fork's own codes for `own_copy`, and `workspace_not_ready` for `my_files` on a workspace still being copied.
pub(crate) async fn create(
    connection: &mut PgConnection,
    state: &State,
    (leader, title): (Uuid, &str),
    profile: Option<Chosen>,
    workspace: WorkspaceMode,
) -> Result<MemberTask, GatewayError> {
    let lead = sqlx::query_as!(
        Lead,
        "SELECT owner_id, model_route, template, mcp_servers, connectors, profile_id, profile_version, profile, \
         workspace_id FROM conversations WHERE id = $1",
        leader
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(GatewayError::NotFound)?;
    // The administrator's setting, else the file's (ADR-0034 S6); with neither, the lead's own route.
    let set = crate::admin::agent_teams::member_route(&mut *connection).await?;
    let helpers = set
        .as_deref()
        .or(state.config.agent_teams.member_route.as_deref());
    let (route, template, profile, left_out) = match profile {
        None => (
            helpers.unwrap_or(&lead.model_route).to_owned(),
            lead.template.clone(),
            None,
            Vec::new(),
        ),
        Some(chosen) => {
            let (chosen, left_out) = narrowed(chosen, &lead);
            let route = chosen
                .profile
                .model_route
                .clone()
                .or_else(|| helpers.map(str::to_owned))
                .unwrap_or_else(|| lead.model_route.clone());
            let template = chosen
                .profile
                .template
                .clone()
                .unwrap_or_else(|| lead.template.clone());
            (route, template, Some(chosen), left_out)
        }
    };
    let creation = Creation::new(state, lead.owner_id, title, &route)?
        .with_template(state, Some(&template))?;
    let creation = if let Some(chosen) = profile {
        creation.with_profile(Some(chosen))?
    } else {
        creation.with_settings(
            lead.mcp_servers.clone(),
            lead.connectors.clone(),
            lead.copy(),
        )
    };
    let id = Uuid::now_v7();
    let (bound, fork, consistent) = place(
        connection,
        state,
        &lead,
        (id, title),
        (workspace, creation.workspace_quota()),
    )
    .await?;
    creation.insert(&mut *connection, id, bound).await?;
    Ok(MemberTask {
        conversation: id,
        workspace: bound,
        left_out,
        fork,
        consistent,
    })
}

/// The profile narrowed to what the lead's task may use, and what that left out (sorted): servers and connectors the
/// lead lacks, and tools outside those the lead may use. A profile allowing every tool takes the lead's tools.
fn narrowed(mut chosen: Chosen, lead: &Lead) -> (Chosen, Vec<String>) {
    let mut left_out = Vec::new();
    let mut within = |wanted: &[String], allowed: Option<&[String]>| -> Vec<String> {
        let (kept, out): (Vec<String>, Vec<String>) = wanted
            .iter()
            .cloned()
            .partition(|name| allowed.is_none_or(|allowed| allowed.contains(name)));
        left_out.extend(out);
        kept
    };
    chosen.profile.mcp_servers = within(&chosen.profile.mcp_servers, Some(&lead.mcp_servers));
    chosen.profile.connectors = within(&chosen.profile.connectors, Some(&lead.connectors));
    let lead_tools = lead.tools();
    chosen.profile.tools = match chosen.profile.tools.take() {
        Some(wanted) => Some(within(&wanted, lead_tools.as_deref())),
        None => lead_tools,
    };
    left_out.sort();
    left_out.dedup();
    (chosen, left_out)
}

/// Where the member works: the lead's own workspace, or a fork of it being made (its source and quota returned for
/// the copy after the commit).
async fn place(
    connection: &mut PgConnection,
    state: &State,
    lead: &Lead,
    (id, title): (Uuid, &str),
    (workspace, quota): (WorkspaceMode, u64),
) -> Result<(Uuid, Option<(Uuid, u64)>, Option<bool>), GatewayError> {
    match workspace {
        WorkspaceMode::MyFiles => {
            let bound = crate::workspaces::bind(
                connection,
                lead.owner_id,
                (Some(lead.workspace_id), quota),
                id,
                title,
            )
            .await?;
            Ok((bound, None, None))
        }
        WorkspaceMode::OwnCopy => {
            let copy = Uuid::now_v7();
            // The task's own title: the kind says it is a helper's copy, and the page words that (GW-I-215).
            let name: String = title.chars().take(100).collect();
            let (stored, consistent) = crate::workspaces::fork::begin(
                connection,
                lead.owner_id,
                lead.workspace_id,
                (copy, &name, crate::workspaces::Kind::HelperCopy),
            )
            .await?;
            let quota = crate::workspaces::quota(&state.config, stored);
            Ok((copy, Some((lead.workspace_id, quota)), Some(consistent)))
        }
    }
}
