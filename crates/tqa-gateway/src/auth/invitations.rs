//! Invitations (GW-I-002): an administrator invites one email address with a single-use, expiring link.
//! Only the token's SHA-256 is stored. Every unusable token gets the same `invitation_invalid` answer.
use super::{Role, SessionAuthz, User, policy, sessions, token};
use crate::{GatewayError, app::State, request::ApiJson};
use axum::{
    Json,
    extract::{ConnectInfo, Path, State as AppState},
    http::StatusCode,
};
use axum_extra::extract::CookieJar;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc};

/// Invitations listed at once; older ones age out of the list, never out of the store.
const MAX_LISTED: i64 = 1000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewInvitation {
    email: String,
    role: Role,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Lookup {
    token: SecretString,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Accept {
    token: SecretString,
    password: SecretString,
}

#[derive(Serialize)]
pub(crate) struct Invitation {
    id: uuid::Uuid,
    email: String,
    role: String,
    /// `pending`, `accepted`, `expired` or `revoked`.
    status: String,
    created_by: uuid::Uuid,
    #[serde(with = "time::serde::rfc3339")]
    created_at: time::OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    expires_at: time::OffsetDateTime,
}

#[derive(Serialize)]
pub(crate) struct Created {
    #[serde(flatten)]
    invitation: Invitation,
    /// Shown once; the join page is `/join/{token}`.
    token: String,
}

#[derive(Serialize)]
pub(crate) struct Invitations {
    items: Vec<Invitation>,
}

#[derive(Serialize)]
pub(crate) struct Invitee {
    email: String,
    role: String,
    #[serde(with = "time::serde::rfc3339")]
    expires_at: time::OffsetDateTime,
}

fn admin(user: &User) -> Result<(), GatewayError> {
    if user.role == Role::Admin {
        Ok(())
    } else {
        Err(GatewayError::Forbidden("administrator role is required"))
    }
}

/// A presented invitation token, hashed; malformed input is just another invalid invitation.
fn hashed(secret: &SecretString) -> Result<Vec<u8>, GatewayError> {
    let value = secret.expose_secret();
    if value.is_empty() || value.len() > 128 {
        return Err(GatewayError::InvitationInvalid);
    }
    Ok(token::digest(value))
}

/// `POST /api/v1/invitations` (administrator, session).
///
/// # Errors
/// Not an administrator, an invalid email, an email that already has an account, or a store failure.
pub(crate) async fn create(
    AppState(state): AppState<Arc<State>>,
    SessionAuthz(user): SessionAuthz,
    ApiJson(body): ApiJson<NewInvitation>,
) -> Result<(StatusCode, Json<Created>), GatewayError> {
    admin(&user)?;
    let email = policy::normalize_email(&body.email)?;
    let role = match body.role {
        Role::Admin => "admin",
        Role::Member => "member",
    };
    let secret = token::mint_url_safe()?;
    let hash = token::digest(secret.expose_secret());
    let id = uuid::Uuid::now_v7();
    let hours = i32::try_from(state.config.limits.invitation_hours)
        .map_err(|_| GatewayError::Configuration("invitation lifetime is too large"))?;
    let invitation = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let taken = sqlx::query_scalar!("SELECT EXISTS(SELECT 1 FROM users WHERE email=$1::text::citext)", email)
            .fetch_one(&state.pool).await?.unwrap_or(true);
        if taken {
            return Err(GatewayError::Conflict("email_taken"));
        }
        let row = sqlx::query!("INSERT INTO invitations (id,email,role,token_hash,created_by,expires_at) VALUES ($1,$2::text::citext,$3,$4,$5,CURRENT_TIMESTAMP+make_interval(hours=>$6)) RETURNING created_at,expires_at",
            id, email, role, hash, user.id, hours).fetch_one(&state.pool).await?;
        Ok(Invitation { id, email, role: role.to_owned(), status: "pending".into(), created_by: user.id, created_at: row.created_at, expires_at: row.expires_at })
    }).await?;
    tracing::info!(user_id = %user.id, invitation_id = %id, "invitation created");
    Ok((
        StatusCode::CREATED,
        Json(Created {
            invitation,
            token: secret.expose_secret().to_owned(),
        }),
    ))
}

/// `GET /api/v1/invitations` (administrator, session): newest first, with their status; never a token.
///
/// # Errors
/// Not an administrator, or a store failure.
pub(crate) async fn list(
    AppState(state): AppState<Arc<State>>,
    SessionAuthz(user): SessionAuthz,
) -> Result<Json<Invitations>, GatewayError> {
    admin(&user)?;
    let items = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query_as!(Invitation, "SELECT id, email::text AS \"email!\", role, CASE WHEN accepted_at IS NOT NULL THEN 'accepted' WHEN revoked_at IS NOT NULL THEN 'revoked' WHEN expires_at<=CURRENT_TIMESTAMP THEN 'expired' ELSE 'pending' END AS \"status!\", created_by, created_at, expires_at FROM invitations ORDER BY created_at DESC, id DESC LIMIT $1", MAX_LISTED)
            .fetch_all(&state.pool).await?)
    }).await?;
    Ok(Json(Invitations { items }))
}

