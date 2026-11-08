//! A run's placeholders (ADR-0037 §3): per run and key, `tqa_ph_` and 32 random bytes, stored only as their hash with
//! where they may be used. Minted when a turn is delivered, bound to the run where its `run.started` is stored, and
//! removed with the run's end, an undelivered turn, or the sandbox's credential.
use super::run_keys::RunKey;
use crate::GatewayError;
use base64::Engine as _;
use rand::TryRngCore as _;
use secrecy::SecretString;
use sqlx::PgConnection;
use uuid::Uuid;

/// What a placeholder is bound to, as the proxy checks it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Bound {
    /// None once the key is deleted, or its sender may no longer use it: a use is then `revoked`.
    pub(crate) key_id: Option<Uuid>,
    /// The key's value's store reference, while the key exists.
    pub(crate) secret_ref: Option<String>,
    pub(crate) conversation_id: Uuid,
    pub(crate) sandbox_id: String,
    /// None until the runtime starts the run the turn was delivered for.
    pub(crate) run_id: Option<String>,
    /// Whether that run has ended.
    pub(crate) ended: bool,
    pub(crate) sender_id: Uuid,
    pub(crate) name: String,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) path_prefix: Option<String>,
    pub(crate) scheme: String,
    pub(crate) user_name: Option<String>,
}

fn hash(placeholder: &str) -> Vec<u8> {
    crate::auth::token::digest(placeholder)
}

fn placeholder() -> Result<SecretString, GatewayError> {
    let mut bytes = [0_u8; 32];
    rand::rngs::OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| GatewayError::SecretUnavailable)?;
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    Ok(SecretString::from(format!("tqa_ph_{encoded}")))
}

/// Mints one placeholder per key for a turn delivered to `sandbox` of `conversation` on behalf of `sender`, replacing
/// the task's placeholders that no run took.
///
/// # Errors
/// Storage failure, or no randomness.
pub(crate) async fn mint(
    connection: &mut PgConnection,
    (conversation, sandbox, sender): (Uuid, &str, Uuid),
    delivery: Uuid,
    keys: &[RunKey],
) -> Result<Vec<(RunKey, SecretString)>, GatewayError> {
    supersede(&mut *connection, conversation).await?;
    insert(
        connection,
        (conversation, sandbox, sender),
        (delivery, None),
        keys,
    )
    .await
}

/// Mints new placeholders for the keys of `run`, already going, in `sandbox`: what its runtime gives the run when it
/// resumes after registering again (GW-I-165). The run's earlier placeholders stay bound to it.
///
/// # Errors
/// Storage failure, or no randomness.
pub(crate) async fn mint_for_run(
    connection: &mut PgConnection,
    (conversation, sandbox, sender): (Uuid, &str, Uuid),
    run: &str,
    keys: &[RunKey],
) -> Result<Vec<(RunKey, SecretString)>, GatewayError> {
    insert(
        connection,
        (conversation, sandbox, sender),
        (Uuid::now_v7(), Some(run)),
        keys,
    )
    .await
}

async fn insert(
    connection: &mut PgConnection,
    (conversation, sandbox, sender): (Uuid, &str, Uuid),
    (delivery, run): (Uuid, Option<&str>),
    keys: &[RunKey],
) -> Result<Vec<(RunKey, SecretString)>, GatewayError> {
    let mut minted = Vec::with_capacity(keys.len());
    for key in keys {
        let placeholder = placeholder()?;
        sqlx::query!(
            "INSERT INTO host_key_placeholders (hash, key_id, conversation_id, sandbox_id, delivery_id, run_id, sender_id, \
             name, host, port, path_prefix, scheme, user_name) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
            hash(secrecy::ExposeSecret::expose_secret(&placeholder)),
            key.id,
            conversation,
            sandbox,
            delivery,
            run,
            sender,
            key.name,
            key.host,
            i32::from(key.port),
            key.path_prefix,
            key.scheme,
            key.user_name
        )
        .execute(&mut *connection)
        .await?;
        minted.push((key.clone(), placeholder));
    }
    Ok(minted)
}

