//! Personal API tokens (GW-I-023): a program acts as its user until the token expires or is revoked. Only the
//! SHA-256 of a token is stored; the token itself is returned once, at creation. Management needs a session.
use super::{SessionAuthz, TOKEN_PREFIX, token};
use crate::{GatewayError, app::State, request::ApiJson};
use axum::{
    Json,
    extract::{Path, State as AppState},
    http::StatusCode,
};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Live tokens one user may hold at once.
const MAX_TOKENS: i64 = 50;
/// The longest lifetime a token may have.
const MAX_DAYS: u32 = 365;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewToken {
    name: String,
    expires_in_days: u32,
}

/// A token's metadata; the secret itself appears only in the creation response.
#[derive(Serialize)]
pub(crate) struct Token {
    id: uuid::Uuid,
    name: String,
    /// The last four characters, to tell tokens apart.
    hint: String,
    #[serde(with = "time::serde::rfc3339")]
    created_at: time::OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    expires_at: time::OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    last_used_at: Option<time::OffsetDateTime>,
}

#[derive(Serialize)]
pub(crate) struct Created {
    #[serde(flatten)]
    token: Token,
    /// Shown once; never stored or returned again.
    #[serde(rename = "token")]
    secret: String,
}

#[derive(Serialize)]
pub(crate) struct Tokens {
    items: Vec<Token>,
}

fn valid_name(name: &str) -> Result<&str, GatewayError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 100 || name.chars().any(char::is_control) {
        return Err(GatewayError::BadRequest(
            "a token name is 1-100 printable characters",
        ));
    }
    Ok(name)
}

/// `POST /api/v1/tokens`.
///
/// # Errors
/// Invalid name or lifetime, a live token with the same name, too many live tokens, or a store failure.
pub(crate) async fn create(
    AppState(state): AppState<Arc<State>>,
    SessionAuthz(user): SessionAuthz,
    ApiJson(body): ApiJson<NewToken>,
) -> Result<(StatusCode, Json<Created>), GatewayError> {
    let name = valid_name(&body.name)?.to_owned();
    if !(1..=MAX_DAYS).contains(&body.expires_in_days) {
        return Err(GatewayError::BadRequest("a token expires in 1 to 365 days"));
    }
    let secret = format!("{TOKEN_PREFIX}{}", token::mint_url_safe()?.expose_secret());
    let hash = token::digest(&secret);
    let hint = secret[secret.len() - 4..].to_owned();
    let id = uuid::Uuid::now_v7();
    let days = i32::try_from(body.expires_in_days)
        .map_err(|_| GatewayError::BadRequest("invalid lifetime"))?;
    let token = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        // Serialise one user's token changes so the name and count checks hold.
        sqlx::query!("SELECT id FROM users WHERE id=$1 FOR UPDATE", user.id).fetch_one(&mut *transaction).await?;
        let live = sqlx::query!("SELECT count(*) AS \"count!\", bool_or(name=$2) AS taken FROM api_tokens WHERE user_id=$1 AND revoked_at IS NULL", user.id, name)
            .fetch_one(&mut *transaction).await?;
        if live.taken == Some(true) {
            return Err(GatewayError::Conflict("token_name_taken"));
        }
        if live.count >= MAX_TOKENS {
            return Err(GatewayError::Conflict("token_limit"));
        }
        let row = sqlx::query!("INSERT INTO api_tokens (id,user_id,name,hint,token_hash,expires_at) VALUES ($1,$2,$3,$4,$5,CURRENT_TIMESTAMP+make_interval(days=>$6)) RETURNING created_at,expires_at",
            id, user.id, name, hint, hash, days).fetch_one(&mut *transaction).await?;
        transaction.commit().await?;
        Ok(Token { id, name, hint, created_at: row.created_at, expires_at: row.expires_at, last_used_at: None })
    }).await?;
    tracing::info!(user_id = %user.id, token_id = %id, "personal API token created");
    Ok((StatusCode::CREATED, Json(Created { token, secret })))
}

/// `GET /api/v1/tokens`: the caller's own live tokens, newest first.
///
/// # Errors
/// A store failure.
pub(crate) async fn list(
    AppState(state): AppState<Arc<State>>,
    SessionAuthz(user): SessionAuthz,
) -> Result<Json<Tokens>, GatewayError> {
    let items = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query_as!(Token, "SELECT id,name,hint,created_at,expires_at,last_used_at FROM api_tokens WHERE user_id=$1 AND revoked_at IS NULL ORDER BY created_at DESC, id DESC LIMIT $2", user.id, MAX_TOKENS)
            .fetch_all(&state.pool).await?)
    }).await?;
    Ok(Json(Tokens { items }))
}

/// `DELETE /api/v1/tokens/{id}`: revokes one of the caller's tokens at once.
///
/// # Errors
/// `404` for an unknown, revoked or another user's token; a store failure.
pub(crate) async fn revoke(
    AppState(state): AppState<Arc<State>>,
    SessionAuthz(user): SessionAuthz,
    Path(id): Path<String>,
) -> Result<StatusCode, GatewayError> {
    let id = super::path_id(&id)?;
    let revoked = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query_scalar!("UPDATE api_tokens SET revoked_at=CURRENT_TIMESTAMP,updated_at=CURRENT_TIMESTAMP WHERE id=$1 AND user_id=$2 AND revoked_at IS NULL RETURNING id", id, user.id)
            .fetch_optional(&state.pool).await?)
    }).await?;
    if revoked.is_none() {
        return Err(GatewayError::NotFound);
    }
    tracing::info!(user_id = %user.id, token_id = %id, "personal API token revoked");
    Ok(StatusCode::NO_CONTENT)
}
