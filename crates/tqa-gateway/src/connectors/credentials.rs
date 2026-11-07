//! A user's own connector credentials (ADR-0031 decision 3): `/api/v1/connectors`. Session-only, like Git credentials:
//! a personal API token can neither plant nor read one. The value is sealed in the secret store and never returned,
//! logged or put in events. A credential is bound to the connector's URL origin when it is set and only counts while
//! the connector still points there.
use crate::{GatewayError, SecretRef, app::State, auth::SessionAuthz, request::ApiJson};
use axum::{
    Json,
    extract::{Path, State as AppState},
    http::StatusCode,
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

use super::{ConnectorDocument, Scheme};

/// The longest credential value: it travels in one HTTP header.
const MAX_VALUE_BYTES: usize = 4096;

/// Where the credential for one call comes from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialSource {
    /// The calling user's own credential.
    User,
    /// The connector's shared credential.
    Shared,
}

/// The credential the MCP proxy injects for one user's call.
#[derive(Clone)]
pub struct ConnectorCredential {
    /// Whose it is; logged as a word, never the value.
    pub source: CredentialSource,
    /// The value, sealed against accidental logging.
    pub value: SecretString,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewCredential {
    value: SecretString,
}

/// One connector as a user sees it: what it is and whether they hold a credential for it, never a value.
#[derive(Serialize)]
pub(crate) struct UserConnector {
    name: String,
    display_name: Option<String>,
    scheme: Scheme,
    required: bool,
    has_credential: bool,
    #[serde(with = "time::serde::rfc3339::option")]
    credential_updated_at: Option<time::OffsetDateTime>,
}

#[derive(Serialize)]
pub(crate) struct UserConnectors {
    items: Vec<UserConnector>,
}

/// The sealed value's store reference: derived from the user and connector, so it names neither.
fn reference(user: Uuid, connector: &str) -> Result<SecretRef, GatewayError> {
    let digest = Sha256::digest(format!("{user}\n{connector}").as_bytes());
    format!("secrets/connector-{}", crate::hex::hex(&digest)).parse()
}

/// The connector URL's origin (`scheme://host[:port]`), which a credential is bound to.
fn origin(document: &ConnectorDocument) -> Result<String, GatewayError> {
    url::Url::parse(&document.url)
        .map(|url| url.origin().ascii_serialization())
        .map_err(|_| GatewayError::StoreUnavailable)
}

/// An enabled connector by name, from the settings snapshot.
fn connector(state: &State, name: &str) -> Result<ConnectorDocument, GatewayError> {
    state
        .settings
        .current()
        .connectors
        .iter()
        .find(|document| document.name == name && document.enabled)
        .cloned()
        .ok_or(GatewayError::NotFound)
}

/// Printable ASCII without spaces: safe inside a header.
fn header_safe(value: &str) -> bool {
    (1..=MAX_VALUE_BYTES).contains(&value.len())
        && value.bytes().all(|byte| byte.is_ascii_graphic())
}

/// `GET /api/v1/connectors`: the enabled connectors (profiles narrow this once they exist), with whether the caller
/// holds a credential that still counts for each.
///
/// # Errors
/// `403 session_required` with a personal token, or a store failure.
pub(crate) async fn list(
    AppState(state): AppState<Arc<State>>,
    SessionAuthz(user): SessionAuthz,
) -> Result<Json<UserConnectors>, GatewayError> {
    let rows = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query!(
            "SELECT connector,origin,updated_at FROM connector_credentials WHERE user_id=$1",
            user.id
        )
        .fetch_all(&state.pool)
        .await?)
    })
    .await?;
    let snapshot = state.settings.current();
    let entries = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        let entries = crate::settings::connectors::entries(&mut transaction).await?;
        transaction.commit().await?;
        Ok(entries)
    })
    .await?;
    let items = snapshot
        .connectors
        .iter()
        .filter(|document| document.enabled)
        .map(|document| {
            let current = origin(document)?;
            let held = rows
                .iter()
                .find(|row| row.connector == document.name && row.origin == current);
            Ok(UserConnector {
                name: document.name.clone(),
                display_name: entries
                    .iter()
                    .find(|stored| stored.document.name == document.name)
                    .and_then(|stored| stored.display_name.clone()),
                scheme: document.auth.scheme,
                required: document.auth.required,
                has_credential: held.is_some(),
                credential_updated_at: held.map(|row| row.updated_at),
            })
        })
        .collect::<Result<_, GatewayError>>()?;
    Ok(Json(UserConnectors { items }))
}

