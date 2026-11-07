//! The `agent_profiles` table (ADR-0012). A personal profile is visible to its owner only; a shared one (no owner) to
//! everyone. The document holds the validated fields; the row adds the id, sharing, version and time.
use serde::Deserialize as _;
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

use super::{References, document::Profile};
use crate::GatewayError;

/// One stored profile.
pub(crate) struct Record {
    pub id: Uuid,
    pub owner_id: Option<Uuid>,
    pub document: Value,
    pub version: i64,
    pub updated_at: time::OffsetDateTime,
}

impl Record {
    /// The names the stored document uses from the install.
    ///
    /// # Errors
    /// A stored document of another shape.
    pub(crate) fn references(&self) -> Result<References, GatewayError> {
        Ok(References::deserialize(&self.document)?)
    }

    /// The API form: the document's fields, with the id, whether it is shared, the version, the time, and whether a
    /// conversation can be made from it now: `unusable` names the first field the install no longer has (GW-I-086).
    ///
    /// # Errors
    /// A time that cannot be formatted.
    pub(crate) fn to_json(&self, unusable: Option<&'static str>) -> Result<Value, GatewayError> {
        let mut value = self.document.clone();
        value["usable"] = Value::Bool(unusable.is_none());
        if let Some(field) = unusable {
            value["unusable_field"] = Value::String(field.to_owned());
        }
        value["id"] = Value::String(self.id.to_string());
        value["shared"] = Value::Bool(self.owner_id.is_none());
        value["version"] = Value::from(self.version);
        value["updated_at"] = Value::String(
            self.updated_at
                .format(&time::format_description::well_known::Rfc3339)
                .map_err(|_| GatewayError::Io)?,
        );
        Ok(value)
    }
}

/// A write that would give two profiles of one owner (or two shared ones) the same name.
fn write_error(error: sqlx::Error) -> GatewayError {
    if error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
    {
        GatewayError::Conflict("name_taken")
    } else {
        GatewayError::from(error)
    }
}

/// The caller's own profiles, then the shared ones, each by name.
///
/// # Errors
/// Storage failures.
pub(crate) async fn visible(
    connection: &mut PgConnection,
    user: Uuid,
) -> Result<Vec<Record>, GatewayError> {
    Ok(sqlx::query_as!(
        Record,
        "SELECT id,owner_id,document,version,updated_at FROM agent_profiles WHERE owner_id=$1 OR owner_id IS NULL \
         ORDER BY owner_id IS NULL, name",
        user
    )
    .fetch_all(connection)
    .await?)
}

/// One profile the caller may see, locked for a write: their own or a shared one. Anyone else's is not found.
///
/// # Errors
/// `404` for a profile the caller may not see; storage failures.
pub(crate) async fn find(
    connection: &mut PgConnection,
    user: Uuid,
    id: Uuid,
) -> Result<Record, GatewayError> {
    sqlx::query_as!(
        Record,
        "SELECT id,owner_id,document,version,updated_at FROM agent_profiles \
         WHERE id=$1 AND (owner_id=$2 OR owner_id IS NULL) FOR UPDATE",
        id,
        user
    )
    .fetch_optional(connection)
    .await?
    .ok_or(GatewayError::NotFound)
}

/// Personal profiles one user may hold (review R1 of slice 1).
pub(crate) const MAX_PROFILES_PER_OWNER: i64 = 100;
/// Shared profiles the API may create; the file may seed as many again.
pub(crate) const MAX_SHARED_PROFILES: i64 = 64;

/// Refuses a new profile past its bound, counted under a lock so two creates cannot both pass it: the owner's user row
/// for a personal profile, the settings lock for a shared one.
///
/// # Errors
/// `409 profile_count`; storage failures.
pub(crate) async fn room(
    connection: &mut PgConnection,
    owner: Option<Uuid>,
) -> Result<(), GatewayError> {
    let (count, bound) = if let Some(owner) = owner {
        sqlx::query!("SELECT id FROM users WHERE id=$1 FOR UPDATE", owner)
            .fetch_one(&mut *connection)
            .await?;
        let count = sqlx::query_scalar!(
            "SELECT count(*) AS \"count!\" FROM agent_profiles WHERE owner_id=$1",
            owner
        )
        .fetch_one(&mut *connection)
        .await?;
        (count, MAX_PROFILES_PER_OWNER)
    } else {
        crate::settings::lock(connection).await?;
        // Seeded profiles are bounded by the file, so the two paths never refuse each other.
        let count = sqlx::query_scalar!(
            "SELECT count(*) AS \"count!\" FROM agent_profiles WHERE owner_id IS NULL AND seed_digest IS NULL"
        )
        .fetch_one(&mut *connection)
        .await?;
        (count, MAX_SHARED_PROFILES)
    };
    if count >= bound {
        return Err(GatewayError::Conflict("profile_count"));
    }
    Ok(())
}

/// Stores a new profile: personal with an `owner`, shared without one.
///
/// # Errors
/// `409 name_taken`; storage failures.
pub(crate) async fn insert(
    connection: &mut PgConnection,
    owner: Option<Uuid>,
    profile: &Profile,
    (set_by, seed): (&str, Option<&[u8]>),
) -> Result<Record, GatewayError> {
    let document = serde_json::to_value(profile)?;
    let state = seed.map(|_| "current");
    sqlx::query_as!(
        Record,
        "INSERT INTO agent_profiles (id,owner_id,name,document,last_set_by,seed_digest,seed_state) \
         VALUES ($1,$2,$3,$4,$5,$6,$7) RETURNING id,owner_id,document,version,updated_at",
        Uuid::now_v7(),
        owner,
        profile.name,
        document,
        set_by,
        seed,
        state
    )
    .fetch_one(connection)
    .await
    .map_err(write_error)
}

/// Replaces a profile at the version read.
///
/// # Errors
/// `409 conflict` for another version, `409 name_taken`; storage failures.
pub(crate) async fn replace(
    connection: &mut PgConnection,
    current: &Record,
    expected: i64,
    (profile, set_by): (&Profile, &str),
) -> Result<Record, GatewayError> {
    if current.version != expected {
        return Err(GatewayError::Conflict("conflict"));
    }
    let document = serde_json::to_value(profile)?;
    sqlx::query_as!(
        Record,
        "UPDATE agent_profiles SET name=$2,document=$3,version=version+1,last_set_by=$4,updated_at=CURRENT_TIMESTAMP \
         WHERE id=$1 RETURNING id,owner_id,document,version,updated_at",
        current.id,
        profile.name,
        document,
        set_by
    )
    .fetch_one(connection)
    .await
    .map_err(write_error)
}

/// Deletes a profile at the version read, and returns its seed digest (a seeded shared profile's tombstone).
/// Conversations made from it keep their copy (ADR-0012 decision 5).
///
/// # Errors
/// `409 conflict` for another version; storage failures.
pub(crate) async fn delete(
    connection: &mut PgConnection,
    current: &Record,
    expected: i64,
) -> Result<Option<Vec<u8>>, GatewayError> {
    if current.version != expected {
        return Err(GatewayError::Conflict("conflict"));
    }
    Ok(sqlx::query_scalar!(
        "DELETE FROM agent_profiles WHERE id=$1 RETURNING seed_digest",
        current.id
    )
    .fetch_one(connection)
    .await?)
}
