//! Conversation creation uses the same owner/key namespace and canonical body fingerprint as actions.
use super::Create;
use crate::conversations::idempotent::Keyed;
use crate::{GatewayError, app::State, conversations::service::Creation};
use axum::{
    Json,
    http::{HeaderMap, StatusCode},
};
use serde_json::Value;
use uuid::Uuid;

const ROUTE: &str = "/api/v1/conversations";

pub(super) async fn create(
    state: &State,
    owner: Uuid,
    headers: &HeaderMap,
    input: &Create,
) -> Result<(StatusCode, Json<Value>), GatewayError> {
    let keyed = Keyed::new(owner, headers, ROUTE.into(), input)?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        if let Some((status, body)) = keyed.begin(state, &mut transaction).await? {
            return Ok((status, Json(body)));
        }
        let chosen = match input.profile {
            Some(id) => Some(crate::profiles::chosen(&mut transaction, state, owner, id).await?),
            None => None,
        };
        let profile = chosen.as_ref().map(|chosen| &chosen.profile);
        let route = crate::profiles::set(
            "model_route",
            input.model_route.as_deref(),
            profile.and_then(|profile| profile.model_route.as_deref()),
        )?
        .ok_or(GatewayError::Invalid {
            field: "model_route",
            reason: crate::InvalidReason::ValueEmpty,
        })?
        .to_owned();
        let template = crate::profiles::set(
            "template",
            input.template.as_deref(),
            profile.and_then(|profile| profile.template.as_deref()),
        )?
        .map(str::to_owned);
        let creation = Creation::new(state, owner, &input.title, &route)?
            .with_template(state, template.as_deref())?
            .with_profile(chosen)?;
        let id = Uuid::now_v7();
        // An implicit workspace takes the conversation's id, so its directory is `workspaces/<id>` as before. A bound
        // workspace already has its directory; none is created before its access check. On the microVM backend the
        // workspace is an image the backend creates for the first sandbox, and nothing is created on the host here.
        let path = match input.workspace_id {
            None if !state.config.uses_block_images() => Some(state.storage.workspace(id).await?),
            _ => None,
        };
        let prepared = async {
            let workspace = crate::workspaces::bind(
                &mut transaction,
                creation.owner(),
                (input.workspace_id, creation.workspace_quota()),
                id,
                creation.title(),
            )
            .await?;
            let conversation = creation.insert(&mut transaction, id, workspace).await?;
            let body = serde_json::to_value(conversation)?;
            // After the row: the answer names the task (its title and description), so it goes with its purge.
            keyed
                .record_about(&mut transaction, id, StatusCode::CREATED, &body)
                .await?;
            Ok::<_, GatewayError>(body)
        }
        .await;
        let body = match prepared {
            Ok(body) => body,
            Err(error) => {
                transaction.rollback().await?;
                // Only an implicit, just-created directory is removed; a bound workspace keeps its files.
                if let Some(path) = &path {
                    state.storage.remove_workspace(path).await?;
                }
                return Err(error);
            }
        };
        // A commit transport failure is ambiguous: retain the workspace for a possible committed row.
        transaction.commit().await?;
        Ok((StatusCode::CREATED, Json(body)))
    })
    .await
}
