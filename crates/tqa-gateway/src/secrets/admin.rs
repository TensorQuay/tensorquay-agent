//! Versioned administrator writes (090, ADR-0028): each runs in the caller's transaction under the store lock, checks
//! the version the administrator read, and records one `admin_audit` row; values never leave the store.
use super::{SecretRef, builtin::BuiltinSecretStore};
use crate::{
    GatewayError,
    settings::{Change, audit, tombstones},
};
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgConnection;
use subtle::ConstantTimeEq;

/// Administration metadata for one reference; never its value, length or a hash of it.
#[derive(Clone, Debug)]
pub struct SecretEntry {
    /// The reference, `secrets/<name>`.
    pub reference: SecretRef,
    /// When the current value was written.
    pub updated_at: time::OffsetDateTime,
    /// Bumped by every write; administrators send it back to replace or delete.
    pub version: i64,
    /// `seed` or `admin`: who wrote the current value.
    pub last_set_by: String,
    /// `current` while the seed file lists it, `removed` after it no longer does; `None` if never seeded.
    pub seed_state: Option<String>,
    /// When the value last changed: a write of the same value does not count (vault slice 1).
    pub rotated_at: time::OffsetDateTime,
    /// The rotation reminder's interval in days, if one is set.
    pub rotate_after_days: Option<i32>,
}

/// One administrator write.
pub struct SecretWrite {
    /// The reference to create or replace.
    pub reference: SecretRef,
    /// The new value.
    pub value: SecretString,
    /// The version read by the administrator; `None` creates, and must then find no current value.
    pub expected: Option<i64>,
    /// The administrator's user id, for the audit row.
    pub actor: uuid::Uuid,
}

