use super::{SecretFiles, SecretRef, SecretStore, SecretStoreConfig, crypto::Cipher, files};
use crate::{GatewayError, settings::KeyedDigest};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use sqlx::PgPool;
use std::collections::BTreeMap;
use tokio_util::sync::CancellationToken;

/// Authenticated encryption in `PostgreSQL`, with a separate private install key and no value cache.
pub struct BuiltinSecretStore {
    pub(super) pool: PgPool,
    pub(super) cipher: Cipher,
    pub(super) seed: KeyedDigest,
    pub(super) requests: KeyedDigest,
    pub(super) config: SecretStoreConfig,
    pub(super) cancel: CancellationToken,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Seed {
    secrets: BTreeMap<String, SecretString>,
}

impl BuiltinSecretStore {
    /// Opens or creates the private install key and applies an optional private seed file under the seed-change rule
    /// (ADR-0028): only entries whose seed value changed since the last import are written.
    ///
    /// # Errors
    /// Fails closed on unsafe files, invalid seeds, unavailable storage, deadlines or cancellation.
    pub async fn open(
        pool: PgPool,
        paths: SecretFiles,
        config: SecretStoreConfig,
        cancel: CancellationToken,
    ) -> Result<Self, GatewayError> {
        config.validate()?;
        let key = crate::io::bounded(
            config.timeout(),
            &cancel,
            files::load_key(&paths.install_key),
        )
        .await?;
        let store = Self {
            pool,
            cipher: Cipher::new(&key)?,
            seed: KeyedDigest::new(&key, b"tqa seed digest v1"),
            requests: KeyedDigest::new(&key, b"tqa request fingerprint v1"),
            config,
            cancel,
        };
        if let Some(path) = paths.seed {
            let bytes = crate::io::bounded(
                store.config.timeout(),
                &store.cancel,
                files::read_private(&path, store.config.seed_bytes),
            )
            .await?;
            let source = std::str::from_utf8(bytes.expose_secret())
                .map_err(|_| GatewayError::Configuration("seed file must be UTF-8 TOML"))?;
            let seed: Seed = toml::from_str(source).map_err(|_| {
                GatewayError::Configuration("seed file has invalid or unknown fields")
            })?;
            let entries = seed
                .secrets
                .into_iter()
                .map(|(name, value)| Ok((name.parse()?, value)))
                .collect::<Result<Vec<_>, GatewayError>>()?;
            store.apply_seed(entries).await?;
        } else {
            store.forget_seed().await?;
        }
        Ok(store)
    }

    async fn put_entries(
        &self,
        entries: Vec<(SecretRef, SecretString)>,
    ) -> Result<(), GatewayError> {
        if entries.len() > self.config.references as usize {
            return Err(GatewayError::Busy);
        }
        let records = entries
            .iter()
            .map(|(reference, value)| {
                Ok((
                    reference,
                    self.cipher.encrypt(reference, value, &self.config)?,
                ))
            })
            .collect::<Result<Vec<_>, GatewayError>>()?;
        crate::io::bounded(self.config.timeout(), &self.cancel, async {
            let mut transaction = self.pool.begin().await?;
            self.lock(&mut transaction).await?;
            for (reference, (nonce, ciphertext)) in records {
                sqlx::query!("INSERT INTO secrets (ref,nonce,ciphertext) VALUES ($1,$2,$3) ON CONFLICT (ref) DO UPDATE SET nonce=EXCLUDED.nonce,ciphertext=EXCLUDED.ciphertext,version=secrets.version+1,last_set_by='admin',updated_at=CURRENT_TIMESTAMP,rotated_at=CURRENT_TIMESTAMP", reference.as_str(), nonce, ciphertext).execute(&mut *transaction).await?;
            }
            self.check_count(&mut transaction).await?;
            transaction.commit().await?;
            Ok(())
        }).await?;
        for (reference, _) in &entries {
            tracing::info!(
                secret_ref = crate::secrets::loggable(reference.as_str()),
                "secret reference updated"
            );
        }
        tracing::info!(count = entries.len(), "secret updates committed");
        Ok(())
    }
}

impl BuiltinSecretStore {
    /// Serialises every write to the store.
    pub(super) async fn lock(
        &self,
        connection: &mut sqlx::PgConnection,
    ) -> Result<(), GatewayError> {
        crate::db::queue(connection, crate::db::Queue::Named("tqa.secret-store")).await?;
        Ok(())
    }

