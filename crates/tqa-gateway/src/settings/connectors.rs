//! Connectors in the database (ADR-0031 decision 7, the ADR-0028 pattern): `[[connectors]]` in gateway.toml seeds them
//! under the seed-change rule, administrators change them through the API, and the snapshot carries every stored one.
use super::{Change, KeyedDigest, audit, tombstones};
use crate::{GatewayError, connectors::ConnectorDocument};
use sqlx::PgConnection;
use subtle::ConstantTimeEq;

const AREA: &str = "connectors";

/// One stored connector with its metadata.
#[derive(Clone, Debug)]
pub(crate) struct StoredConnector {
    pub document: ConnectorDocument,
    pub display_name: Option<String>,
    pub version: i64,
    pub last_set_by: String,
    pub seed_state: Option<String>,
    pub updated_at: time::OffsetDateTime,
}

/// A create (`expected` is `None`) or a replacement of the version read.
pub(crate) struct ConnectorWrite {
    pub document: ConnectorDocument,
    pub display_name: Option<String>,
    pub expected: Option<i64>,
    pub actor: uuid::Uuid,
}

fn invalid_record() -> GatewayError {
    GatewayError::StoreUnavailable
}

/// Applies the `[[connectors]]` seed (already validated at start) in the caller's settings transaction; returns
/// whether it wrote.
///
/// # Errors
/// Storage failure.
pub(crate) async fn seed(
    connection: &mut PgConnection,
    seed: &[ConnectorDocument],
    digest: &KeyedDigest,
) -> Result<bool, GatewayError> {
    let mut wrote = false;
    for document in seed {
        wrote |= seed_one(connection, document, digest).await?;
    }
    let names: Vec<String> = seed.iter().map(|document| document.name.clone()).collect();
    tombstones::forget_unlisted(connection, AREA, &names).await?;
    let removed = sqlx::query!(
        "UPDATE connectors SET seed_state='removed' WHERE seed_state='current' AND NOT (name = ANY($1))",
        &names
    )
    .execute(&mut *connection)
    .await?;
    tracing::info!(connectors = seed.len(), "connectors seed applied");
    Ok(wrote || removed.rows_affected() > 0)
}

/// A file's connector binds the shared secret it names (D10: the file is the operator's declaration).
async fn bind_seeded(
    connection: &mut PgConnection,
    document: &ConnectorDocument,
) -> Result<(), GatewayError> {
    match document.secret_ref.as_deref() {
        Some(reference) if !tqa_types::is_per_user_secret(reference) => {
            crate::secrets::bindings::seed(connection, reference, ("connector", &document.name))
                .await
        }
        _ => Ok(()),
    }
}

/// Imports one seed connector when it is new or its seed entry changed; returns whether it wrote anything.
async fn seed_one(
    connection: &mut PgConnection,
    document: &ConnectorDocument,
    digest: &KeyedDigest,
) -> Result<bool, GatewayError> {
    let value = serde_json::to_value(document)?;
    let seed = digest.of(
        &format!("{AREA}/{}", document.name),
        &serde_json::to_vec(&value)?,
    );
    let current = sqlx::query!(
        "SELECT version,seed_digest,seed_state,document FROM connectors WHERE name=$1 FOR UPDATE",
        document.name
    )
    .fetch_optional(&mut *connection)
    .await?;
    let Some(current) = current else {
        if tombstones::buried(connection, AREA, &document.name, &seed).await? {
            return Ok(false);
        }
        sqlx::query!(
            "INSERT INTO connectors (name,document,version,last_set_by,seed_digest,seed_state) VALUES ($1,$2,1,'seed',$3,'current')",
            document.name, value, &seed[..]
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
            "UPDATE connectors SET seed_state='current' WHERE name=$1",
            document.name
        )
        .execute(&mut *connection)
        .await?;
        return Ok(true);
    }
    let version = current.version + 1;
    sqlx::query!(
        "UPDATE connectors SET document=$2,version=$3,last_set_by='seed',seed_digest=$4,seed_state='current',updated_at=CURRENT_TIMESTAMP WHERE name=$1",
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
    Ok(true)
}

/// Every stored connector, by name.
///
/// # Errors
/// Storage failure, or a record that is not a connector.
pub(crate) async fn entries(
    connection: &mut PgConnection,
) -> Result<Vec<StoredConnector>, GatewayError> {
    let rows = sqlx::query!(
        "SELECT document,display_name,version,last_set_by,seed_state,updated_at FROM connectors ORDER BY name"
    )
    .fetch_all(connection)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(StoredConnector {
                document: serde_json::from_value(row.document).map_err(|_| invalid_record())?,
                display_name: row.display_name,
                version: row.version,
                last_set_by: row.last_set_by,
                seed_state: row.seed_state,
                updated_at: row.updated_at,
            })
        })
        .collect()
}

