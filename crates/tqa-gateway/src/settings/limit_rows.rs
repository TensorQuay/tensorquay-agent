//! Limit overrides in the database (ADR-0028 slice 5): one row per overridable limit, seeded from the file's values under
//! the seed-change rule. At load, a value a file edit left out of bounds is clamped, audited as a seed change, logged,
//! and marked (`clamped_from`) until an administrator sets it again. Administrators replace a value or reset it to the
//! file's; the whole resulting set passes `limits::check` first.
use super::{
    Change, KeyedDigest, audit,
    limits::{LimitKey, Limits, check_write, clamp},
};
use crate::{GatewayError, InvalidReason};
use sqlx::PgConnection;
use subtle::ConstantTimeEq;

/// One stored limit with its administration metadata.
#[derive(Clone, Debug)]
pub(crate) struct StoredLimit {
    pub key: LimitKey,
    pub value: u64,
    pub version: i64,
    pub last_set_by: String,
    pub seed_state: Option<String>,
    /// The value the last load moved into the file's bounds, until an administrator sets a new one.
    pub clamped_from: Option<u64>,
    pub updated_at: time::OffsetDateTime,
}

fn stored_value(value: i64) -> Result<u64, GatewayError> {
    u64::try_from(value).map_err(|_| GatewayError::Configuration("a stored limit is negative"))
}

fn database_value(value: u64) -> Result<i64, GatewayError> {
    i64::try_from(value).map_err(|_| GatewayError::Invalid {
        field: "value",
        reason: InvalidReason::LimitOutOfRange,
    })
}

/// Applies the file's limits as the seed, then clamps every stored value into the file's current bounds, in the
/// caller's settings transaction; returns whether it wrote.
///
/// # Errors
/// Storage failure.
pub(crate) async fn seed(
    connection: &mut PgConnection,
    file: &Limits,
    max_live: u32,
    digest: &KeyedDigest,
) -> Result<bool, GatewayError> {
    let mut wrote = false;
    for key in LimitKey::ALL {
        wrote |= seed_limit(connection, key, file.get(key), digest).await?;
    }
    let (_, clamped) = clamp(effective(connection).await?, max_live);
    for change in clamped {
        let current = sqlx::query_scalar!(
            "UPDATE settings SET value=$2,version=version+1,last_set_by='seed',clamped_from=$3,updated_at=CURRENT_TIMESTAMP \
             WHERE key=$1 AND user_id IS NULL RETURNING version",
            change.key.as_str(),
            database_value(change.to)?,
            database_value(change.from)?
        )
        .fetch_one(&mut *connection)
        .await?;
        let record = Change {
            area: "limits",
            actor: None,
            item: change.key.as_str(),
            action: "replace",
            before: Some(current - 1),
            after: Some(current),
            value_changed: true,
        };
        audit(connection, &record).await?;
        tracing::warn!(
            key = change.key.as_str(),
            from = change.from,
            to = change.to,
            "a limit was lowered or raised to fit the configuration file"
        );
        wrote = true;
    }
    Ok(wrote)
}

/// Imports one limit's file value when it is new or changed since the last import; returns whether it wrote.
async fn seed_limit(
    connection: &mut PgConnection,
    key: LimitKey,
    value: u64,
    digest: &KeyedDigest,
) -> Result<bool, GatewayError> {
    let seed = digest.of(&format!("limits/{}", key.as_str()), &value.to_be_bytes());
    let stored = database_value(value)?;
    let current = sqlx::query!(
        "SELECT version,value,seed_digest,seed_state FROM settings WHERE key=$1 AND user_id IS NULL FOR UPDATE",
        key.as_str()
    )
    .fetch_optional(&mut *connection)
    .await?;
    let Some(current) = current else {
        sqlx::query!(
            "INSERT INTO settings (id,key,value,version,last_set_by,seed_digest,seed_state) VALUES ($1,$2,$3,1,'seed',$4,'current')",
            uuid::Uuid::now_v7(), key.as_str(), stored, &seed[..]
        )
        .execute(&mut *connection)
        .await?;
        let change = Change {
            area: "limits",
            actor: None,
            item: key.as_str(),
            action: "create",
            before: None,
            after: Some(1),
            value_changed: true,
        };
        audit(connection, &change).await?;
        return Ok(true);
    };
    if current
        .seed_digest
        .as_deref()
        .is_some_and(|stored| bool::from(stored.ct_eq(&seed[..])))
    {
        return Ok(false);
    }
    let version = current.version + 1;
    sqlx::query!(
        "UPDATE settings SET value=$2,version=$3,last_set_by='seed',seed_digest=$4,seed_state='current',clamped_from=NULL,\
         updated_at=CURRENT_TIMESTAMP WHERE key=$1 AND user_id IS NULL",
        key.as_str(), stored, version, &seed[..]
    )
    .execute(&mut *connection)
    .await?;
    let change = Change {
        area: "limits",
        actor: None,
        item: key.as_str(),
        action: "replace",
        before: Some(current.version),
        after: Some(version),
        value_changed: current.value != stored,
    };
    audit(connection, &change).await?;
    Ok(true)
}

