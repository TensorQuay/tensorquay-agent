use super::{Role, User, policy, token};
use crate::{GatewayError, app::State};
use secrecy::{ExposeSecret, SecretString};
use subtle::ConstantTimeEq;
use uuid::Uuid;

// All bootstrap attempts use one transaction lock, including attempts handled by another process.
const BOOTSTRAP_LOCK: i64 = 0x0074_7161_626f_6f74;

/// Whether the install still needs its first administrator.
///
/// # Errors
/// Fails on a bounded storage error.
pub(super) async fn setup_required(state: &State) -> Result<bool, GatewayError> {
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let existing = sqlx::query_scalar!("SELECT EXISTS(SELECT 1 FROM users)")
            .fetch_one(&state.pool)
            .await?
            .unwrap_or(true);
        Ok(!existing)
    })
    .await
}

pub(super) async fn bootstrap(
    state: &State,
    email: &str,
    password: SecretString,
    supplied: &SecretString,
) -> Result<User, GatewayError> {
    let configured = state
        .secrets
        .bootstrap_token
        .as_ref()
        .ok_or(GatewayError::Conflict("bootstrap_closed"))?;
    let matches =
        token::digest(configured.expose_secret()).ct_eq(&token::digest(supplied.expose_secret()));
    if !bool::from(matches) {
        return Err(GatewayError::Unauthorized);
    }
    let email = policy::normalize_email(email)?;
    policy::validate_password(password.expose_secret(), &email)?;
    let hash = state.passwords.hash(password).await?;
    let id = Uuid::now_v7();
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        crate::db::queue(&mut transaction, crate::db::Queue::Key(BOOTSTRAP_LOCK)).await?;
        let existing = sqlx::query_scalar!("SELECT EXISTS(SELECT 1 FROM users)").fetch_one(&mut *transaction).await?.unwrap_or(true);
        if existing { return Err(GatewayError::Conflict("bootstrap_closed")); }
        sqlx::query!("INSERT INTO users (id, email, password_hash, role) VALUES ($1, $2::text::citext, $3, 'admin')", id, email, hash)
            .execute(&mut *transaction).await?;
        transaction.commit().await?;
        Ok(User { id, email, role: Role::Admin })
    }).await
}

pub(super) async fn authenticate(
    state: &State,
    email: &str,
    password: SecretString,
) -> Result<User, GatewayError> {
    let email = policy::normalize_email(email)?;
    if password.expose_secret().len() > 1024 {
        return Err(GatewayError::Unauthorized);
    }
    let record = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        sqlx::query!("SELECT id, email::text AS \"email!\", password_hash, role, disabled_at IS NOT NULL AS \"disabled!\" FROM users WHERE email = $1::text::citext", email)
            .fetch_optional(&state.pool).await.map_err(GatewayError::from)
    }).await?;
    let hash = record.as_ref().map_or_else(
        || state.dummy_hash.clone(),
        |record| record.password_hash.clone(),
    );
    let valid = state.passwords.verify(password, hash).await?;
    if !valid {
        return Err(GatewayError::Unauthorized);
    }
    // A disabled account is refused exactly like a wrong password, after the same verification work.
    let record = record
        .filter(|record| !record.disabled)
        .ok_or(GatewayError::Unauthorized)?;
    Ok(User {
        id: record.id,
        email: record.email,
        role: Role::parse(&record.role)?,
    })
}

/// Changes `user`'s own password (GW-I-124): the current one must verify, and the new one meets the rules the first
/// administrator's password met and differs from it. The session whose token hashes to `keep` stays; the user's other
/// sessions end in the same transaction, which also writes the audit row. Personal API tokens are left as they are.
///
/// # Errors
/// `400` naming `current_password` or `new_password`, or a bounded storage failure.
pub(super) async fn change_password(
    state: &State,
    user: &User,
    current: SecretString,
    new: SecretString,
    keep: &[u8],
) -> Result<(), GatewayError> {
    let refused = |field, reason| GatewayError::Invalid { field, reason };
    let incorrect = || refused("current_password", crate::InvalidReason::PasswordIncorrect);
    if current.expose_secret().len() > 1024 {
        return Err(incorrect());
    }
    let unchanged = current.expose_secret() == new.expose_secret();
    let stored = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(
            sqlx::query_scalar!("SELECT password_hash FROM users WHERE id=$1", user.id)
                .fetch_one(&state.pool)
                .await?,
        )
    })
    .await?;
    if !state.passwords.verify(current, stored.clone()).await? {
        return Err(incorrect());
    }
    policy::validate_password(new.expose_secret(), &user.email)
        .map_err(|_| refused("new_password", crate::InvalidReason::PasswordWeak))?;
    if unchanged {
        return Err(refused(
            "new_password",
            crate::InvalidReason::PasswordUnchanged,
        ));
    }
    let hash = state.passwords.hash(new).await?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        // Only over the hash just verified: a concurrent change makes this one's current password wrong.
        let changed = sqlx::query!("UPDATE users SET password_hash=$2, updated_at=CURRENT_TIMESTAMP WHERE id=$1 AND password_hash=$3", user.id, hash, stored)
            .execute(&mut *transaction).await?.rows_affected();
        if changed != 1 {
            return Err(incorrect());
        }
        sqlx::query!("DELETE FROM sessions WHERE user_id=$1 AND token_hash<>$2", user.id, keep)
            .execute(&mut *transaction).await?;
        crate::settings::audit_user(&mut transaction, user.id, "account", "password", "replace").await?;
        transaction.commit().await?;
        Ok(())
    })
    .await?;
    tracing::info!(user_id = %user.id, "password changed; other sessions ended");
    Ok(())
}
