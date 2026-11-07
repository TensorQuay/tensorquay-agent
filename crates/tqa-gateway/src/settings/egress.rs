//! Egress rules in the database (ADR-0028): `[[egress.allow]]` seeds them under the seed-change rule, administrators
//! change them through the API, and the snapshot's allow-list is built from the stored rules. Every rule passes the
//! one validator the file uses (`egress::canonical`) and is stored in its canonical form, keyed by its host pattern.
use super::{Change, KeyedDigest, audit, tombstones};
use crate::{
    GatewayError,
    egress::{AllowList, RuleInput},
};
use sqlx::PgConnection;
use subtle::ConstantTimeEq;

/// Rules the allow-list may hold, as the file allows.
pub(crate) const MAX_RULES: i64 = 256;

/// One stored rule with its administration metadata.
#[derive(Clone, Debug)]
pub(crate) struct StoredRule {
    /// The canonical host pattern: an exact name or address, or `*.domain`.
    pub host: String,
    /// Sorted, distinct ports.
    pub ports: Vec<u16>,
    /// The administrator lets this exact host name reach a private address (ADR-0035 amendment).
    pub internal: bool,
    pub version: i64,
    /// `seed` or `admin`: who wrote the current rule.
    pub last_set_by: String,
    /// `current` while the file lists it, `removed` after it no longer does; `None` if never seeded.
    pub seed_state: Option<String>,
    /// The internet-access preset that added it, until an administrator edits it (then it is theirs).
    pub preset: Option<String>,
    pub updated_at: time::OffsetDateTime,
}

/// One administrator write of a canonical rule.
pub(crate) struct RuleWrite {
    pub rule: RuleInput,
    /// The version read; `None` creates, and must then find no rule for the host.
    pub expected: Option<i64>,
    pub actor: uuid::Uuid,
}

fn ports(rule: &RuleInput) -> Vec<i32> {
    rule.ports.iter().copied().map(i32::from).collect()
}

fn from_stored(ports: Vec<i32>) -> Result<Vec<u16>, GatewayError> {
    ports
        .into_iter()
        .map(u16::try_from)
        .collect::<Result<_, _>>()
        .map_err(|_| GatewayError::Configuration("a stored egress rule is invalid"))
}

/// Applies the file's canonical rules in the caller's settings transaction; returns whether it wrote.
///
/// # Errors
/// Storage failure.
pub(crate) async fn seed(
    connection: &mut PgConnection,
    rules: &[RuleInput],
    digest: &KeyedDigest,
) -> Result<bool, GatewayError> {
    let mut wrote = false;
    for rule in rules {
        wrote |= seed_rule(connection, rule, digest).await?;
    }
    let hosts: Vec<String> = rules.iter().map(|rule| rule.host.clone()).collect();
    tombstones::forget_unlisted(connection, "egress", &hosts).await?;
    let removed = sqlx::query!(
        "UPDATE egress_rules SET seed_state='removed' WHERE seed_state='current' AND NOT (host = ANY($1))",
        &hosts
    )
    .execute(&mut *connection)
    .await?;
    tracing::info!(rules = rules.len(), "egress rules seed applied");
    Ok(wrote || removed.rows_affected() > 0)
}

