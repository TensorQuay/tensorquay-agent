//! `tqa-gateway reset-password <email> [gateway.toml]` (decided on 21 Nov 2026): an administrator on the host sets a
//! person's password when no one can sign in to do it from the page, for example the only administrator who forgot
//! theirs. The new password comes on standard input, never on the command line (it would show in the process list
//! and the shell's history), and meets the same rules as a person's own change. The person's sessions end, and the
//! change is audited as the gateway's own, without the password. Personal API tokens are left as they are, as a
//! person's own change leaves them.
use super::{password::Passwords, policy};
use crate::{GatewayConfig, GatewayError, GatewaySecrets};
use secrecy::{ExposeSecret, SecretString};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

/// Sets the password of the active account `email` to `password`, ends its sessions and audits the change; answers the
/// account's email as stored.
///
/// # Errors
/// `user_not_found` for no active account with that email; `400` naming `new_password` when it is too weak; a storage
/// or connection failure.
pub async fn reset_password(
    config: &GatewayConfig,
    secrets: &GatewaySecrets,
    email: &str,
    password: SecretString,
    cancel: &CancellationToken,
) -> Result<String, GatewayError> {
    let pool = Box::pin(crate::db::connect(config, secrets, cancel)).await?;
    let reset = reset_password_in(&pool, config, email, password, cancel).await;
    pool.close().await;
    reset
}

/// [`reset_password`] on an open pool (the gateway's tests use their own database).
///
/// # Errors
/// As [`reset_password`].
pub async fn reset_password_in(
    pool: &sqlx::PgPool,
    config: &GatewayConfig,
    email: &str,
    password: SecretString,
    cancel: &CancellationToken,
) -> Result<String, GatewayError> {
    let account = crate::io::bounded(config.io_timeout(), cancel, async {
        Ok(sqlx::query!(
            "SELECT id, email::text AS \"email!\" FROM users WHERE email = $1::text::citext AND disabled_at IS NULL",
            email
        )
        .fetch_optional(pool)
        .await?)
    })
    .await?
    .ok_or(GatewayError::UserNotFound)?;
    policy::validate_password(password.expose_secret(), &account.email).map_err(|_| {
        GatewayError::Invalid {
            field: "new_password",
            reason: crate::InvalidReason::PasswordWeak,
        }
    })?;
    let tasks = TaskTracker::new();
    let hash = Passwords::new(config, tasks.clone(), cancel.clone())
        .hash(password)
        .await?;
    tasks.close();
    crate::io::bounded(config.io_timeout(), cancel, async {
        let mut transaction = pool.begin().await?;
        sqlx::query!(
            "UPDATE users SET password_hash = $2, updated_at = CURRENT_TIMESTAMP WHERE id = $1",
            account.id,
            hash
        )
        .execute(&mut *transaction)
        .await?;
        sqlx::query!("DELETE FROM sessions WHERE user_id = $1", account.id)
            .execute(&mut *transaction)
            .await?;
        crate::settings::audit_system(
            &mut transaction,
            "account",
            &account.id.to_string(),
            "replace",
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    })
    .await?;
    tasks.wait().await;
    tracing::info!(user_id = %account.id, "password reset from the host; sessions ended");
    Ok(account.email)
}
