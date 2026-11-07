//! The seed-change rule (ADR-0028): a seed entry applies only when its keyed digest differs from the one last imported,
//! so a rotation made after the import survives restarts and an operator's file edit still wins. References the file no
//! longer lists are kept and marked removed.
use super::{SecretRef, builtin::BuiltinSecretStore};
use crate::{
    GatewayError,
    settings::{Change, audit, tombstones},
};
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgConnection;
use subtle::ConstantTimeEq;

impl BuiltinSecretStore {
    /// Applies the seed file's entries under the store lock, in one transaction.
    pub(super) async fn apply_seed(
        &self,
        entries: Vec<(SecretRef, SecretString)>,
    ) -> Result<(), GatewayError> {
        if entries.len() > self.config.references as usize {
            return Err(GatewayError::Busy);
        }
        let applied = crate::io::bounded(self.config.timeout(), &self.cancel, async {
            let mut transaction = self.pool.begin().await?;
            self.lock(&mut transaction).await?;
            let mut applied = 0_usize;
            for (reference, value) in &entries {
                applied += usize::from(self.seed_entry(&mut transaction, reference, value).await?);
            }
            let listed: Vec<String> = entries.iter().map(|(reference, _)| reference.as_str().to_owned()).collect();
            tombstones::forget_unlisted(&mut transaction, "secrets", &listed).await?;
            sqlx::query!(
                "UPDATE secrets SET seed_state='removed' WHERE seed_digest IS NOT NULL AND NOT (ref = ANY($1))",
                &listed
            )
            .execute(&mut *transaction)
            .await?;
            self.check_count(&mut transaction).await?;
            transaction.commit().await?;
            Ok::<_, GatewayError>(applied)
        })
        .await?;
        tracing::info!(listed = entries.len(), applied, "secret seed applied");
        Ok(())
    }

    /// Imports one entry when it is new or its seed value changed; returns whether it wrote.
    async fn seed_entry(
        &self,
        connection: &mut PgConnection,
        reference: &SecretRef,
        value: &SecretString,
    ) -> Result<bool, GatewayError> {
        warn_if_key_shaped(reference);
        let digest = self
            .seed
            .of(reference.as_str(), value.expose_secret().as_bytes());
        let current = sqlx::query!(
            "SELECT version,seed_digest,nonce,ciphertext FROM secrets WHERE ref=$1 FOR UPDATE",
            reference.as_str()
        )
        .fetch_optional(&mut *connection)
        .await?;
        let (nonce, ciphertext) = self.cipher.encrypt(reference, value, &self.config)?;
        let Some(current) = current else {
            if tombstones::buried(connection, "secrets", reference.as_str(), &digest).await? {
                return Ok(false);
            }
            sqlx::query!(
                "INSERT INTO secrets (ref,nonce,ciphertext,version,last_set_by,seed_digest,seed_state) VALUES ($1,$2,$3,1,'seed',$4,'current')",
                reference.as_str(), nonce, ciphertext, &digest[..]
            )
            .execute(&mut *connection)
            .await?;
            let change = Change {
                area: "secrets",
                actor: None,
                item: reference.as_str(),
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
            .is_some_and(|stored| bool::from(stored.ct_eq(&digest[..])))
        {
            sqlx::query!(
                "UPDATE secrets SET seed_state='current' WHERE ref=$1",
                reference.as_str()
            )
            .execute(&mut *connection)
            .await?;
            return Ok(false);
        }
        let previous = self
            .cipher
            .decrypt(reference, &current.nonce, &current.ciphertext)?;
        let changed = !bool::from(
            previous
                .expose_secret()
                .as_bytes()
                .ct_eq(value.expose_secret().as_bytes()),
        );
        let version = current.version + 1;
        sqlx::query!(
            "UPDATE secrets SET nonce=$2,ciphertext=$3,version=$4,last_set_by='seed',seed_digest=$5,seed_state='current',updated_at=CURRENT_TIMESTAMP,\
             rotated_at=CASE WHEN $6 THEN CURRENT_TIMESTAMP ELSE rotated_at END WHERE ref=$1",
            reference.as_str(), nonce, ciphertext, version, &digest[..], changed
        )
        .execute(&mut *connection)
        .await?;
        let change = Change {
            area: "secrets",
            actor: None,
            item: reference.as_str(),
            action: "replace",
            before: Some(current.version),
            after: Some(version),
            value_changed: changed,
        };
        audit(connection, &change).await?;
        Ok(true)
    }

    /// Without a seed file every seeded reference is no longer seeded.
    pub(super) async fn forget_seed(&self) -> Result<(), GatewayError> {
        crate::io::bounded(self.config.timeout(), &self.cancel, async {
            sqlx::query!("UPDATE secrets SET seed_state='removed' WHERE seed_digest IS NOT NULL")
                .execute(&self.pool)
                .await?;
            Ok(())
        })
        .await
    }
}

/// A seeded name that looks like a key (SEC-033) is never refused, since an upgrade must start, and never named,
/// since it may be one.
fn warn_if_key_shaped(reference: &SecretRef) {
    if tqa_types::is_key_shaped_name(reference.as_str()) {
        tracing::warn!(
            "a secret in the secrets seed file has a name that looks like a key, not a name: rename it there and in \
             what names it"
        );
    }
}
