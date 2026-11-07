//! Model routes in the database (ADR-0028): models.toml seeds them under the seed-change rule, and the registry is
//! built from the stored records through tqa-llm's own validator, the same one the file loader uses.
use super::{Change, KeyedDigest, audit, tombstones};
use crate::GatewayError;
use sqlx::PgConnection;
use subtle::ConstantTimeEq;
use tqa_llm::{Registry, RouteConfig};

/// Applies the routes seed (the file's registry) in the caller's settings transaction; returns whether it wrote.
///
/// # Errors
/// Storage failure.
pub(crate) async fn seed(
    connection: &mut PgConnection,
    seed: &Registry,
    digest: &KeyedDigest,
) -> Result<bool, GatewayError> {
    // Each with the display name the file gave it (N2), checked by the API's rule before anything is written.
    let configs: Vec<RouteConfig> = seed
        .names()
        .filter_map(|name| seed.get(name))
        .cloned()
        .map(|route| seed.config(route))
        .collect();
    if configs.iter().any(|config| {
        config
            .display_name
            .as_deref()
            .is_some_and(|name| !crate::admin::route_display_name_clean(name))
    }) {
        return Err(GatewayError::Configuration(
            "a route's display_name in models.toml must be 1 to 80 characters on one line, without surrounding spaces",
        ));
    }
    let mut wrote = false;
    for config in &configs {
        wrote |= seed_route(connection, config, digest).await?;
    }
    let names: Vec<String> = configs.iter().map(|config| config.name.clone()).collect();
    tombstones::forget_unlisted(connection, "routes", &names).await?;
    let removed = sqlx::query!(
        "UPDATE model_routes SET seed_state='removed' WHERE seed_state='current' AND NOT (name = ANY($1))",
        &names
    )
    .execute(&mut *connection)
    .await?;
    tracing::info!(routes = configs.len(), "model routes seed applied");
    Ok(wrote || removed.rows_affected() > 0)
}

/// A file's route binds the shared secret it names (D10: the file is the operator's declaration).
async fn bind_seeded(
    connection: &mut PgConnection,
    config: &RouteConfig,
) -> Result<(), GatewayError> {
    match config.key_ref.as_deref() {
        Some(reference) if !tqa_types::is_per_user_secret(reference) => {
            crate::secrets::bindings::seed(connection, reference, ("route", &config.name)).await
        }
        _ => Ok(()),
    }
}

/// Imports one seed route when it is new or its seed record changed; returns whether it wrote anything. The file's
/// display name goes to its own column, never into the stored record; the seed's digest covers it only when it is set,
/// so a route without one keeps the digest it had and is not seeded again (GW-I-216).
async fn seed_route(
    connection: &mut PgConnection,
    config: &RouteConfig,
    digest: &KeyedDigest,
) -> Result<bool, GatewayError> {
    let item = format!("routes/{}", config.name);
    let seed = digest.of(&item, &serde_json::to_vec(&serde_json::to_value(config)?)?);
    // The digest a release before `stream_usage` became optional took of the same file record (migration 0085).
    let legacy = digest.of(
        &item,
        &serde_json::to_vec(&serde_json::to_value(config.legacy_form())?)?,
    );
    let document = serde_json::to_value(RouteConfig {
        display_name: None,
        ..config.clone()
    })?;
    let current = sqlx::query!(
        "SELECT version,seed_digest,seed_state,document FROM model_routes WHERE name=$1 FOR UPDATE",
        config.name
    )
    .fetch_optional(&mut *connection)
    .await?;
    let Some(current) = current else {
        return insert_route(connection, config, &document, &seed).await;
    };
    let stored_is = |candidate: &[u8]| {
        current
            .seed_digest
            .as_deref()
            .is_some_and(|stored| bool::from(stored.ct_eq(candidate)))
    };
    // An unchanged file whose digest was taken over the old form: the one-time upgrade (migration 0085).
    let unchanged_old_form = seed != legacy && stored_is(&legacy[..]);
    if unchanged_old_form && upgrade_old_form(connection, config, &current.document, &seed).await? {
        return Ok(true);
    }
    if unchanged_old_form || stored_is(&seed[..]) {
        if current.seed_state.as_deref() == Some("current") {
            return Ok(false);
        }
        sqlx::query!(
            "UPDATE model_routes SET seed_state='current' WHERE name=$1",
            config.name
        )
        .execute(&mut *connection)
        .await?;
        return Ok(true);
    }
    let version = current.version + 1;
    sqlx::query!(
        // A file without a display name leaves the one an administrator gave.
        "UPDATE model_routes SET document=$2,display_name=COALESCE($5,display_name),version=$3,last_set_by='seed',seed_digest=$4,seed_state='current',updated_at=CURRENT_TIMESTAMP WHERE name=$1",
        config.name, document, version, &seed[..], config.display_name
    )
    .execute(&mut *connection)
    .await?;
    bind_seeded(connection, config).await?;
    let change = Change {
        area: "routes",
        actor: None,
        item: &config.name,
        action: "replace",
        before: Some(current.version),
        after: Some(version),
        value_changed: current.document != document,
    };
    audit(connection, &change).await?;
    Ok(true)
}