/// The connectors for the snapshot.
///
/// # Errors
/// Storage failure, or a record that is not a connector.
pub(crate) async fn documents(
    connection: &mut PgConnection,
) -> Result<Vec<ConnectorDocument>, GatewayError> {
    Ok(entries(connection)
        .await?
        .into_iter()
        .map(|stored| stored.document)
        .collect())
}

async fn stored(
    connection: &mut PgConnection,
    name: &str,
) -> Result<StoredConnector, GatewayError> {
    entries(connection)
        .await?
        .into_iter()
        .find(|stored| stored.document.name == name)
        .ok_or(GatewayError::NotFound)
}

/// Creates (`expected` is `None` and the name is free) or replaces (`expected` is the current version) one connector
/// in the caller's settings transaction; the caller has validated the document. Returns whether it created.
///
/// # Errors
/// `409 conflict` for a stale or unexpected version, storage failure.
pub(crate) async fn write(
    connection: &mut PgConnection,
    write: ConnectorWrite,
) -> Result<(bool, StoredConnector), GatewayError> {
    let name = write.document.name.clone();
    // A shared secret must be bound to the connector first (vault slice 3).
    if let Some(reference) = write.document.secret_ref.as_deref() {
        crate::secrets::bindings::require(connection, reference, ("connector", &name), write.actor)
            .await?;
    }
    let current = sqlx::query!(
        "SELECT version,document,display_name FROM connectors WHERE name=$1 FOR UPDATE",
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
    let (before, after, value_changed) = if let Some(current) = current {
        sqlx::query!(
            "UPDATE connectors SET document=$2,display_name=$3,version=version+1,last_set_by='admin',updated_at=CURRENT_TIMESTAMP WHERE name=$1",
            name, value, write.display_name
        )
        .execute(&mut *connection)
        .await?;
        let changed = current.document != value || current.display_name != write.display_name;
        (Some(current.version), current.version + 1, changed)
    } else {
        // A seeded connector deleted earlier keeps its seed digest, so an unchanged seed does not overwrite this one.
        let digest = tombstones::unearth(connection, AREA, &name).await?;
        let state = digest.as_ref().map(|_| "current");
        sqlx::query!(
            "INSERT INTO connectors (name,document,display_name,version,last_set_by,seed_digest,seed_state) VALUES ($1,$2,$3,1,'admin',$4,$5)",
            name, value, write.display_name, digest, state
        )
        .execute(&mut *connection)
        .await?;
        (None, 1, true)
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
    tracing::info!(connector = %name, version = after, "connector updated by an administrator");
    Ok((created, stored(connection, &name).await?))
}

/// Deletes the connector when its version is `expected`. A seeded one is buried, so an unchanged seed never brings
/// it back.
///
/// # Errors
/// `404`, `409 conflict` for a stale version, storage failure.
pub(crate) async fn remove(
    connection: &mut PgConnection,
    name: &str,
    expected: i64,
    actor: uuid::Uuid,
) -> Result<(), GatewayError> {
    let current = sqlx::query!(
        "SELECT version,seed_digest FROM connectors WHERE name=$1 FOR UPDATE",
        name
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(GatewayError::NotFound)?;
    if current.version != expected {
        return Err(GatewayError::Conflict("conflict"));
    }
    sqlx::query!("DELETE FROM connectors WHERE name=$1", name)
        .execute(&mut *connection)
        .await?;
    crate::secrets::bindings::forget(connection, ("connector", name)).await?;
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
    tracing::info!(connector = name, "connector deleted by an administrator");
    Ok(())
}
