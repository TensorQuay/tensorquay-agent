//! Bringing a finished helper back (ADR-0034's rounds amendment, A3): `start_member` naming a helper of this team that
//! has finished brings that helper back, its task, row and name kept, instead of starting a new one. Its workspace and
//! profile are its own; `files` keeps its copy or takes a fresh one; and it gets no more than its lead
//! has now (SEC-067).
use super::start::{Started, open_team};
use crate::GatewayError;
use crate::config::AgentTeams;
use sqlx::PgConnection;
use tqa_types::{BroughtBackFiles, StartMember, WorkspaceMode};
use uuid::Uuid;

/// A finished helper the call names.
pub(crate) struct Finished {
    conversation: Uuid,
    name: String,
    my_files: bool,
    profile_id: Option<String>,
    profile_name: Option<String>,
    copy_ready: bool,
}

/// The lead's finished helper named `role`, in any letter case: `done`, `stopped`, `waiting`, or failed with its task
/// still running; a live or gone one is no match (a new helper then).
///
/// # Errors
/// Storage failure.
pub(crate) async fn find(
    connection: &mut PgConnection,
    lead: Uuid,
    role: &str,
) -> Result<Option<Finished>, GatewayError> {
    let row = sqlx::query!(
        r#"SELECT m.conversation_id, m.name, m.workspace, c.profile_id::text AS profile_id,
                  c.profile->>'name' AS profile_name, w.state AS "workspace_state?"
           FROM agent_team_members m JOIN agent_teams t ON t.id = m.team_id
           JOIN conversations c ON c.id = m.conversation_id LEFT JOIN workspaces w ON w.id = c.workspace_id
           WHERE t.lead_conversation = $1 AND t.state <> 'stopping' AND lower(m.name) = lower($2)
             AND c.state = 'active'
             AND (m.state IN ('done', 'stopped', 'waiting') OR (m.state = 'failed' AND m.failed_run))
           ORDER BY t.created_at DESC LIMIT 1"#,
        lead,
        role
    )
    .fetch_optional(connection)
    .await?;
    Ok(row.map(|row| Finished {
        conversation: row.conversation_id,
        name: row.name,
        my_files: row.workspace == "my_files",
        profile_id: row.profile_id,
        profile_name: row.profile_name,
        copy_ready: row.workspace_state.as_deref() == Some("ready"),
    }))
}

/// What the helper's own workspace is, in words.
const fn place(my_files: bool) -> &'static str {
    if my_files {
        "the lead's own files"
    } else {
        "its own copy"
    }
}

/// The checks a bring-back passes first (A3): its own workspace and profile, `files` it can take, and a copy to keep.
fn check(
    request: &StartMember,
    back: &Finished,
) -> Result<(WorkspaceMode, BroughtBackFiles), GatewayError> {
    let name = &back.name;
    let theirs = if back.my_files {
        WorkspaceMode::MyFiles
    } else {
        WorkspaceMode::OwnCopy
    };
    if request.workspace != theirs {
        return Err(GatewayError::ParameterRefused {
            field: "workspace",
            said: format!(
                "{name} works in {}, which a helper brought back keeps; name it with that workspace, and use files \
                 to choose keep or fresh_copy",
                place(back.my_files)
            ),
        });
    }
    if let Some(profile) = &request.profile
        && back.profile_id.as_deref() != Some(profile.as_str())
        && back.profile_name.as_deref() != Some(profile.as_str())
    {
        return Err(GatewayError::ParameterRefused {
            field: "profile",
            said: format!(
                "{name} keeps the agent profile it had; leave profile out to bring it back"
            ),
        });
    }
    let files = request.files.unwrap_or_default();
    if back.my_files && files == BroughtBackFiles::FreshCopy {
        return Err(GatewayError::ParameterRefused {
            field: "files",
            said: format!("{name} works on the lead's own files, so it takes keep only"),
        });
    }
    if !back.my_files && files == BroughtBackFiles::Keep && !back.copy_ready {
        return Err(GatewayError::ConflictSaid {
            code: "workspace_gone",
            said: format!(
                "{name}'s copy of the files is gone; bring it back with files fresh_copy for a new one"
            ),
        });
    }
    Ok((theirs, files))
}