impl BuiltinSecretStore {
    pub(super) async fn admin_entries(&self) -> Result<Vec<SecretEntry>, GatewayError> {
        let limit = i64::from(self.config.references);
        let rows = crate::io::bounded(self.config.timeout(), &self.cancel, async {
            Ok(sqlx::query!(
                "SELECT ref,updated_at,version,last_set_by,seed_state,rotated_at,rotate_after_days FROM secrets ORDER BY ref LIMIT $1",
                limit
            )
            .fetch_all(&self.pool)
            .await?)
        })
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(SecretEntry {
                    reference: row
                        .r#ref
                        .parse()
                        .map_err(|_| GatewayError::SecretUnavailable)?,
                    updated_at: row.updated_at,
                    version: row.version,
                    last_set_by: row.last_set_by,
                    seed_state: row.seed_state,
                    rotated_at: row.rotated_at,
                    rotate_after_days: row.rotate_after_days,
                })
            })
            .collect()
    }

    /// Creates (`expected` is `None` and the reference is absent) or replaces (`expected` is the current version).
    /// Returns whether it created, and the new metadata.
    pub(super) async fn admin_write(
        &self,
        connection: &mut PgConnection,
        write: SecretWrite,
    ) -> Result<(bool, SecretEntry), GatewayError> {
        let (nonce, ciphertext) =
            self.cipher
                .encrypt(&write.reference, &write.value, &self.config)?;
        self.lock(connection).await?;
        let current = sqlx::query!(
            "SELECT version,nonce,ciphertext FROM secrets WHERE ref=$1 FOR UPDATE",
            write.reference.as_str()
        )
        .fetch_optional(&mut *connection)
        .await?;
        let change = match (current, write.expected) {
            (None, None) => {
                self.admin_create(connection, &write.reference, &nonce, &ciphertext)
                    .await?;
                (true, None, 1, true)
            }
            (Some(current), Some(expected)) if current.version == expected => {
                let previous =
                    self.cipher
                        .decrypt(&write.reference, &current.nonce, &current.ciphertext)?;
                let same = previous
                    .expose_secret()
                    .as_bytes()
                    .ct_eq(write.value.expose_secret().as_bytes());
                sqlx::query!(
                    "UPDATE secrets SET nonce=$2,ciphertext=$3,version=version+1,last_set_by='admin',updated_at=CURRENT_TIMESTAMP,\
                     rotated_at=CASE WHEN $4 THEN CURRENT_TIMESTAMP ELSE rotated_at END WHERE ref=$1",
                    write.reference.as_str(), nonce, ciphertext, !bool::from(same)
                )
                .execute(&mut *connection)
                .await?;
                (false, Some(expected), expected + 1, !bool::from(same))
            }
            _ => return Err(GatewayError::Conflict("conflict")),
        };
        let (created, before, after, value_changed) = change;
        let action = if created { "create" } else { "replace" };
        let record = Change {
            area: "secrets",
            actor: Some(write.actor),
            item: write.reference.as_str(),
            action,
            before,
            after: Some(after),
            value_changed,
        };
        audit(connection, &record).await?;
        let row = sqlx::query!(
            "SELECT updated_at,version,last_set_by,seed_state,rotated_at,rotate_after_days FROM secrets WHERE ref=$1",
            write.reference.as_str()
        )
        .fetch_one(&mut *connection)
        .await?;
        tracing::info!(
            secret_ref = crate::secrets::loggable(write.reference.as_str()),
            version = row.version,
            "secret reference updated by an administrator"
        );
        Ok((
            created,
            SecretEntry {
                reference: write.reference,
                updated_at: row.updated_at,
                version: row.version,
                last_set_by: row.last_set_by,
                seed_state: row.seed_state,
                rotated_at: row.rotated_at,
                rotate_after_days: row.rotate_after_days,
            },
        ))
    }

    /// Sets or clears the rotation reminder (`expected` is the current version); versioned and audited as a replace
    /// that changes no value.
    pub(super) async fn admin_remind(
        &self,
        connection: &mut sqlx::PgConnection,
        reference: &SecretRef,
        days: Option<i32>,
        (expected, actor): (i64, uuid::Uuid),
    ) -> Result<SecretEntry, GatewayError> {
        let row = sqlx::query!(
            "UPDATE secrets SET rotate_after_days=$2,version=version+1 WHERE ref=$1 AND version=$3 \
             RETURNING updated_at,version,last_set_by,seed_state,rotated_at,rotate_after_days",
            reference.as_str(),
            days,
            expected
        )
        .fetch_optional(&mut *connection)
        .await?;
        let Some(row) = row else {
            let exists = sqlx::query_scalar!(
                "SELECT version FROM secrets WHERE ref=$1",
                reference.as_str()
            )
            .fetch_optional(&mut *connection)
            .await?;
            return Err(if exists.is_some() {
                GatewayError::Conflict("conflict")
            } else {
                GatewayError::NotFound
            });
        };
        let change = Change {
            area: "secrets",
            actor: Some(actor),
            item: reference.as_str(),
            action: "replace",
            before: Some(expected),
            after: Some(row.version),
            value_changed: false,
        };
        audit(connection, &change).await?;
        Ok(SecretEntry {
            reference: reference.clone(),
            updated_at: row.updated_at,
            version: row.version,
            last_set_by: row.last_set_by,
            seed_state: row.seed_state,
            rotated_at: row.rotated_at,
            rotate_after_days: row.rotate_after_days,
        })
    }

    /// Inserts a new reference within the store's bound. A seeded secret deleted earlier keeps its seed digest, so an
    /// unchanged seed does not overwrite this one.
    async fn admin_create(
        &self,
        connection: &mut PgConnection,
        reference: &SecretRef,
        nonce: &[u8],
        ciphertext: &[u8],
    ) -> Result<(), GatewayError> {
        let count = sqlx::query_scalar!("SELECT count(*) AS \"count!\" FROM secrets")
            .fetch_one(&mut *connection)
            .await?;
        if count >= i64::from(self.config.references) {
            return Err(GatewayError::Conflict("secret_limit"));
        }
        let digest = tombstones::unearth(connection, "secrets", reference.as_str()).await?;
        let state = digest.as_ref().map(|_| "current");
        sqlx::query!(
            "INSERT INTO secrets (ref,nonce,ciphertext,version,last_set_by,seed_digest,seed_state) VALUES ($1,$2,$3,1,'admin',$4,$5)",
            reference.as_str(), nonce, ciphertext, digest, state
        )
        .execute(&mut *connection)
        .await?;
        Ok(())
    }

    /// Deletes the reference when its version is `expected`.
    pub(super) async fn admin_remove(
        &self,
        connection: &mut PgConnection,
        reference: &SecretRef,
        expected: i64,
        actor: uuid::Uuid,
    ) -> Result<(), GatewayError> {
        self.lock(connection).await?;
        let current = sqlx::query!(
            "SELECT version,seed_digest FROM secrets WHERE ref=$1 FOR UPDATE",
            reference.as_str()
        )
        .fetch_optional(&mut *connection)
        .await?
        .ok_or(GatewayError::NotFound)?;
        let version = current.version;
        if version != expected {
            return Err(GatewayError::Conflict("conflict"));
        }
        sqlx::query!("DELETE FROM secrets WHERE ref=$1", reference.as_str())
            .execute(&mut *connection)
            .await?;
        tombstones::bury(
            connection,
            "secrets",
            reference.as_str(),
            current.seed_digest.as_deref(),
        )
        .await?;
        let record = Change {
            area: "secrets",
            actor: Some(actor),
            item: reference.as_str(),
            action: "delete",
            before: Some(version),
            after: None,
            value_changed: true,
        };
        audit(connection, &record).await?;
        tracing::info!(
            secret_ref = crate::secrets::loggable(reference.as_str()),
            "secret reference deleted by an administrator"
        );
        Ok(())
    }

    /// A keyed digest of a request value for idempotency fingerprints; never a plain hash of the value.
    pub(super) fn request_fingerprint(
        &self,
        reference: &SecretRef,
        value: &SecretString,
    ) -> [u8; 32] {
        self.requests
            .of(reference.as_str(), value.expose_secret().as_bytes())
    }
}
