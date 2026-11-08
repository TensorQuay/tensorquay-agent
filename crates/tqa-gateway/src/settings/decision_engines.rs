//! Decision engines in the database (ADR-0013 point 6, the ADR-0028 pattern): `[[decision_engines]]` in gateway.toml
//! seeds them under the seed-change rule, administrators change them through the API, and the snapshot carries every
//! stored one. A change of an engine's exposure is audited as its own action.
use super::{Change, KeyedDigest, audit, audit::audit_detail, tombstones};
use crate::{
    GatewayError,
    decision_engines::{BudgetPeriod, EngineBudget, EngineDocument},
};
use sqlx::PgConnection;
use subtle::ConstantTimeEq;
use tqa_types::DecisionExposure;

const AREA: &str = "decision_engines";

/// Applies the `[[decision_engines]]` seed (already validated at start) in the caller's settings transaction; returns
/// whether it wrote.
///
/// # Errors
/// Storage failure.
pub(crate) async fn seed(
    connection: &mut PgConnection,
    seed: &[EngineDocument],
    digest: &KeyedDigest,
) -> Result<bool, GatewayError> {
    let mut wrote = false;
    for document in seed {
        wrote |= seed_one(connection, document, digest).await?;
    }
    let names: Vec<String> = seed.iter().map(|document| document.name.clone()).collect();
    tombstones::forget_unlisted(connection, AREA, &names).await?;
    let removed = sqlx::query!(
        "UPDATE decision_engines SET seed_state='removed' WHERE seed_state='current' AND NOT (name = ANY($1))",
        &names
    )
    .execute(&mut *connection)
    .await?;
    tracing::info!(
        decision_engines = seed.len(),
        "decision engines seed applied"
    );
    Ok(wrote || removed.rows_affected() > 0)
}

/// A file's engine binds the shared secret it names (the file is the operator's declaration).
async fn bind_seeded(
    connection: &mut PgConnection,
    document: &EngineDocument,
) -> Result<(), GatewayError> {
    match document.key_ref.as_deref() {
        Some(reference) if !tqa_types::is_per_user_secret(reference) => {
            crate::secrets::bindings::seed(connection, reference, ("engine", &document.name)).await
        }
        _ => Ok(()),
    }
}

/// Imports a seed engine the store doesn't hold, unless an administrator deleted it under this same seed entry.
async fn seed_new(
    connection: &mut PgConnection,
    document: &EngineDocument,
    value: &serde_json::Value,
    seed: &[u8],
) -> Result<bool, GatewayError> {
    if tombstones::buried(connection, AREA, &document.name, seed).await? {
        return Ok(false);
    }
    sqlx::query!(
            "INSERT INTO decision_engines (name,document,version,last_set_by,seed_digest,seed_state,budget_since) \
             VALUES ($1,$2,1,'seed',$3,'current',CASE WHEN $2::jsonb ? 'budget' THEN CURRENT_TIMESTAMP END)",
            document.name, value, seed
        )
        .execute(&mut *connection)
        .await?;
    bind_seeded(connection, document).await?;
    let change = Change {
        area: AREA,
        actor: None,
        item: &document.name,
        action: "create",
        before: None,
        after: Some(1),
        value_changed: true,
    };
    audit(connection, &change).await?;
    audit_budget(connection, &change, None, document.budget).await?;
    Ok(true)
}