/// A new seed route, unless the file's record was removed by an administrator (its tombstone).
async fn insert_route(
    connection: &mut PgConnection,
    config: &RouteConfig,
    document: &serde_json::Value,
    seed: &[u8; 32],
) -> Result<bool, GatewayError> {
    if tombstones::buried(connection, "routes", &config.name, seed).await? {
        return Ok(false);
    }
    sqlx::query!(
        "INSERT INTO model_routes (name,document,display_name,version,last_set_by,seed_digest,seed_state) VALUES ($1,$2,$3,1,'seed',$4,'current')",
        config.name, document, config.display_name, &seed[..]
    )
    .execute(&mut *connection)
    .await?;
    bind_seeded(connection, config).await?;
    let change = Change {
        area: "routes",
        actor: None,
        item: &config.name,
        action: "create",
        before: None,
        after: Some(1),
        value_changed: true,
    };
    audit(connection, &change).await?;
    Ok(true)
}

/// The one-time upgrade of a route the file seeded under the old form: the digest moves on, so neither the stored record
/// nor an administrator's edits are replaced by a release's change of form. A file that leaves `stream_usage` out means
/// the new default, on: a stored `false` there was the old default, never applied to runs, and becomes `true`, a new
/// version and a seed change in the audit (migration 0085 leaves seeded routes to this). An explicit `false` in the file
/// has the same digest in both forms, so it never comes here and stays as the file says. Returns whether the record
/// changed.
async fn upgrade_old_form(
    connection: &mut PgConnection,
    config: &RouteConfig,
    stored: &serde_json::Value,
    seed: &[u8; 32],
) -> Result<bool, GatewayError> {
    sqlx::query!(
        "UPDATE model_routes SET seed_digest=$2 WHERE name=$1",
        config.name,
        &seed[..]
    )
    .execute(&mut *connection)
    .await?;
    if config.stream_usage.is_some() || stored["stream_usage"] != serde_json::Value::Bool(false) {
        return Ok(false);
    }
    let version = sqlx::query_scalar!(
        "UPDATE model_routes SET document=jsonb_set(document,'{stream_usage}','true'::jsonb),version=version+1,updated_at=CURRENT_TIMESTAMP WHERE name=$1 RETURNING version",
        config.name
    )
    .fetch_one(&mut *connection)
    .await?;
    let change = Change {
        area: "routes",
        actor: None,
        item: &config.name,
        action: "replace",
        before: Some(version - 1),
        after: Some(version),
        value_changed: true,
    };
    audit(connection, &change).await?;
    Ok(true)
}

/// Whether a stored route cannot be used until its key is entered again: it authenticates but names no secret
/// reference, because the reference was missing or a pasted key was scrubbed from it (SEC-033, GW-I-097).
pub(crate) fn key_ref_invalid(route: &RouteConfig) -> bool {
    route.auth != tqa_llm::Auth::None
        && !route
            .key_ref
            .as_deref()
            .is_some_and(tqa_types::is_secret_reference)
}

/// The stored routes that can be used, with routes that cannot left out and removed from the others' fallbacks, and
/// the names left out.
fn usable(configs: Vec<RouteConfig>) -> (Vec<RouteConfig>, Vec<String>) {
    let (mut usable, flagged): (Vec<_>, Vec<_>) = configs
        .into_iter()
        .partition(|config| !key_ref_invalid(config));
    let flagged: Vec<String> = flagged.into_iter().map(|config| config.name).collect();
    for config in &mut usable {
        config
            .fallbacks
            .retain(|fallback| !flagged.contains(fallback));
    }
    (usable, flagged)
}

