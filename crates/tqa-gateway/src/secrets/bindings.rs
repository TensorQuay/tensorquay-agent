//! Which consumers may use each shared secret (ADR-0033; GW-U-056,
//! GW-I-096, SEC-032). A route or connector may read a shared secret only when the secret is bound to it:
//! - an administrator binds them (versioned and audited), and a write naming an unbound secret is refused;
//! - a file (models.toml, gateway.toml) binds the secret of each route or connector it seeds: the file is the
//!   operator's declaration;
//! - the upgrade bound each secret to the consumers that used it then (the migration), and a secret created later
//!   allows none until bound.
//!
//! A read for a consumer the secret is not bound to fails closed. Users' own credentials are not bound: only their
//! owner's calls use them.
use crate::{GatewayError, InvalidReason, settings::Change};
use sqlx::PgConnection;
use std::collections::BTreeSet;

/// The most consumers one secret may be bound to.
pub(crate) const MAX_CONSUMERS: usize = 64;

/// A consumer a shared secret may be bound to.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub(crate) struct Consumer {
    /// `connector` or `route`.
    pub kind: String,
    pub name: String,
}

/// Every binding, as (reference, consumer).
pub(crate) type Bindings = BTreeSet<(String, Consumer)>;

/// Whether `reference` may be read for `(kind, name)`.
pub(crate) fn allows(bindings: &Bindings, reference: &str, kind: &str, name: &str) -> bool {
    bindings.contains(&(
        reference.to_owned(),
        Consumer {
            kind: kind.to_owned(),
            name: name.to_owned(),
        },
    ))
}

/// The consumers `reference` is bound to, sorted.
pub(crate) fn of(bindings: &Bindings, reference: &str) -> Vec<Consumer> {
    bindings
        .iter()
        .filter(|(bound, _)| bound == reference)
        .map(|(_, consumer)| consumer.clone())
        .collect()
}

/// Every binding in the store.
///
/// # Errors
/// Storage failure.
pub(crate) async fn read(connection: &mut PgConnection) -> Result<Bindings, GatewayError> {
    let rows = sqlx::query!("SELECT ref,consumer_kind,consumer FROM secret_consumers")
        .fetch_all(connection)
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            (
                row.r#ref,
                Consumer {
                    kind: row.consumer_kind,
                    name: row.consumer,
                },
            )
        })
        .collect())
}

/// Refuses an administrator's write of a route or connector naming an existing shared secret not bound to it. A
/// secret that does not exist yet is bound to it instead: there is nothing to protect yet, and the route is
/// shown as missing its key until an administrator sets a value, who then sees the binding.
///
/// # Errors
/// `409 secret_not_allowed`, or storage failure.
pub(crate) async fn require(
    connection: &mut PgConnection,
    reference: &str,
    (kind, name): (&str, &str),
    actor: uuid::Uuid,
) -> Result<(), GatewayError> {
    let row = sqlx::query!(
        "SELECT EXISTS(SELECT 1 FROM secret_consumers WHERE ref=$1 AND consumer_kind=$2 AND consumer=$3) AS \"bound!\", \
         EXISTS(SELECT 1 FROM secrets WHERE ref=$1) AS \"exists!\"",
        reference,
        kind,
        name
    )
    .fetch_one(&mut *connection)
    .await?;
    if row.bound {
        return Ok(());
    }
    if row.exists {
        return Err(GatewayError::Conflict("secret_not_allowed"));
    }
    sqlx::query!(
        "INSERT INTO secret_consumers (ref,consumer_kind,consumer,bound_by) VALUES ($1,$2,$3,'admin') ON CONFLICT DO NOTHING",
        reference,
        kind,
        name
    )
    .execute(&mut *connection)
    .await?;
    let change = Change {
        area: "secrets",
        actor: Some(actor),
        item: reference,
        action: "replace",
        before: None,
        after: None,
        value_changed: false,
    };
    crate::settings::audit(connection, &change).await
}

/// Binds the secret a file's route or connector names to it, audited as the seed when it is new.
///
/// # Errors
/// Storage failure.
pub(crate) async fn seed(
    connection: &mut PgConnection,
    reference: &str,
    (kind, name): (&str, &str),
) -> Result<(), GatewayError> {
    // The file is the operator's declaration, so it is never refused (an upgrade must start), but a reference that looks
    // like a key is named by its consumer, never its value (SEC-033).
    if tqa_types::is_key_shaped_name(reference) {
        tracing::warn!(
            kind,
            consumer = name,
            "a configuration file's reference names a secret that looks like a key, not a name: rename the secret and \
             the reference"
        );
    }
    let inserted = sqlx::query!(
        "INSERT INTO secret_consumers (ref,consumer_kind,consumer,bound_by) VALUES ($1,$2,$3,'seed') ON CONFLICT DO NOTHING",
        reference,
        kind,
        name
    )
    .execute(&mut *connection)
    .await?
    .rows_affected();
    if inserted > 0 {
        let change = Change {
            area: "secrets",
            actor: None,
            item: reference,
            action: "replace",
            before: None,
            after: None,
            value_changed: false,
        };
        crate::settings::audit(connection, &change).await?;
    }
    Ok(())
}