/// Imports one seed engine when it is new or its seed entry changed; returns whether it wrote anything.
async fn seed_one(
    connection: &mut PgConnection,
    document: &EngineDocument,
    digest: &KeyedDigest,
) -> Result<bool, GatewayError> {
    let value = serde_json::to_value(document)?;
    let seed = digest.of(
        &format!("{AREA}/{}", document.name),
        &serde_json::to_vec(&value)?,
    );
    let current = sqlx::query!(
        "SELECT version,seed_digest,seed_state,document FROM decision_engines WHERE name=$1 FOR UPDATE",
        document.name
    )
    .fetch_optional(&mut *connection)
    .await?;
    let Some(current) = current else {
        return seed_new(connection, document, &value, &seed).await;
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
            "UPDATE decision_engines SET seed_state='current' WHERE name=$1",
            document.name
        )
        .execute(&mut *connection)
        .await?;
        return Ok(true);
    }
    let version = current.version + 1;
    sqlx::query!(
        "UPDATE decision_engines SET document=$2,version=$3,last_set_by='seed',seed_digest=$4,seed_state='current',\
         updated_at=CURRENT_TIMESTAMP,budget_since=CASE WHEN document->'budget' IS NOT DISTINCT FROM $2::jsonb->'budget' \
         THEN budget_since WHEN $2::jsonb ? 'budget' THEN CURRENT_TIMESTAMP END WHERE name=$1",
        document.name, value, version, &seed[..]
    )
    .execute(&mut *connection)
    .await?;
    bind_seeded(connection, document).await?;
    let change = Change {
        area: AREA,
        actor: None,
        item: &document.name,
        action: "replace",
        before: Some(current.version),
        after: Some(version),
        value_changed: current.document != value,
    };
    audit(connection, &change).await?;
    let old: Option<EngineDocument> = serde_json::from_value(current.document).ok();
    audit_budget(
        connection,
        &change,
        old.and_then(|old| old.budget),
        document.budget,
    )
    .await?;
    Ok(true)
}

/// One stored engine with its metadata.
#[derive(Clone, Debug)]
pub(crate) struct StoredEngine {
    pub document: EngineDocument,
    pub version: i64,
    pub last_set_by: String,
    pub seed_state: Option<String>,
    pub updated_at: time::OffsetDateTime,
}

/// A create (`expected` is `None`) or a replacement of the version read.
pub(crate) struct EngineWrite {
    pub document: EngineDocument,
    pub expected: Option<i64>,
    pub actor: uuid::Uuid,
}

/// Every stored engine, by name.
///
/// # Errors
/// Storage failure, or a record that is not an engine.
pub(crate) async fn entries(
    connection: &mut PgConnection,
) -> Result<Vec<StoredEngine>, GatewayError> {
    let rows = sqlx::query!(
        "SELECT document,version,last_set_by,seed_state,updated_at FROM decision_engines ORDER BY name"
    )
    .fetch_all(connection)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(StoredEngine {
                document: serde_json::from_value(row.document)
                    .map_err(|_| GatewayError::StoreUnavailable)?,
                version: row.version,
                last_set_by: row.last_set_by,
                seed_state: row.seed_state,
                updated_at: row.updated_at,
            })
        })
        .collect()
}

/// Every stored engine, by name, for the snapshot.
///
/// # Errors
/// Storage failure, or a record that is not an engine.
pub(crate) async fn documents(
    connection: &mut PgConnection,
) -> Result<Vec<EngineDocument>, GatewayError> {
    Ok(entries(connection)
        .await?
        .into_iter()
        .map(|stored| stored.document)
        .collect())
}