/// Removes every pasted key from the stored routes, in the caller's settings transaction at start-up (GW-I-097):
/// a `key_ref` that is not a secret reference is deleted from its route's document, with every idempotency record
/// whose cached response holds it. Returns the routes it scrubbed; each is logged by name, never with its value.
///
/// # Errors
/// Storage failure.
pub(crate) async fn scrub(connection: &mut PgConnection) -> Result<Vec<String>, GatewayError> {
    let rows = sqlx::query!(
        "SELECT name,document->>'key_ref' AS \"key_ref!\" FROM model_routes \
         WHERE jsonb_typeof(document->'key_ref')='string' ORDER BY name FOR UPDATE"
    )
    .fetch_all(&mut *connection)
    .await?;
    let mut scrubbed = Vec::new();
    for row in rows {
        let value = secrecy::SecretString::from(row.key_ref);
        if tqa_types::is_secret_reference(secrecy::ExposeSecret::expose_secret(&value)) {
            continue;
        }
        let cached = sqlx::query!(
            "DELETE FROM idempotency WHERE position(convert_to($1,'UTF8') in response_body) > 0",
            secrecy::ExposeSecret::expose_secret(&value)
        )
        .execute(&mut *connection)
        .await?
        .rows_affected();
        sqlx::query!(
            "UPDATE model_routes SET document=document-'key_ref' WHERE name=$1",
            row.name
        )
        .execute(&mut *connection)
        .await?;
        tracing::error!(
            route = %row.name,
            cached_responses_deleted = cached,
            "a route's key_ref held a pasted key instead of a secret reference; it was removed and the route is \
             unusable until its key is entered again: rotate that key, it was stored in plain text"
        );
        scrubbed.push(row.name);
    }
    Ok(scrubbed)
}

/// The registry from every usable stored route, validated as the file would be. A route that cannot be used
/// (`key_ref_invalid`) is left out with a warning, and removed from other routes' fallbacks.
///
/// # Errors
/// Storage failure, or a stored route the validator refuses.
pub(crate) async fn registry(connection: &mut PgConnection) -> Result<Registry, GatewayError> {
    let documents = sqlx::query_scalar!("SELECT document FROM model_routes ORDER BY name")
        .fetch_all(connection)
        .await?;
    let configs = documents
        .into_iter()
        .map(serde_json::from_value::<RouteConfig>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| GatewayError::Configuration("a stored model route has an invalid record"))?;
    let (configs, flagged) = usable(configs);
    for route in &flagged {
        tracing::warn!(route = %route, "a model route is unusable until its key is entered again (key_ref_invalid)");
    }
    Registry::from_configs(configs).map_err(|error| {
        tracing::error!(
            route = error.route().unwrap_or_default(),
            code = error.reason().map(tqa_llm::RouteInvalid::code),
            "a stored model route is invalid"
        );
        GatewayError::Configuration("a stored model route is invalid")
    })
}

/// One stored route with its administration metadata.
#[derive(Clone, Debug)]
pub(crate) struct StoredRoute {
    pub config: RouteConfig,
    pub display_name: Option<String>,
    /// The administrator's one-line purpose (GW-I-132); display only.
    pub description: Option<String>,
    pub version: i64,
    /// `seed` or `admin`: who wrote the current record.
    pub last_set_by: String,
    /// `current` while the models file lists it, `removed` after it no longer does; `None` if never seeded.
    pub seed_state: Option<String>,
    pub updated_at: time::OffsetDateTime,
}

/// One administrator write of a whole route record.
pub(crate) struct RouteWrite {
    pub config: RouteConfig,
    pub display_name: Option<String>,
    pub description: Option<String>,
    /// The version read; `None` creates, and must then find no route of that name.
    pub expected: Option<i64>,
    pub actor: uuid::Uuid,
}

fn invalid_record() -> GatewayError {
    GatewayError::Configuration("a stored model route has an invalid record")
}