/// Brings `back` back for `request` in the lead's `run`: the checks, the round (opened if the team ended), a seat, the
/// helper narrowed to its lead's settings now, `starting` in this round, and the brief as its `assign`.
///
/// # Errors
/// `parameter_invalid` for a changed workspace or profile or `fresh_copy` on the lead's files, `workspace_gone`, the
/// round's own refusals (`round_needs_person`, `team_ended`, `team_stopped`), `member_limit`, storage failure.
pub(crate) async fn start(
    connection: &mut PgConnection,
    state: &crate::app::State,
    (leader, owner, run): (Uuid, Uuid, &str),
    (request, back): (&StartMember, Finished),
    (cap, teams): (super::cap::Cap, &AgentTeams),
) -> Result<Started, GatewayError> {
    let (theirs, files) = check(request, &back)?;
    let (team, channel, _) = open_team(connection, (leader, Some(run)), owner, teams).await?;
    let live = sqlx::query_scalar!(
        "SELECT count(*) AS \"live!\" FROM agent_team_members m JOIN agent_teams t ON t.id = m.team_id \
         WHERE m.team_id = $1 AND m.round = t.round \
         AND m.state <> 'stopped' AND NOT (m.state = 'failed' AND NOT m.failed_run)",
        team
    )
    .fetch_one(&mut *connection)
    .await?;
    if live >= i64::from(cap.limit) {
        return Err(GatewayError::MemberLimit {
            limit: cap.limit,
            by: cap.by,
        });
    }
    let (fork, replaced) = if files == BroughtBackFiles::FreshCopy {
        fresh_copy(connection, state, (leader, owner), &back).await?
    } else {
        (None, None)
    };
    narrow(connection, back.conversation, leader).await?;
    let round = sqlx::query_scalar!(
        "UPDATE agent_team_members m SET state = 'starting', round = t.round FROM agent_teams t \
         WHERE m.conversation_id = $1 AND t.id = m.team_id RETURNING t.round",
        back.conversation
    )
    .fetch_one(&mut *connection)
    .await?;
    let seq = super::store::assign_brief(
        connection,
        channel,
        (leader, back.conversation),
        &request.brief,
    )
    .await?;
    let workspace_id = sqlx::query_scalar!(
        "SELECT workspace_id FROM conversations WHERE id = $1",
        back.conversation
    )
    .fetch_one(&mut *connection)
    .await?;
    Ok(Started {
        member: serde_json::json!({
            "name": back.name,
            "conversation": back.conversation,
            "workspace": theirs,
            "workspace_id": workspace_id,
            "state": "starting",
        }),
        channel,
        seq,
        fork,
        brought_back: Some((files, u32::try_from(round).unwrap_or(u32::MAX))),
        replaced: replaced.map(|old| (back.conversation, old)),
    })
}

/// A fresh copy for a helper brought back: a new fork of its lead's files now, the helper's task moved to it; its old
/// copy is answered, to delete once the new one is ready.
async fn fresh_copy(
    connection: &mut PgConnection,
    state: &crate::app::State,
    (leader, owner): (Uuid, Uuid),
    back: &Finished,
) -> Result<(Option<(Uuid, Uuid, u64)>, Option<Uuid>), GatewayError> {
    let row = sqlx::query!(
        "SELECT l.workspace_id AS lead_files, h.workspace_id AS old FROM conversations l, conversations h \
         WHERE l.id = $1 AND h.id = $2",
        leader,
        back.conversation
    )
    .fetch_one(&mut *connection)
    .await?;
    let (lead_files, old) = (row.lead_files, row.old);
    let copy = Uuid::now_v7();
    let title: String = back.name.chars().take(100).collect();
    let (stored, _) = crate::workspaces::fork::begin(
        connection,
        owner,
        lead_files,
        (copy, &title, crate::workspaces::Kind::HelperCopy),
    )
    .await?;
    sqlx::query!(
        "UPDATE conversations SET workspace_id = $1, updated_at = CURRENT_TIMESTAMP WHERE id = $2",
        copy,
        back.conversation
    )
    .execute(&mut *connection)
    .await?;
    let quota = crate::workspaces::quota(&state.config, stored);
    Ok((Some((lead_files, copy, quota)), Some(old)))
}