/// Creates (`expected` is `None` and the name is free) or replaces (`expected` is the current version) one engine in
/// the caller's settings transaction; the caller has validated the document. A shared secret must allow the engine
/// first. Returns whether it created.
///
/// # Errors
/// `409 conflict` for a stale or unexpected version, the secret binding's refusal, storage failure.
pub(crate) async fn write(
    connection: &mut PgConnection,
    write: EngineWrite,
) -> Result<(bool, StoredEngine), GatewayError> {
    let name = write.document.name.clone();
    if let Some(reference) = write.document.key_ref.as_deref() {
        crate::secrets::bindings::require(connection, reference, ("engine", &name), write.actor)
            .await?;
    }
    let current = sqlx::query!(
        "SELECT version,document FROM decision_engines WHERE name=$1 FOR UPDATE",
        name
    )
    .fetch_optional(&mut *connection)
    .await?;
    let created = match (&current, write.expected) {
        (None, None) => true,
        (Some(current), Some(expected)) if current.version == expected => false,
        _ => return Err(GatewayError::Conflict("conflict")),
    };
    let value = serde_json::to_value(&write.document)?;
    let (before, after, value_changed, old_exposure, old_budget) = if let Some(current) = current {
        let old: EngineDocument = serde_json::from_value(current.document.clone())
            .map_err(|_| GatewayError::StoreUnavailable)?;
        sqlx::query!(
            "UPDATE decision_engines SET document=$2,version=version+1,last_set_by='admin',updated_at=CURRENT_TIMESTAMP,\
             budget_since=CASE WHEN document->'budget' IS NOT DISTINCT FROM $2::jsonb->'budget' THEN budget_since \
             WHEN $2::jsonb ? 'budget' THEN CURRENT_TIMESTAMP END WHERE name=$1",
            name, value
        )
        .execute(&mut *connection)
        .await?;
        (
            Some(current.version),
            current.version + 1,
            current.document != value,
            old.exposure,
            old.budget,
        )
    } else {
        // A seeded engine deleted earlier keeps its seed digest, so an unchanged seed does not overwrite this one.
        let digest = tombstones::unearth(connection, AREA, &name).await?;
        let state = digest.as_ref().map(|_| "current");
        sqlx::query!(
            "INSERT INTO decision_engines (name,document,version,last_set_by,seed_digest,seed_state,budget_since) \
             VALUES ($1,$2,1,'admin',$3,$4,CASE WHEN $2::jsonb ? 'budget' THEN CURRENT_TIMESTAMP END)",
            name, value, digest, state
        )
        .execute(&mut *connection)
        .await?;
        (None, 1, true, DecisionExposure::None, None)
    };
    let change = Change {
        area: AREA,
        actor: Some(write.actor),
        item: &name,
        action: if created { "create" } else { "replace" },
        before,
        after: Some(after),
        value_changed,
    };
    audit(connection, &change).await?;
    audit_budget(connection, &change, old_budget, write.document.budget).await?;
    audit_exposure(connection, change, old_exposure, write.document.exposure).await?;
    tracing::info!(engine = %name, version = after, "decision engine updated by an administrator");
    let stored = entries(connection)
        .await?
        .into_iter()
        .find(|stored| stored.document.name == name)
        .ok_or(GatewayError::StoreUnavailable)?;
    Ok((created, stored))
}