/// Every stored limit, in the API's order.
///
/// # Errors
/// Storage failure, or a stored key or value the gateway does not know.
pub(crate) async fn rows(connection: &mut PgConnection) -> Result<Vec<StoredLimit>, GatewayError> {
    let rows = sqlx::query!(
        "SELECT key,value,version,last_set_by,seed_state,clamped_from,updated_at FROM settings WHERE user_id IS NULL"
    )
    .fetch_all(connection)
    .await?;
    let mut limits = rows
        .into_iter()
        .map(|row| {
            Ok(StoredLimit {
                key: LimitKey::parse(&row.key)
                    .ok_or(GatewayError::Configuration("a stored limit key is unknown"))?,
                value: stored_value(row.value)?,
                version: row.version,
                last_set_by: row.last_set_by,
                seed_state: row.seed_state,
                clamped_from: row.clamped_from.map(stored_value).transpose()?,
                updated_at: row.updated_at,
            })
        })
        .collect::<Result<Vec<_>, GatewayError>>()?;
    limits.sort_by_key(|limit| LimitKey::ALL.iter().position(|key| *key == limit.key));
    Ok(limits)
}

/// The effective limits, from every stored row.
///
/// # Errors
/// Storage failure, or a limit with no row (the load seeds every key).
pub(crate) async fn effective(connection: &mut PgConnection) -> Result<Limits, GatewayError> {
    let stored = rows(connection).await?;
    let mut limits = Limits {
        slots: 0,
        per_user: 0,
        idle_seconds: 0,
        max_lifetime_seconds: 0,
        mcp_calls_per_minute: 0,
        team_max_members: 0,
        team_token_budget: 0,
        team_deadline_minutes: 0,
        member_idle_minutes: 0,
        member_stuck_minutes: 0,
        team_idle_stop_minutes: 0,
        desktop_max_sessions: 0,
        desktop_screenshot_retention_days: 0,
        computer_max_screenshots_per_run: 0,
        computer_max_actions_per_run: 0,
    };
    for key in LimitKey::ALL {
        let row = stored
            .iter()
            .find(|row| row.key == key)
            .ok_or(GatewayError::Configuration("a limit has no stored value"))?;
        limits = limits.with(key, row.value);
    }
    Ok(limits)
}

/// Sets `key` to `value` (an administrator's write, or a reset to the file's value when `actor` resets), after checking
/// the whole resulting set, in the caller's settings transaction. Returns the stored limit.
///
/// # Errors
/// `400 invalid_request` for a value or order the bounds refuse, `409 conflict` for a stale version, storage failure.
pub(crate) async fn write(
    connection: &mut PgConnection,
    write: LimitWrite,
) -> Result<StoredLimit, GatewayError> {
    let current = sqlx::query!(
        "SELECT version,value FROM settings WHERE key=$1 AND user_id IS NULL FOR UPDATE",
        write.key.as_str()
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(GatewayError::NotFound)?;
    if current.version != write.expected {
        return Err(GatewayError::Conflict("conflict"));
    }
    let next = effective(connection).await?.with(write.key, write.value);
    check_write(&next, write.key, write.max_live).map_err(|(key, reason)| {
        GatewayError::Invalid {
            field: if key == write.key { "value" } else { "limits" },
            reason,
        }
    })?;
    let stored = database_value(write.value)?;
    let last_set_by = if write.reset { "seed" } else { "admin" };
    sqlx::query!(
        "UPDATE settings SET value=$2,version=version+1,last_set_by=$3,clamped_from=NULL,updated_at=CURRENT_TIMESTAMP \
         WHERE key=$1 AND user_id IS NULL",
        write.key.as_str(),
        stored,
        last_set_by
    )
    .execute(&mut *connection)
    .await?;
    let change = Change {
        area: "limits",
        actor: Some(write.actor),
        item: write.key.as_str(),
        action: "replace",
        before: Some(current.version),
        after: Some(current.version + 1),
        value_changed: current.value != stored,
    };
    audit(connection, &change).await?;
    tracing::info!(
        key = write.key.as_str(),
        version = current.version + 1,
        "limit updated by an administrator"
    );
    rows(connection)
        .await?
        .into_iter()
        .find(|limit| limit.key == write.key)
        .ok_or(GatewayError::StoreUnavailable)
}

/// One administrator write of a limit.
pub(crate) struct LimitWrite {
    pub key: LimitKey,
    pub value: u64,
    /// The version read; limits always exist, so a write always names one.
    pub expected: i64,
    pub actor: uuid::Uuid,
    /// The file-only bound of `admission.slots`.
    pub max_live: u32,
    /// A reset to the file's value: recorded as the seed's value again.
    pub reset: bool,
}