/// `DELETE /api/v1/invitations/{id}` (administrator, session): revokes an invitation not yet accepted.
///
/// # Errors
/// Not an administrator, `404` for an unknown one, `409 invitation_closed` for an accepted or revoked one.
pub(crate) async fn revoke(
    AppState(state): AppState<Arc<State>>,
    SessionAuthz(user): SessionAuthz,
    Path(id): Path<String>,
) -> Result<StatusCode, GatewayError> {
    admin(&user)?;
    let id = super::path_id(&id)?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let revoked = sqlx::query_scalar!("UPDATE invitations SET revoked_at=CURRENT_TIMESTAMP,updated_at=CURRENT_TIMESTAMP WHERE id=$1 AND accepted_at IS NULL AND revoked_at IS NULL RETURNING id", id)
            .fetch_optional(&state.pool).await?;
        if revoked.is_some() {
            return Ok(());
        }
        let exists = sqlx::query_scalar!("SELECT EXISTS(SELECT 1 FROM invitations WHERE id=$1)", id)
            .fetch_one(&state.pool).await?.unwrap_or(false);
        Err(if exists { GatewayError::Conflict("invitation_closed") } else { GatewayError::NotFound })
    }).await?;
    tracing::info!(user_id = %user.id, invitation_id = %id, "invitation revoked");
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/v1/invitations/lookup` (public, login-limited): whom a usable invitation is for.
///
/// # Errors
/// `400 invitation_invalid` for any unusable token; `429` from the login limiter.
pub(crate) async fn lookup(
    AppState(state): AppState<Arc<State>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    ApiJson(body): ApiJson<Lookup>,
) -> Result<Json<Invitee>, GatewayError> {
    state
        .login_rate
        .check(peer.ip(), tokio::time::Instant::now())?;
    let hash = hashed(&body.token)?;
    let invitee = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query_as!(Invitee, "SELECT email::text AS \"email!\", role, expires_at FROM invitations WHERE token_hash=$1 AND accepted_at IS NULL AND revoked_at IS NULL AND expires_at>CURRENT_TIMESTAMP", hash)
            .fetch_optional(&state.pool).await?)
    }).await?;
    invitee.map(Json).ok_or(GatewayError::InvitationInvalid)
}

/// `POST /api/v1/invitations/accept` (public, login-limited): creates the invited account with the chosen password
/// and signs it in. The invitation is consumed in the same transaction as the account insert, so it works once.
///
/// # Errors
/// `400 invitation_invalid`, a password policy failure, `429` from the login limiter, or a store failure.
pub(crate) async fn accept(
    AppState(state): AppState<Arc<State>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    ApiJson(body): ApiJson<Accept>,
) -> Result<(StatusCode, CookieJar, Json<super::endpoints::SignedIn>), GatewayError> {
    state
        .login_rate
        .check(peer.ip(), tokio::time::Instant::now())?;
    let hash = hashed(&body.token)?;
    let pending = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query!("SELECT id, email::text AS \"email!\", role, created_by FROM invitations WHERE token_hash=$1 AND accepted_at IS NULL AND revoked_at IS NULL AND expires_at>CURRENT_TIMESTAMP", hash)
            .fetch_optional(&state.pool).await?)
    }).await?.ok_or(GatewayError::InvitationInvalid)?;
    policy::validate_password(body.password.expose_secret(), &pending.email)?;
    let password_hash = state.passwords.hash(body.password).await?;
    let id = uuid::Uuid::now_v7();
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        let taken = sqlx::query_scalar!("SELECT EXISTS(SELECT 1 FROM users WHERE email=$1::text::citext)", pending.email)
            .fetch_one(&mut *transaction).await?.unwrap_or(true);
        if taken {
            return Err(GatewayError::InvitationInvalid);
        }
        sqlx::query!("INSERT INTO users (id,email,password_hash,role,invited_by) VALUES ($1,$2::text::citext,$3,$4,$5)",
            id, pending.email, password_hash, pending.role, pending.created_by).execute(&mut *transaction).await?;
        let consumed = sqlx::query_scalar!("UPDATE invitations SET accepted_at=CURRENT_TIMESTAMP,accepted_by=$2,updated_at=CURRENT_TIMESTAMP WHERE id=$1 AND accepted_at IS NULL AND revoked_at IS NULL AND expires_at>CURRENT_TIMESTAMP RETURNING id", pending.id, id)
            .fetch_optional(&mut *transaction).await?;
        if consumed.is_none() {
            return Err(GatewayError::InvitationInvalid);
        }
        transaction.commit().await?;
        Ok(())
    }).await?;
    let user = User {
        id,
        email: pending.email,
        role: Role::parse(&pending.role)?,
    };
    tracing::info!(user_id = %id, invitation_id = %pending.id, "invitation accepted");
    let cookie = sessions::create(&state, &user).await?;
    Ok((
        StatusCode::CREATED,
        jar.add(cookie),
        super::endpoints::signed_in(&state, user),
    ))
}