/// Every stored route, by name.
///
/// # Errors
/// Storage failure, or a record that is not a route.
pub(crate) async fn entries(
    connection: &mut PgConnection,
) -> Result<Vec<StoredRoute>, GatewayError> {
    let rows = sqlx::query!(
        "SELECT document,display_name,description,version,last_set_by,seed_state,updated_at FROM model_routes ORDER BY name"
    )
    .fetch_all(connection)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(StoredRoute {
                config: serde_json::from_value(row.document).map_err(|_| invalid_record())?,
                display_name: row.display_name,
                description: row.description,
                version: row.version,
                last_set_by: row.last_set_by,
                seed_state: row.seed_state,
                updated_at: row.updated_at,
            })
        })
        .collect()
}

/// The display names by route name, for the snapshot.
pub(crate) async fn display_names(
    connection: &mut PgConnection,
) -> Result<std::collections::BTreeMap<String, String>, GatewayError> {
    let rows = sqlx::query!(
        "SELECT name,display_name AS \"display_name!\" FROM model_routes WHERE display_name IS NOT NULL"
    )
    .fetch_all(connection)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| (row.name, row.display_name))
        .collect())
}

/// The descriptions by route name, for the snapshot (GW-I-132).
pub(crate) async fn descriptions(
    connection: &mut PgConnection,
) -> Result<std::collections::BTreeMap<String, String>, GatewayError> {
    let rows = sqlx::query!(
        "SELECT name,description AS \"description!\" FROM model_routes WHERE description IS NOT NULL"
    )
    .fetch_all(connection)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| (row.name, row.description))
        .collect())
}

/// SHA-256 of the stored route document (no secret values: `key_ref` is a name), for the route probe (099) to tell
/// whether the route it tested is still the one stored. `None` for an unknown route.
///
/// # Errors
/// Storage failure.
pub(crate) async fn document_digest(
    connection: &mut PgConnection,
    name: &str,
) -> Result<Option<Vec<u8>>, GatewayError> {
    Ok(sqlx::query_scalar!(
        "SELECT sha256(convert_to(document::text, 'UTF8')) AS \"digest!\" FROM model_routes WHERE name=$1",
        name
    )
    .fetch_optional(connection)
    .await?)
}

/// Whether the route names a secret reference that has no value in the store.
///
/// # Errors
/// A store failure.
pub(crate) async fn key_missing(
    route: &RouteConfig,
    secrets: &dyn crate::SecretStore,
) -> Result<bool, GatewayError> {
    Ok(key_missing_in(route, &secrets.refs().await?))
}

/// Whether the route names a secret reference outside `refs` (one store listing serves a whole route list).
pub(crate) fn key_missing_in(route: &RouteConfig, refs: &[crate::secrets::SecretRef]) -> bool {
    route.key_ref.as_deref().is_some_and(|reference| {
        reference
            .parse::<crate::secrets::SecretRef>()
            .map_or(true, |reference| !refs.contains(&reference))
    })
}

/// The registry that would result from writing `written`, validated as a whole (fallbacks name other routes). The
/// other stored routes that cannot be used are left out, as the live registry leaves them out; `written` itself is
/// always validated as written.
async fn validated(
    connection: &mut PgConnection,
    written: &RouteConfig,
) -> Result<(), GatewayError> {
    let others: Vec<RouteConfig> = entries(connection)
        .await?
        .into_iter()
        .map(|stored| stored.config)
        .filter(|existing| existing.name != written.name)
        .collect();
    let (mut configs, _) = usable(others);
    configs.push(written.clone());
    Registry::from_configs(configs)
        .map(drop)
        .map_err(|error| super::route_reasons::refusal(&error))
}

async fn stored(connection: &mut PgConnection, name: &str) -> Result<StoredRoute, GatewayError> {
    let row = sqlx::query!(
        "SELECT document,display_name,description,version,last_set_by,seed_state,updated_at FROM model_routes WHERE name=$1",
        name
    )
    .fetch_one(connection)
    .await?;
    Ok(StoredRoute {
        config: serde_json::from_value(row.document).map_err(|_| invalid_record())?,
        display_name: row.display_name,
        description: row.description,
        version: row.version,
        last_set_by: row.last_set_by,
        seed_state: row.seed_state,
        updated_at: row.updated_at,
    })
}

