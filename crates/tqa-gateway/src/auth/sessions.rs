use super::{Role, User, token};
use crate::{GatewayError, app::State};
use axum::{
    extract::{ConnectInfo, FromRequestParts},
    http::{header::AUTHORIZATION, request::Parts},
};
use axum_extra::extract::{
    CookieJar,
    cookie::{Cookie, SameSite},
};
use secrecy::ExposeSecret;
use std::{net::SocketAddr, sync::Arc};

/// The caller, by session cookie or by personal API token. A request with an `Authorization` header is
/// authenticated only by that token: a cookie on the same request is ignored, and a bad token is `401`.
pub(crate) struct Authz(pub User);

/// The caller by session cookie only: token and user administration.
pub(crate) struct SessionAuthz(pub User);

/// Personal API tokens carry this prefix; runtime tokens never do, so neither can stand in for the other.
pub(crate) const TOKEN_PREFIX: &str = "tqa_pat_";

impl FromRequestParts<Arc<State>> for Authz {
    type Rejection = GatewayError;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<State>,
    ) -> Result<Self, Self::Rejection> {
        let user = if parts.headers.contains_key(AUTHORIZATION) {
            by_token(parts, state).await?
        } else {
            by_cookie(parts, state).await?
        };
        active(state, user.id).await?;
        Ok(Self(user))
    }
}

impl FromRequestParts<Arc<State>> for SessionAuthz {
    type Rejection = GatewayError;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<State>,
    ) -> Result<Self, Self::Rejection> {
        if parts.headers.contains_key(AUTHORIZATION) {
            return Err(GatewayError::SessionRequired);
        }
        let user = by_cookie(parts, state).await?;
        active(state, user.id).await?;
        Ok(Self(user))
    }
}

/// Records that the account was active, at most once a minute (066a addendum): a no-op update otherwise.
async fn active(state: &State, user: uuid::Uuid) -> Result<(), GatewayError> {
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        sqlx::query!("UPDATE users SET last_active_at=CURRENT_TIMESTAMP WHERE id=$1 AND (last_active_at IS NULL OR last_active_at<CURRENT_TIMESTAMP-INTERVAL '1 minute')", user)
            .execute(&state.pool).await?;
        Ok(())
    }).await
}

async fn by_cookie(parts: &Parts, state: &State) -> Result<User, GatewayError> {
    let jar = CookieJar::from_headers(&parts.headers);
    let value = jar.get("tqa_session").ok_or(GatewayError::Unauthorized)?;
    let hash = token::credential(value.value())?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let record = sqlx::query!(
            "SELECT u.id, u.email::text AS \"email!\", u.role FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.token_hash=$1 AND s.expires_at>CURRENT_TIMESTAMP AND u.disabled_at IS NULL", hash)
            .fetch_optional(&state.pool).await?.ok_or(GatewayError::Unauthorized)?;
        Ok(User { id: record.id, email: record.email, role: Role::parse(&record.role)? })
    }).await
}

/// Authenticates a personal API token. Failures are counted per address; a limited address is refused first.
async fn by_token(parts: &Parts, state: &State) -> Result<User, GatewayError> {
    let address = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip())
        .ok_or(GatewayError::Unauthorized)?;
    let now = tokio::time::Instant::now();
    state.token_failures.check(address, now)?;
    let user = match token::bearer(&parts.headers) {
        Ok(secret) if secret.expose_secret().starts_with(TOKEN_PREFIX) => {
            lookup(state, &token::digest(secret.expose_secret())).await?
        }
        _ => None,
    };
    if let Some(user) = user {
        return Ok(user);
    }
    state.token_failures.fail(address, now)?;
    Err(GatewayError::Unauthorized)
}

async fn lookup(state: &State, hash: &[u8]) -> Result<Option<User>, GatewayError> {
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let Some(record) = sqlx::query!(
            "SELECT t.id AS token_id, u.id, u.email::text AS \"email!\", u.role FROM api_tokens t JOIN users u ON u.id=t.user_id WHERE t.token_hash=$1 AND t.revoked_at IS NULL AND t.expires_at>CURRENT_TIMESTAMP AND u.disabled_at IS NULL", hash)
            .fetch_optional(&state.pool).await? else { return Ok(None) };
        // At most one write a minute per token, not one per request.
        sqlx::query!("UPDATE api_tokens SET last_used_at=CURRENT_TIMESTAMP WHERE id=$1 AND (last_used_at IS NULL OR last_used_at<CURRENT_TIMESTAMP-INTERVAL '1 minute')", record.token_id)
            .execute(&state.pool).await?;
        Ok(Some(User { id: record.id, email: record.email, role: Role::parse(&record.role)? }))
    }).await
}

pub(super) async fn create(state: &State, user: &User) -> Result<Cookie<'static>, GatewayError> {
    let plaintext = token::mint()?;
    let hash = token::digest(plaintext.expose_secret());
    let id = uuid::Uuid::now_v7();
    let seconds = i64::try_from(state.config.session_ttl.as_secs())
        .map_err(|_| GatewayError::Configuration("session TTL is too large"))?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        sqlx::query!("INSERT INTO sessions (id, user_id, token_hash, expires_at) VALUES ($1, $2, $3, CURRENT_TIMESTAMP + $4::bigint * INTERVAL '1 second')", id, user.id, hash, seconds)
            .execute(&state.pool).await?;
        Ok(())
    }).await?;
    Ok(cookie(state, plaintext.expose_secret().to_owned(), seconds))
}

pub(super) async fn revoke(
    state: &State,
    jar: &CookieJar,
) -> Result<Cookie<'static>, GatewayError> {
    if let Some(session) = jar.get("tqa_session") {
        let hash = token::credential(session.value())?;
        crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
            sqlx::query!("DELETE FROM sessions WHERE token_hash=$1", hash)
                .execute(&state.pool)
                .await?;
            Ok(())
        })
        .await?;
    }
    Ok(cookie(state, String::new(), 0))
}

fn cookie(state: &State, value: String, seconds: i64) -> Cookie<'static> {
    Cookie::build(("tqa_session", value))
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(state.config.public_origin.scheme() == "https")
        .path("/")
        .max_age(time::Duration::seconds(seconds))
        .build()
}