/// The run going in the task, if it holds keys: its id, its sender and the keys it was given that still exist.
///
/// # Errors
/// Storage failure.
pub(crate) async fn in_progress(
    connection: &mut PgConnection,
    conversation: Uuid,
) -> Result<Option<(String, Uuid, Vec<Uuid>)>, GatewayError> {
    let rows = sqlx::query!(
        "SELECT run_id AS \"run!\", sender_id, coalesce(array_agg(DISTINCT key_id) FILTER (WHERE key_id IS NOT NULL), \
         '{}') AS \"keys!\" FROM host_key_placeholders WHERE conversation_id=$1 AND run_id IS NOT NULL \
         AND ended_at IS NULL GROUP BY run_id, sender_id LIMIT 2",
        conversation
    )
    .fetch_all(connection)
    .await?;
    // One run at a time holds a task's placeholders; anything else is left alone.
    Ok(match rows.as_slice() {
        [row] => Some((row.run.clone(), row.sender_id, row.keys.clone())),
        _ => None,
    })
}

/// Removes the task's placeholders that no run took: one turn is in flight at a time, so an earlier turn's (a busy
/// refusal, a turn lost with the link) are never bound to the next one's run.
///
/// # Errors
/// Storage failure.
pub(crate) async fn supersede(
    connection: &mut PgConnection,
    conversation: Uuid,
) -> Result<(), GatewayError> {
    sqlx::query!(
        "DELETE FROM host_key_placeholders WHERE conversation_id=$1 AND run_id IS NULL",
        conversation
    )
    .execute(connection)
    .await?;
    Ok(())
}

/// What `placeholder` is bound to; None for one the gateway doesn't hold.
///
/// # Errors
/// Storage failure.
pub(crate) async fn find(
    connection: &mut PgConnection,
    placeholder: &str,
) -> Result<Option<Bound>, GatewayError> {
    // The key as it is now, not as it was at minting: its address and form, and whether the
    // sender may still use it (its owner, or one of a shared key's people). A key gone or no longer the sender's reads
    // as gone (no `key_id`), so a use is `revoked`; while the key exists its value's reference stays, so that refusal
    // is still a counted row (ADR-0037's R4). The value itself is never read for it.
    let row = sqlx::query!(
        "SELECT CASE WHEN usable THEN p.key_id END AS \"key_id?\", k.secret_ref \
         AS \"secret_ref?\", p.conversation_id, p.sandbox_id, p.run_id, p.ended_at IS NOT NULL AS \"ended!\", \
         p.sender_id, p.name, COALESCE(k.host, p.host) AS \"host!\", COALESCE(k.port, p.port) AS \"port!\", \
         CASE WHEN k.id IS NULL THEN p.path_prefix ELSE k.path_prefix END AS path_prefix, \
         COALESCE(k.scheme, p.scheme) AS \"scheme!\", \
         CASE WHEN k.id IS NULL THEN p.user_name ELSE k.user_name END AS user_name \
         FROM host_key_placeholders p LEFT JOIN host_keys k ON k.id=p.key_id \
         CROSS JOIN LATERAL (SELECT k.id IS NOT NULL AND (k.owner_id=p.sender_id OR (k.owner_id IS NULL AND \
         (k.for_everyone OR EXISTS (SELECT 1 FROM host_key_users u WHERE u.key_id=k.id AND u.user_id=p.sender_id))))\
         AS usable) m WHERE p.hash=$1",
        hash(placeholder)
    )
    .fetch_optional(connection)
    .await?;
    row.map(|row| {
        Ok(Bound {
            key_id: row.key_id,
            secret_ref: row.secret_ref,
            conversation_id: row.conversation_id,
            sandbox_id: row.sandbox_id,
            run_id: row.run_id,
            ended: row.ended,
            sender_id: row.sender_id,
            name: row.name,
            host: row.host,
            port: u16::try_from(row.port).map_err(|_| GatewayError::StoreUnavailable)?,
            path_prefix: row.path_prefix,
            scheme: row.scheme,
            user_name: row.user_name,
        })
    })
    .transpose()
}