/// Deletes the engine when its version is `expected` and nothing names it, with its secret binding. A seeded one is
/// buried, so an unchanged seed never brings it back.
///
/// # Errors
/// `404`, `409 conflict` for a stale version, `409 decision_engine_in_use` with what names it, storage failure.
pub(crate) async fn remove(
    connection: &mut PgConnection,
    name: &str,
    expected: i64,
    actor: uuid::Uuid,
) -> Result<(), GatewayError> {
    let current = sqlx::query!(
        "SELECT version,seed_digest FROM decision_engines WHERE name=$1 FOR UPDATE",
        name
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(GatewayError::NotFound)?;
    if current.version != expected {
        return Err(GatewayError::Conflict("conflict"));
    }
    in_use(connection, name).await?;
    sqlx::query!("DELETE FROM decision_engines WHERE name=$1", name)
        .execute(&mut *connection)
        .await?;
    crate::secrets::bindings::forget(connection, ("engine", name)).await?;
    tombstones::bury(connection, AREA, name, current.seed_digest.as_deref()).await?;
    let change = Change {
        area: AREA,
        actor: Some(actor),
        item: name,
        action: "delete",
        before: Some(current.version),
        after: None,
        value_changed: true,
    };
    audit(connection, &change).await?;
    tracing::info!(engine = name, "decision engine deleted by an administrator");
    Ok(())
}

/// Refuses deleting an engine that a profile or an active conversation names: one click would strand them all, where
/// disabling it is the soft path that already ends each of them visibly.
async fn in_use(connection: &mut PgConnection, name: &str) -> Result<(), GatewayError> {
    let conversations = sqlx::query_scalar!(
        "SELECT count(*) AS \"count!\" FROM conversations WHERE profile->>'decision_engine'=$1 AND state='active'",
        name
    )
    .fetch_one(&mut *connection)
    .await?;
    let profiles = sqlx::query_scalar!(
        "SELECT name FROM agent_profiles WHERE owner_id IS NULL AND document->>'decision_engine'=$1 ORDER BY name",
        name
    )
    .fetch_all(&mut *connection)
    .await?;
    let personal_profiles = sqlx::query_scalar!(
        "SELECT count(*) AS \"count!\" FROM agent_profiles WHERE owner_id IS NOT NULL AND document->>'decision_engine'=$1",
        name
    )
    .fetch_one(&mut *connection)
    .await?;
    if conversations > 0 || !profiles.is_empty() || personal_profiles > 0 {
        return Err(GatewayError::EngineInUse(super::templates::InUse {
            default: false,
            conversations,
            mcp_servers: Vec::new(),
            profiles,
            personal_profiles,
        }));
    }
    Ok(())
}

/// The exposure decides how much of each conversation reaches a third party: every change of it is its own row, with the
/// old and new exposure, so the trail outlives the logs. A new engine that sees more than names counts as a change from
/// none.
async fn audit_exposure(
    connection: &mut PgConnection,
    change: Change<'_>,
    old: DecisionExposure,
    new: DecisionExposure,
) -> Result<(), GatewayError> {
    if new == old {
        return Ok(());
    }
    let detail = format!("{}->{}", exposure_name(old), exposure_name(new));
    let exposure = Change {
        action: "exposure",
        ..change
    };
    audit_detail(connection, &exposure, &detail).await?;
    tracing::info!(engine = %exposure.item, exposure = %detail, "decision engine exposure changed by an administrator");
    Ok(())
}

/// Records a change of the engine's budget as its own `budget` row, with the old and new budget, like the exposure; a
/// write that keeps the budget records none. The seed's changes are recorded as the seed's.
async fn audit_budget(
    connection: &mut PgConnection,
    change: &Change<'_>,
    old: Option<EngineBudget>,
    new: Option<EngineBudget>,
) -> Result<(), GatewayError> {
    if old == new {
        return Ok(());
    }
    let detail = format!("{}->{}", budget_detail(old), budget_detail(new));
    let budget = Change {
        area: change.area,
        actor: change.actor,
        item: change.item,
        action: "budget",
        before: change.before,
        after: change.after,
        value_changed: true,
    };
    audit_detail(connection, &budget, &detail).await?;
    tracing::info!(engine = %change.item, budget = %detail, "decision engine budget changed");
    Ok(())
}

/// A budget for the audit detail: `none`, or `calls<N>_tokens<N>_<period>` with the absent limits left out.
fn budget_detail(budget: Option<EngineBudget>) -> String {
    let Some(budget) = budget else {
        return "none".into();
    };
    let calls = budget.calls.map(|calls| format!("calls{calls}_"));
    let tokens = budget.tokens.map(|tokens| format!("tokens{tokens}_"));
    let period = match budget.period {
        BudgetPeriod::Total => "total",
        BudgetPeriod::Month => "month",
    };
    format!(
        "{}{}{period}",
        calls.unwrap_or_default(),
        tokens.unwrap_or_default()
    )
}

/// The exposure's wire name, for the audit detail.
fn exposure_name(exposure: DecisionExposure) -> &'static str {
    match exposure {
        DecisionExposure::Calls => "calls",
        DecisionExposure::Full => "full",
        _ => "none",
    }
}