    /// Refuses a transaction that would leave more references than the store's cap.
    pub(super) async fn check_count(
        &self,
        connection: &mut sqlx::PgConnection,
    ) -> Result<(), GatewayError> {
        let count = sqlx::query_scalar!("SELECT count(*) AS \"count!\" FROM secrets")
            .fetch_one(connection)
            .await?;
        if count > i64::from(self.config.references) {
            return Err(GatewayError::Busy);
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl SecretStore for BuiltinSecretStore {
    async fn put(&self, reference: &SecretRef, value: SecretString) -> Result<(), GatewayError> {
        self.put_entries(vec![(reference.clone(), value)]).await
    }
    async fn get(&self, reference: &SecretRef) -> Result<Option<SecretString>, GatewayError> {
        let row = crate::io::bounded(self.config.timeout(), &self.cancel, async {
            sqlx::query!(
                "SELECT nonce,ciphertext FROM secrets WHERE ref=$1",
                reference.as_str()
            )
            .fetch_optional(&self.pool)
            .await
            .map_err(GatewayError::from)
        })
        .await?;
        row.map(|record| {
            self.cipher
                .decrypt(reference, &record.nonce, &record.ciphertext)
        })
        .transpose()
    }
    async fn delete(&self, reference: &SecretRef) -> Result<(), GatewayError> {
        crate::io::bounded(self.config.timeout(), &self.cancel, async {
            sqlx::query!("DELETE FROM secrets WHERE ref=$1", reference.as_str())
                .execute(&self.pool)
                .await?;
            Ok(())
        })
        .await
    }
    async fn entries(&self) -> Result<Vec<super::SecretEntry>, GatewayError> {
        self.admin_entries().await
    }
    async fn write(
        &self,
        connection: &mut sqlx::PgConnection,
        write: super::SecretWrite,
    ) -> Result<(bool, super::SecretEntry), GatewayError> {
        self.admin_write(connection, write).await
    }
    async fn remind(
        &self,
        connection: &mut sqlx::PgConnection,
        reference: &SecretRef,
        days: Option<i32>,
        version: (i64, uuid::Uuid),
    ) -> Result<super::SecretEntry, GatewayError> {
        self.admin_remind(connection, reference, days, version)
            .await
    }
    async fn remove(
        &self,
        connection: &mut sqlx::PgConnection,
        reference: &SecretRef,
        expected: i64,
        actor: uuid::Uuid,
    ) -> Result<(), GatewayError> {
        self.admin_remove(connection, reference, expected, actor)
            .await
    }
    fn fingerprint(
        &self,
        reference: &SecretRef,
        value: &SecretString,
    ) -> Result<[u8; 32], GatewayError> {
        Ok(self.request_fingerprint(reference, value))
    }
    async fn refs(&self) -> Result<Vec<SecretRef>, GatewayError> {
        let limit = i64::from(self.config.references) + 1;
        let names = crate::io::bounded(self.config.timeout(), &self.cancel, async {
            sqlx::query_scalar!("SELECT ref FROM secrets ORDER BY ref LIMIT $1", limit)
                .fetch_all(&self.pool)
                .await
                .map_err(GatewayError::from)
        })
        .await?;
        if names.len() > self.config.references as usize {
            return Err(GatewayError::Busy);
        }
        names
            .into_iter()
            .map(|name| name.parse().map_err(|_| GatewayError::SecretUnavailable))
            .collect()
    }
}