/// Imports one seed rule when it is new or its seed record changed; returns whether it wrote anything.
async fn seed_rule(
    connection: &mut PgConnection,
    rule: &RuleInput,
    digest: &KeyedDigest,
) -> Result<bool, GatewayError> {
    let ports = ports(rule);
    // The record a change is detected by: the ports, and `internal` only when set, so every rule seeded before the
    // switch existed keeps its digest (ENGINEERING-GUARDRAILS §8).
    let record = if rule.internal {
        serde_json::to_vec(&serde_json::json!({"ports": rule.ports, "internal": true}))?
    } else {
        serde_json::to_vec(&rule.ports)?
    };
    let seed = digest.of(&format!("egress/{}", rule.host), &record);
    let current = sqlx::query!(
        "SELECT version,ports,internal,seed_digest,seed_state FROM egress_rules WHERE host=$1 FOR UPDATE",
        rule.host
    )
    .fetch_optional(&mut *connection)
    .await?;
    let Some(current) = current else {
        if tombstones::buried(connection, "egress", &rule.host, &seed).await? {
            return Ok(false);
        }
        sqlx::query!(
            "INSERT INTO egress_rules (host,ports,internal,version,last_set_by,seed_digest,seed_state) VALUES ($1,$2,$3,1,'seed',$4,'current')",
            rule.host, &ports, rule.internal, &seed[..]
        )
        .execute(&mut *connection)
        .await?;
        let change = Change {
            area: "egress",
            actor: None,
            item: &rule.host,
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
        if current.seed_state.as_deref() == Some("current") {
            return Ok(false);
        }
        sqlx::query!(
            "UPDATE egress_rules SET seed_state='current' WHERE host=$1",
            rule.host
        )
        .execute(&mut *connection)
        .await?;
        return Ok(true);
    }
    let version = current.version + 1;
    sqlx::query!(
        "UPDATE egress_rules SET ports=$2,internal=$5,version=$3,last_set_by='seed',seed_digest=$4,seed_state='current',updated_at=CURRENT_TIMESTAMP WHERE host=$1",
        rule.host, &ports, version, &seed[..], rule.internal
    )
    .execute(&mut *connection)
    .await?;
    let change = Change {
        area: "egress",
        actor: None,
        item: &rule.host,
        action: "replace",
        before: Some(current.version),
        after: Some(version),
        value_changed: current.ports != ports || current.internal != rule.internal,
    };
    audit(connection, &change).await?;
    Ok(true)
}

/// Every stored rule, by host.
///
/// # Errors
/// Storage failure, or a stored rule out of range.
pub(crate) async fn rules(connection: &mut PgConnection) -> Result<Vec<StoredRule>, GatewayError> {
    let rows = sqlx::query!(
        "SELECT host,ports,internal,version,last_set_by,seed_state,updated_at,CASE WHEN preset_edited THEN NULL ELSE preset END AS preset FROM egress_rules ORDER BY host LIMIT $1",
        MAX_RULES
    )
    .fetch_all(connection)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(StoredRule {
                host: row.host,
                ports: from_stored(row.ports)?,
                internal: row.internal,
                version: row.version,
                last_set_by: row.last_set_by,
                seed_state: row.seed_state,
                preset: row.preset,
                updated_at: row.updated_at,
            })
        })
        .collect()
}

/// The allow-list from every stored rule, validated as the file would be, plus the any-host rule only the `open_web`
/// preset stores.
///
/// # Errors
/// Storage failure, or a stored rule the validator refuses.
pub(crate) async fn allow_list(connection: &mut PgConnection) -> Result<AllowList, GatewayError> {
    let inputs = rules(connection)
        .await?
        .into_iter()
        .map(|rule| RuleInput {
            host: rule.host,
            ports: rule.ports,
            internal: rule.internal,
        })
        .collect();
    AllowList::stored(inputs).map_err(|_| {
        tracing::error!("a stored egress rule is invalid");
        GatewayError::Configuration("a stored egress rule is invalid")
    })
}

async fn stored(connection: &mut PgConnection, host: &str) -> Result<StoredRule, GatewayError> {
    let row = sqlx::query!(
        "SELECT host,ports,internal,version,last_set_by,seed_state,updated_at,CASE WHEN preset_edited THEN NULL ELSE preset END AS preset FROM egress_rules WHERE host=$1",
        host
    )
    .fetch_one(connection)
    .await?;
    Ok(StoredRule {
        host: row.host,
        ports: from_stored(row.ports)?,
        internal: row.internal,
        version: row.version,
        last_set_by: row.last_set_by,
        seed_state: row.seed_state,
        preset: row.preset,
        updated_at: row.updated_at,
    })
}