/// Binds the task's placeholders that have no run yet to the run the runtime just started; a resume changes nothing.
///
/// # Errors
/// Storage failure.
pub(crate) async fn bind_run(
    connection: &mut PgConnection,
    conversation: Uuid,
    run: &str,
) -> Result<(), GatewayError> {
    sqlx::query!(
        "UPDATE host_key_placeholders SET run_id=$2, updated_at=CURRENT_TIMESTAMP WHERE conversation_id=$1 \
         AND run_id IS NULL",
        conversation,
        run
    )
    .execute(connection)
    .await?;
    Ok(())
}

/// Ends a run's placeholders at its end: kept a day, so a later use reads `expired` or `foreign_run` rather than
/// `absent`; [`sweep`] removes them after that.
///
/// # Errors
/// Storage failure.
pub(crate) async fn end_run(
    connection: &mut PgConnection,
    conversation: Uuid,
    run: &str,
) -> Result<(), GatewayError> {
    sqlx::query!(
        "UPDATE host_key_placeholders SET ended_at=CURRENT_TIMESTAMP, updated_at=CURRENT_TIMESTAMP WHERE conversation_id=$1 AND run_id=$2 \
         AND ended_at IS NULL",
        conversation,
        run
    )
    .execute(connection)
    .await?;
    Ok(())
}

/// The ended placeholders removed per statement, so a sweep never holds a long lock.
const SWEEP_BATCH: i64 = 1000;

/// Removes placeholders whose run ended more than a day ago, in bounded batches; run hourly with the audit retention
/// sweep. Returns how many it removed.
///
/// # Errors
/// A store failure, a deadline or cancellation.
pub(crate) async fn sweep(state: &crate::app::State) -> Result<u64, GatewayError> {
    let mut removed = 0;
    loop {
        let deleted = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
            let mut connection = state.pool.acquire().await?;
            sweep_batch(&mut connection, SWEEP_BATCH).await
        })
        .await?;
        removed += deleted;
        if deleted == 0 {
            return Ok(removed);
        }
    }
}

/// Removes at most `batch` placeholders whose run ended more than a day ago; how many.
///
/// # Errors
/// A store failure.
pub(crate) async fn sweep_batch(
    connection: &mut PgConnection,
    batch: i64,
) -> Result<u64, GatewayError> {
    Ok(sqlx::query!(
        "DELETE FROM host_key_placeholders WHERE hash IN (SELECT hash FROM host_key_placeholders \
         WHERE ended_at < CURRENT_TIMESTAMP - interval '1 day' ORDER BY ended_at LIMIT $1)",
        batch
    )
    .execute(connection)
    .await?
    .rows_affected())
}

/// Removes a turn's placeholders when the runtime never took it; a started run's stay.
///
/// # Errors
/// Storage failure.
pub(crate) async fn drop_delivery(
    connection: &mut PgConnection,
    delivery: Uuid,
) -> Result<(), GatewayError> {
    sqlx::query!(
        "DELETE FROM host_key_placeholders WHERE delivery_id=$1 AND run_id IS NULL",
        delivery
    )
    .execute(connection)
    .await?;
    Ok(())
}

/// Removes a sandbox's untaken placeholders when its credential is revoked or replaced. A run's stay until it ends:
/// the proxy refuses them in any other sandbox (`foreign_sandbox`), and the run gets new ones when its runtime
/// registers again.
///
/// # Errors
/// Storage failure.
pub(crate) async fn end_sandbox(
    connection: &mut PgConnection,
    sandbox: &str,
) -> Result<(), GatewayError> {
    sqlx::query!(
        "DELETE FROM host_key_placeholders WHERE sandbox_id=$1 AND run_id IS NULL",
        sandbox
    )
    .execute(connection)
    .await?;
    Ok(())
}

/// Removes the placeholders of deleted tasks.
///
/// # Errors
/// Storage failure.
pub(crate) async fn end_tasks(
    connection: &mut PgConnection,
    conversations: &[Uuid],
) -> Result<(), GatewayError> {
    sqlx::query!(
        "DELETE FROM host_key_placeholders WHERE conversation_id=ANY($1)",
        conversations
    )
    .execute(connection)
    .await?;
    Ok(())
}

#[cfg(test)]
#[path = "placeholders_tests.rs"]
mod tests;
