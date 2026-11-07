//! Gateway-owned credentials; consumers receive secret values only through the store boundary.

mod admin;
pub(crate) mod bindings;
mod builtin;
mod config;
mod crypto;
mod fake;
mod files;
pub(crate) use files::read_private;
pub(crate) mod reads;
mod reference;
pub(crate) mod rotation;
mod seed;

#[cfg(test)]
mod file_tests;
#[cfg(test)]
mod tests;

use crate::GatewayError;
pub use admin::{SecretEntry, SecretWrite};
pub use builtin::BuiltinSecretStore;
pub use config::{SecretFiles, SecretStoreConfig};
pub use fake::FakeSecretStore;
pub use reference::SecretRef;
use secrecy::SecretString;

/// A bounded, fallible credential store. Values never appear in listings, errors or logs.
#[async_trait::async_trait]
pub trait SecretStore: Send + Sync {
    /// Replaces a credential; the next read observes the committed value without a restart.
    ///
    /// # Errors
    /// Rejects invalid sizes, exhausted capacity, cancellation and storage failure.
    async fn put(&self, reference: &SecretRef, value: SecretString) -> Result<(), GatewayError>;
    /// Resolves the current value, or `None` if the reference does not exist.
    ///
    /// # Errors
    /// Fails closed on corruption, authentication failure, cancellation or storage failure.
    async fn get(&self, reference: &SecretRef) -> Result<Option<SecretString>, GatewayError>;
    /// Deletes a reference; absence is success.
    ///
    /// # Errors
    /// Returns cancellation or storage failure if deletion cannot be confirmed.
    async fn delete(&self, reference: &SecretRef) -> Result<(), GatewayError>;
    /// Lists bounded, sorted references without returning their values.
    ///
    /// # Errors
    /// Rejects exhausted capacity, cancellation and storage failure.
    async fn refs(&self) -> Result<Vec<SecretRef>, GatewayError>;
    /// Administration metadata for every reference, sorted by reference; never values (090).
    ///
    /// # Errors
    /// Storage failure, or a store without administration.
    async fn entries(&self) -> Result<Vec<SecretEntry>, GatewayError> {
        Err(UNSUPPORTED)
    }
    /// A versioned administrator write in the caller's transaction, audited; returns whether it created.
    ///
    /// # Errors
    /// `409 conflict` for a stale, missing or unexpected version, `409 secret_limit` at the cap, invalid values,
    /// storage failure.
    async fn write(
        &self,
        _connection: &mut sqlx::PgConnection,
        _write: SecretWrite,
    ) -> Result<(bool, SecretEntry), GatewayError> {
        Err(UNSUPPORTED)
    }
    /// A versioned, audited administrator delete in the caller's transaction.
    ///
    /// # Errors
    /// `404` when absent, `409 conflict` for a stale version, storage failure.
    async fn remove(
        &self,
        _connection: &mut sqlx::PgConnection,
        _reference: &SecretRef,
        _expected: i64,
        _actor: uuid::Uuid,
    ) -> Result<(), GatewayError> {
        Err(UNSUPPORTED)
    }
    /// Sets or clears the rotation reminder of an existing reference at version `expected`, in the caller's
    /// transaction; versioned and audited (vault slice 1).
    ///
    /// # Errors
    /// `404` when absent, `409 conflict` for a stale version, storage failure.
    async fn remind(
        &self,
        _connection: &mut sqlx::PgConnection,
        _reference: &SecretRef,
        _days: Option<i32>,
        _version: (i64, uuid::Uuid),
    ) -> Result<SecretEntry, GatewayError> {
        Err(UNSUPPORTED)
    }
    /// A keyed digest of a value for request fingerprints; never a plain hash of it.
    ///
    /// # Errors
    /// A store without administration.
    fn fingerprint(
        &self,
        _reference: &SecretRef,
        _value: &SecretString,
    ) -> Result<[u8; 32], GatewayError> {
        Err(UNSUPPORTED)
    }
}

/// Stores other than the built-in one do not support administration.
const UNSUPPORTED: GatewayError =
    GatewayError::Configuration("secret administration needs the built-in store");

/// The keyed digest for seeded settings (ADR-0028), under the same install key and label as the secrets seed; each
/// area prefixes its items (`routes/<name>`), so no digest of one item can match another's.
///
/// # Errors
/// A missing or unsafe install key, a deadline or cancellation.
pub(crate) async fn seed_digest(
    config: &crate::GatewayConfig,
    secrets: &crate::GatewaySecrets,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<crate::settings::KeyedDigest, GatewayError> {
    let files = secrets
        .secret_files
        .as_ref()
        .ok_or(GatewayError::Configuration(
            "install-key file must be configured",
        ))?;
    let key = crate::io::bounded(
        config.io_timeout(),
        cancel,
        files::load_key(&files.install_key),
    )
    .await?;
    Ok(crate::settings::KeyedDigest::new(
        &key,
        b"tqa seed digest v1",
    ))
}

pub(crate) async fn open(
    pool: &sqlx::PgPool,
    config: &crate::GatewayConfig,
    secrets: &crate::GatewaySecrets,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<std::sync::Arc<dyn SecretStore>, GatewayError> {
    let files = secrets
        .secret_files
        .clone()
        .ok_or(GatewayError::Configuration(
            "install-key file must be configured",
        ))?;
    let store = BuiltinSecretStore::open(
        pool.clone(),
        files,
        config.secret_store.clone(),
        cancel.clone(),
    )
    .await?;
    Ok(std::sync::Arc::new(store))
}

/// What the log may show of a secret reference: a name shaped like a key (one stored before SEC-033's check, from a
/// seed file, or a reference a file still names) is never written out, as a key pasted where a name belongs would then
/// reach the log (GW-U-165). Every log field carrying a reference goes through this.
#[must_use]
pub(crate) fn loggable(reference: &str) -> &str {
    if tqa_types::is_key_shaped_name(reference) {
        "secrets/<a name shaped like a key>"
    } else {
        reference
    }
}

#[cfg(test)]
#[path = "secrets/loggable_tests.rs"]
mod loggable_tests;