/// No more than its lead has now (SEC-067): a helper brought back keeps only the servers and connectors its lead has,
/// and its profile is narrowed to its lead's as it is now, as a new helper's is: one that carried its lead's copy (or
/// none) takes the lead's copy now; one with a profile of its own keeps only the tools the lead's profile allows now
/// (reviewed on 18 Nov 2026: the narrower profile is taken, never a refusal).
async fn narrow(
    connection: &mut PgConnection,
    helper: Uuid,
    leader: Uuid,
) -> Result<(), GatewayError> {
    sqlx::query!(
        "UPDATE conversations h SET \
         mcp_servers = ARRAY(SELECT x FROM unnest(h.mcp_servers) WITH ORDINALITY AS a(x, i) \
                             WHERE x = ANY(l.mcp_servers) ORDER BY i), \
         connectors = ARRAY(SELECT x FROM unnest(h.connectors) WITH ORDINALITY AS a(x, i) \
                            WHERE x = ANY(l.connectors) ORDER BY i) \
         FROM conversations l WHERE h.id = $1 AND l.id = $2",
        helper,
        leader
    )
    .execute(&mut *connection)
    .await?;
    narrow_profile(connection, helper, leader).await
}

/// The profile half of [`narrow`].
async fn narrow_profile(
    connection: &mut PgConnection,
    helper: Uuid,
    leader: Uuid,
) -> Result<(), GatewayError> {
    let row = sqlx::query!(
        "SELECT l.profile_id AS lead_id, l.profile_version AS lead_version, l.profile AS lead_profile, \
         h.profile_id AS own_id, h.profile AS own_profile FROM conversations l, conversations h \
         WHERE l.id = $1 AND h.id = $2",
        leader,
        helper
    )
    .fetch_one(&mut *connection)
    .await?;
    let tools = |profile: &Option<serde_json::Value>| -> Option<Vec<String>> {
        profile
            .as_ref()
            .and_then(|copy| copy.get("tools"))
            .and_then(serde_json::Value::as_array)
            .map(|tools| {
                tools
                    .iter()
                    .filter_map(|tool| tool.as_str().map(str::to_owned))
                    .collect()
            })
    };
    let carried = row.own_id.is_none() || row.own_id == row.lead_id;
    if carried {
        // Its lead's copy as it is now, as a new helper without a profile takes it.
        sqlx::query!(
            "UPDATE conversations SET profile_id = $2, profile_version = $3, profile = $4 WHERE id = $1",
            helper,
            row.lead_id,
            row.lead_version,
            row.lead_profile
        )
        .execute(&mut *connection)
        .await?;
        return Ok(());
    }
    let Some(allowed) = tools(&row.lead_profile) else {
        return Ok(());
    };
    let kept: Vec<String> = match tools(&row.own_profile) {
        Some(own) => own
            .into_iter()
            .filter(|tool| allowed.contains(tool))
            .collect(),
        None => allowed,
    };
    sqlx::query!(
        "UPDATE conversations SET profile = jsonb_set(profile, '{tools}', to_jsonb($2::text[])) WHERE id = $1",
        helper,
        &kept
    )
    .execute(&mut *connection)
    .await?;
    Ok(())
}

/// Deletes a brought-back helper's old copy once its fresh one is made: the purge takes its files. Only a copy no
/// active task still works in.
///
/// # Errors
/// Storage failure.
pub(crate) async fn delete_old_copy(
    state: &crate::app::State,
    old: Uuid,
) -> Result<(), GatewayError> {
    sqlx::query!(
        "UPDATE workspaces SET state = 'deleted', updated_at = CURRENT_TIMESTAMP WHERE id = $1 AND state <> 'deleted' \
         AND NOT EXISTS (SELECT 1 FROM conversations WHERE workspace_id = $1 AND state = 'active')",
        old
    )
    .execute(&state.pool)
    .await?;
    Ok(())
}