/// Creates (`expected` is `None` and the name is free) or replaces (`expected` is the current version) one route in
/// the caller's settings transaction, after validating the whole resulting registry. Returns whether it created.
///
/// # Errors
/// `400 invalid_request` with the validator's reason, `409 conflict` for a stale or unexpected version, storage failure.
pub(crate) async fn write(
    connection: &mut PgConnection,
    write: RouteWrite,
) -> Result<(bool, StoredRoute), GatewayError> {
    let name = write.config.name.clone();
    let current = sqlx::query!(
        "SELECT version,document,display_name,description FROM model_routes WHERE name=$1 FOR UPDATE",
        name
    )
    .fetch_optional(&mut *connection)
    .await?;
    let created = match (&current, write.expected) {
        (None, None) => true,
        (Some(current), Some(expected)) if current.version == expected => false,
        _ => return Err(GatewayError::Conflict("conflict")),
    };
    validated(connection, &write.config).await?;
    // A shared secret must be bound to the route first (vault slice 3).
    if let Some(reference) = write.config.key_ref.as_deref() {
        crate::secrets::bindings::require(
            connection,
            reference,
            ("route", &write.config.name),
            write.actor,
        )
        .await?;
    }
    let document = serde_json::to_value(&write.config)?;
    let (before, after, value_changed) = if let Some(current) = current {
        sqlx::query!(
            "UPDATE model_routes SET document=$2,display_name=$3,description=$4,version=version+1,last_set_by='admin',updated_at=CURRENT_TIMESTAMP WHERE name=$1",
            name, document, write.display_name, write.description
        )
        .execute(&mut *connection)
        .await?;
        let changed = current.document != document
            || current.display_name != write.display_name
            || current.description != write.description;
        (Some(current.version), current.version + 1, changed)
    } else {
        // A seeded route deleted earlier keeps its seed digest, so an unchanged seed does not overwrite this one.
        let digest = tombstones::unearth(connection, "routes", &name).await?;
        let state = digest.as_ref().map(|_| "current");
        sqlx::query!(
            "INSERT INTO model_routes (name,document,display_name,description,version,last_set_by,seed_digest,seed_state) VALUES ($1,$2,$3,$4,1,'admin',$5,$6)",
            name, document, write.display_name, write.description, digest, state
        )
        .execute(&mut *connection)
        .await?;
        (None, 1, true)
    };
    let change = Change {
        area: "routes",
        actor: Some(write.actor),
        item: &name,
        action: if created { "create" } else { "replace" },
        before,
        after: Some(after),
        value_changed,
    };
    audit(connection, &change).await?;
    tracing::info!(route = %name, version = after, "model route updated by an administrator");
    Ok((created, stored(connection, &name).await?))
}

/// Deletes the route when its version is `expected` and no other route lists it as a fallback.
///
/// # Errors
/// `404`, `409 conflict` for a stale version, `409 route_is_fallback`, storage failure.
pub(crate) async fn remove(
    connection: &mut PgConnection,
    name: &str,
    expected: i64,
    actor: uuid::Uuid,
) -> Result<(), GatewayError> {
    let current = sqlx::query!(
        "SELECT version,seed_digest FROM model_routes WHERE name=$1 FOR UPDATE",
        name
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(GatewayError::NotFound)?;
    let version = current.version;
    if version != expected {
        return Err(GatewayError::Conflict("conflict"));
    }
    let listed = entries(connection).await?.iter().any(|stored| {
        stored
            .config
            .fallbacks
            .iter()
            .any(|fallback| fallback == name)
    });
    if listed {
        return Err(GatewayError::Conflict("route_is_fallback"));
    }
    sqlx::query!("DELETE FROM model_routes WHERE name=$1", name)
        .execute(&mut *connection)
        .await?;
    crate::secrets::bindings::forget(connection, ("route", name)).await?;
    tombstones::bury(connection, "routes", name, current.seed_digest.as_deref()).await?;
    let change = Change {
        area: "routes",
        actor: Some(actor),
        item: name,
        action: "delete",
        before: Some(version),
        after: None,
        value_changed: true,
    };
    audit(connection, &change).await?;
    tracing::info!(route = name, "model route deleted by an administrator");
    Ok(())
}

#[cfg(test)]
#[path = "routes_tests.rs"]
mod tests;