/// Forgets a deleted route's or connector's bindings, so a later one of the same name inherits none.
///
/// # Errors
/// Storage failure.
pub(crate) async fn forget(
    connection: &mut PgConnection,
    (kind, name): (&str, &str),
) -> Result<(), GatewayError> {
    sqlx::query!(
        "DELETE FROM secret_consumers WHERE consumer_kind=$1 AND consumer=$2",
        kind,
        name
    )
    .execute(connection)
    .await?;
    Ok(())
}

/// Replaces the consumers `reference` is bound to, at version `expected`: the secret's version increases, and the
/// change is audited as a replace that changes no value. Returns the new version.
///
/// # Errors
/// `404` when absent, `409 conflict` for a stale version, storage failure.
pub(crate) async fn replace(
    connection: &mut PgConnection,
    reference: &str,
    consumers: &[Consumer],
    (expected, actor): (i64, uuid::Uuid),
) -> Result<i64, GatewayError> {
    let current = sqlx::query_scalar!(
        "SELECT version FROM secrets WHERE ref=$1 FOR UPDATE",
        reference
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(GatewayError::NotFound)?;
    if current != expected {
        return Err(GatewayError::Conflict("conflict"));
    }
    sqlx::query!("DELETE FROM secret_consumers WHERE ref=$1", reference)
        .execute(&mut *connection)
        .await?;
    for consumer in consumers {
        sqlx::query!(
            "INSERT INTO secret_consumers (ref,consumer_kind,consumer,bound_by) VALUES ($1,$2,$3,'admin')",
            reference,
            consumer.kind,
            consumer.name
        )
        .execute(&mut *connection)
        .await?;
    }
    let version = sqlx::query_scalar!(
        "UPDATE secrets SET version=version+1 WHERE ref=$1 RETURNING version",
        reference
    )
    .fetch_one(&mut *connection)
    .await?;
    let change = Change {
        area: "secrets",
        actor: Some(actor),
        item: reference,
        action: "replace",
        before: Some(expected),
        after: Some(version),
        value_changed: false,
    };
    crate::settings::audit(connection, &change).await?;
    Ok(version)
}

/// The request `{consumers: [{kind: route|connector, name}], version}`: each name checked by its kind's rule,
/// duplicates removed, sorted, at most [`MAX_CONSUMERS`].
///
/// # Errors
/// `400 invalid_request` naming the field.
pub(crate) fn parse(body: &[u8]) -> Result<(Vec<Consumer>, i64), GatewayError> {
    let invalid =
        |field: &'static str, reason: InvalidReason| GatewayError::Invalid { field, reason };
    let body: serde_json::Map<String, serde_json::Value> =
        serde_json::from_slice(body).map_err(|_| invalid("body", InvalidReason::BodyInvalid))?;
    if body
        .keys()
        .any(|key| key != "consumers" && key != "version")
    {
        return Err(invalid("body", InvalidReason::BodyInvalid));
    }
    let version = body
        .get("version")
        .and_then(serde_json::Value::as_i64)
        .filter(|version| *version >= 1)
        .ok_or(invalid("version", InvalidReason::VersionInvalid))?;
    let listed = body
        .get("consumers")
        .and_then(serde_json::Value::as_array)
        .ok_or(invalid("consumers", InvalidReason::ConsumersInvalid))?;
    let mut consumers = BTreeSet::new();
    for entry in listed {
        let (Some(kind), Some(name), 2) = (
            entry.get("kind").and_then(serde_json::Value::as_str),
            entry.get("name").and_then(serde_json::Value::as_str),
            entry.as_object().map_or(0, serde_json::Map::len),
        ) else {
            return Err(invalid("consumers", InvalidReason::ConsumersInvalid));
        };
        let valid = match kind {
            "route" => crate::admin::route_name_valid(name),
            "connector" => crate::config::valid_mcp_name(name),
            "engine" => crate::decision_engines::valid_engine_name(name),
            _ => false,
        };
        if !valid {
            return Err(invalid("consumers", InvalidReason::ConsumersInvalid));
        }
        consumers.insert(Consumer {
            kind: kind.to_owned(),
            name: name.to_owned(),
        });
    }
    if consumers.len() > MAX_CONSUMERS {
        return Err(invalid("consumers", InvalidReason::ConsumersInvalid));
    }
    Ok((consumers.into_iter().collect(), version))
}

#[cfg(test)]
#[path = "bindings_tests.rs"]
mod tests;
