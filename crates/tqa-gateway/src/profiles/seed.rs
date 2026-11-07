//! Shared profiles seeded from `[[profiles]]` (ADR-0012, ADR-0028, GW-I-088). A seeded profile is imported when it is
//! new or its keyed seed digest changed; an unchanged seed never overwrites an administrator's edit; a deleted one
//! stays deleted until its seed changes (tombstones); one the file no longer lists is kept and marked `removed`. As
//! for routes and templates, the first import of a name replaces an administrator-created shared profile of that name.
//! A name the install does not have (a route, template, server or connector) is logged at WARN, never a failed start.
use sqlx::PgConnection;
use subtle::ConstantTimeEq;

use super::{document::Profile, store};
use crate::{
    GatewayError,
    config::McpServer,
    settings::{Change, KeyedDigest, Snapshot, audit, tombstones},
};

/// Imports the file's shared profiles once the settings are loaded, warning about any name the install does not have.
///
/// # Errors
/// Storage failures.
pub(crate) async fn seed(
    connection: &mut PgConnection,
    profiles: &[Profile],
    digest: &KeyedDigest,
    (snapshot, servers): (&Snapshot, &[McpServer]),
) -> Result<(), GatewayError> {
    for profile in profiles {
        // The install's names change through the API, so a name that has gone must never stop start-up: the profile
        // is seeded as usual and shown unusable until the name exists again (GW-I-086).
        if let Some(field) = super::unknown(snapshot, servers, &super::References::of(profile)) {
            tracing::warn!(
                profile = %profile.name,
                field,
                "a [[profiles]] entry names what the install does not have; it is unusable until it does"
            );
        }
        seed_one(connection, profile, digest).await?;
    }
    let names: Vec<String> = profiles
        .iter()
        .map(|profile| profile.name.clone())
        .collect();
    tombstones::forget_unlisted(connection, "profiles", &names).await?;
    sqlx::query!(
        "UPDATE agent_profiles SET seed_state='removed' \
         WHERE owner_id IS NULL AND seed_state='current' AND NOT (name = ANY($1))",
        &names
    )
    .execute(&mut *connection)
    .await?;
    tracing::info!(profiles = profiles.len(), "profiles seed applied");
    Ok(())
}

fn change<'a>(name: &'a str, action: &'static str, versions: (Option<i64>, i64)) -> Change<'a> {
    Change {
        area: "profiles",
        actor: None,
        item: name,
        action,
        before: versions.0,
        after: Some(versions.1),
        value_changed: true,
    }
}

async fn seed_one(
    connection: &mut PgConnection,
    profile: &Profile,
    digest: &KeyedDigest,
) -> Result<(), GatewayError> {
    let document = serde_json::to_value(profile)?;
    let seed = digest.of(
        &format!("profiles/{}", profile.name),
        &serde_json::to_vec(&document)?,
    );
    let current = sqlx::query!(
        "SELECT id,version,seed_digest,seed_state FROM agent_profiles \
         WHERE owner_id IS NULL AND name=$1 FOR UPDATE",
        profile.name
    )
    .fetch_optional(&mut *connection)
    .await?;
    let Some(current) = current else {
        if tombstones::buried(connection, "profiles", &profile.name, &seed).await? {
            return Ok(());
        }
        store::insert(connection, None, profile, ("seed", Some(&seed[..]))).await?;
        return audit(connection, &change(&profile.name, "create", (None, 1))).await;
    };
    let unchanged = current
        .seed_digest
        .as_deref()
        .is_some_and(|stored| bool::from(stored.ct_eq(&seed[..])));
    if unchanged {
        if current.seed_state.as_deref() != Some("current") {
            sqlx::query!(
                "UPDATE agent_profiles SET seed_state='current' WHERE id=$1",
                current.id
            )
            .execute(&mut *connection)
            .await?;
        }
        return Ok(());
    }
    sqlx::query!(
        "UPDATE agent_profiles SET document=$2,version=version+1,last_set_by='seed',seed_digest=$3,seed_state='current',\
         updated_at=CURRENT_TIMESTAMP WHERE id=$1",
        current.id,
        document,
        &seed[..]
    )
    .execute(&mut *connection)
    .await?;
    let versions = (Some(current.version), current.version + 1);
    audit(connection, &change(&profile.name, "replace", versions)).await
}