/// `PUT /api/v1/connectors/{name}/credential {value}`: sets or replaces the caller's credential for an enabled
/// connector that sends one, bound to its current URL origin.
///
/// # Errors
/// `400` for an invalid value, `403 session_required`, `404` for an unknown or disabled connector,
/// `409 credential_not_used` for a connector without an auth scheme, or a store or secret-store failure.
pub(crate) async fn put(
    AppState(state): AppState<Arc<State>>,
    SessionAuthz(user): SessionAuthz,
    Path(name): Path<String>,
    ApiJson(body): ApiJson<NewCredential>,
) -> Result<StatusCode, GatewayError> {
    let document = connector(&state, &name)?;
    if document.auth.scheme == Scheme::None {
        return Err(GatewayError::Conflict("credential_not_used"));
    }
    if !header_safe(body.value.expose_secret()) {
        return Err(GatewayError::BadRequest(
            "value must be 1-4096 printable characters without spaces",
        ));
    }
    let secret = reference(user.id, &name)?;
    let origin = origin(&document)?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        let created = sqlx::query_scalar!(
            "INSERT INTO connector_credentials (user_id,connector,origin,secret_ref) VALUES ($1,$2,$3,$4) \
             ON CONFLICT (user_id,connector) DO UPDATE SET origin=EXCLUDED.origin, updated_at=CURRENT_TIMESTAMP \
             RETURNING (xmax = 0) AS \"created!\"",
            user.id,
            name,
            origin,
            secret.as_str()
        )
        .fetch_one(&mut *transaction)
        .await?;
        // The value is stored before the row commits: a committed row always has its value.
        state.secret_store.put(&secret, body.value).await?;
        let action = if created { "create" } else { "replace" };
        crate::settings::audit_user(&mut transaction, user.id, "connector_credentials", secret.as_str(), action)
            .await?;
        transaction.commit().await?;
        Ok(())
    })
    .await?;
    tracing::info!(user_id = %user.id, connector = %name, "connector credential set");
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /api/v1/connectors/{name}/credential`: deletes the caller's credential and its sealed value.
///
/// # Errors
/// `404` when the caller holds none, `403 session_required`, or a store or secret-store failure.
pub(crate) async fn remove(
    AppState(state): AppState<Arc<State>>,
    SessionAuthz(user): SessionAuthz,
    Path(name): Path<String>,
) -> Result<StatusCode, GatewayError> {
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        let secret = sqlx::query_scalar!(
            "DELETE FROM connector_credentials WHERE user_id=$1 AND connector=$2 RETURNING secret_ref",
            user.id,
            name
        )
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(GatewayError::NotFound)?;
        state.secret_store.delete(&secret.parse()?).await?;
        crate::settings::audit_user(&mut transaction, user.id, "connector_credentials", &secret, "delete").await?;
        transaction.commit().await?;
        Ok(())
    })
    .await?;
    tracing::info!(user_id = %user.id, connector = %name, "connector credential deleted");
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /api/v1/connectors/{name}/credential/reads?limit&before`: when the caller's own credential for a connector was
/// read, for which conversation and run, newest first; never its value (vault slice 2).
///
/// # Errors
/// `404` when the caller holds none, `400` for invalid paging, `403 session_required`, or a store failure.
pub(crate) async fn reads(
    AppState(state): AppState<Arc<State>>,
    SessionAuthz(user): SessionAuthz,
    Path(name): Path<String>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
) -> Result<axum::Json<crate::secrets::reads::ReadPage>, GatewayError> {
    let paging = crate::secrets::reads::paging(query.as_deref())?;
    let secret = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query_scalar!(
            "SELECT secret_ref FROM connector_credentials WHERE user_id=$1 AND connector=$2",
            user.id,
            name
        )
        .fetch_optional(&state.pool)
        .await?)
    })
    .await?
    .ok_or(GatewayError::NotFound)?;
    Ok(axum::Json(
        crate::secrets::reads::page(&state, &secret.parse()?, &paging, false).await?,
    ))
}

/// Deletes every user's credential for a connector that is being deleted, with their sealed values, in the caller's
/// transaction.
///
/// # Errors
/// A store or secret-store failure.
pub(crate) async fn forget(
    state: &State,
    connection: &mut sqlx::PgConnection,
    name: &str,
) -> Result<u64, GatewayError> {
    let secrets = sqlx::query_scalar!(
        "DELETE FROM connector_credentials WHERE connector=$1 RETURNING secret_ref",
        name
    )
    .fetch_all(connection)
    .await?;
    for secret in &secrets {
        state.secret_store.delete(&secret.parse()?).await?;
    }
    Ok(u64::try_from(secrets.len()).unwrap_or(u64::MAX))
}

/// The credential for `user`'s call to `name`: their own while it is bound to the connector's current origin, else
/// the connector's shared one, else none. Records the use of a user's credential.
///
/// # Errors
/// `404` for an unknown or disabled connector, or a store or secret-store failure (fail closed).
pub(crate) async fn resolve(
    state: &State,
    user: Uuid,
    name: &str,
    conversation: Option<Uuid>,
) -> Result<Option<ConnectorCredential>, GatewayError> {
    // Every read is recorded, never a value (vault slice 2).
    let context = crate::secrets::reads::ReadContext {
        conversation,
        user: Some(user),
        run: None,
    };
    let consumer = (crate::secrets::reads::ConsumerKind::Connector, name);
    let document = connector(state, name)?;
    if document.auth.scheme == Scheme::None {
        return Ok(None);
    }
    let current = origin(&document)?;
    let own = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query_scalar!(
            "UPDATE connector_credentials SET last_used_at=CURRENT_TIMESTAMP WHERE user_id=$1 AND connector=$2 AND origin=$3 RETURNING secret_ref",
            user,
            name,
            current
        )
        .fetch_optional(&state.pool)
        .await?)
    })
    .await?;
    if let Some(secret) = own
        && let Some(value) =
            crate::secrets::reads::read(state, &secret.parse()?, consumer, &context).await?
    {
        return Ok(Some(ConnectorCredential {
            source: CredentialSource::User,
            value,
        }));
    }
    let Some(shared) = document.secret_ref.as_deref() else {
        return Ok(None);
    };
    Ok(
        crate::secrets::reads::read(state, &shared.parse()?, consumer, &context)
            .await?
            .map(|value| ConnectorCredential {
                source: CredentialSource::Shared,
                value,
            }),
    )
}