/// Creates (`expected` is `None` and the host has no rule) or replaces (`expected` is the current version) one rule in
/// the caller's settings transaction. Returns whether it created, and the stored rule.
///
/// # Errors
/// `409 conflict` for a stale or unexpected version, `409 egress_rule_limit` when the list is full, storage failure.
pub(crate) async fn write(
    connection: &mut PgConnection,
    write: RuleWrite,
) -> Result<(bool, StoredRule), GatewayError> {
    let ports = ports(&write.rule);
    let host = write.rule.host.as_str();
    let current = sqlx::query!(
        "SELECT version,ports,internal FROM egress_rules WHERE host=$1 FOR UPDATE",
        host
    )
    .fetch_optional(&mut *connection)
    .await?;
    let (created, before, after, value_changed) = match (current, write.expected) {
        (None, None) => {
            let count = sqlx::query_scalar!("SELECT count(*) AS \"count!\" FROM egress_rules")
                .fetch_one(&mut *connection)
                .await?;
            if count >= MAX_RULES {
                return Err(GatewayError::Conflict("egress_rule_limit"));
            }
            // A seeded rule deleted earlier keeps its seed digest, so an unchanged seed does not overwrite this one.
            let digest = tombstones::unearth(connection, "egress", host).await?;
            let state = digest.as_ref().map(|_| "current");
            sqlx::query!(
                "INSERT INTO egress_rules (host,ports,internal,version,last_set_by,seed_digest,seed_state) VALUES ($1,$2,$5,1,'admin',$3,$4)",
                host,
                &ports,
                digest,
                state,
                write.rule.internal
            )
            .execute(&mut *connection)
            .await?;
            (true, None, 1, true)
        }
        (Some(current), Some(expected)) if current.version == expected => {
            sqlx::query!(
                "UPDATE egress_rules SET ports=$2,internal=$3,version=version+1,last_set_by='admin',preset_edited=(preset IS NOT NULL),updated_at=CURRENT_TIMESTAMP WHERE host=$1",
                host, &ports, write.rule.internal
            )
            .execute(&mut *connection)
            .await?;
            let changed = current.ports != ports || current.internal != write.rule.internal;
            (false, Some(expected), expected + 1, changed)
        }
        _ => return Err(GatewayError::Conflict("conflict")),
    };
    let change = Change {
        area: "egress",
        actor: Some(write.actor),
        item: host,
        action: if created { "create" } else { "replace" },
        before,
        after: Some(after),
        value_changed,
    };
    audit(connection, &change).await?;
    tracing::info!(
        host,
        version = after,
        "egress rule updated by an administrator"
    );
    Ok((created, stored(connection, host).await?))
}

/// Deletes the rule for `host` when its version is `expected`, in the caller's settings transaction.
///
/// # Errors
/// `404`, `409 conflict` for a stale version, storage failure.
pub(crate) async fn remove(
    connection: &mut PgConnection,
    host: &str,
    expected: i64,
    actor: uuid::Uuid,
) -> Result<(), GatewayError> {
    let current = sqlx::query!(
        "SELECT version,seed_digest FROM egress_rules WHERE host=$1 FOR UPDATE",
        host
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(GatewayError::NotFound)?;
    let version = current.version;
    if version != expected {
        return Err(GatewayError::Conflict("conflict"));
    }
    sqlx::query!("DELETE FROM egress_rules WHERE host=$1", host)
        .execute(&mut *connection)
        .await?;
    tombstones::bury(connection, "egress", host, current.seed_digest.as_deref()).await?;
    let change = Change {
        area: "egress",
        actor: Some(actor),
        item: host,
        action: "delete",
        before: Some(version),
        after: None,
        value_changed: true,
    };
    audit(connection, &change).await?;
    tracing::info!(host, "egress rule deleted by an administrator");
    Ok(())
}
