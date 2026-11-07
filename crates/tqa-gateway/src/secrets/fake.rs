use super::{SecretRef, SecretStore, SecretStoreConfig};
use crate::GatewayError;
use secrecy::{ExposeSecret, SecretString};
use std::{
    collections::BTreeMap,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

/// In-memory store for consumers' tests, with the same value bounds and an explicit outage switch.
#[derive(Default)]
pub struct FakeSecretStore {
    values: Mutex<BTreeMap<SecretRef, SecretString>>,
    unavailable: AtomicBool,
}
impl FakeSecretStore {
    /// Enables or clears a deterministic outage; stored values are retained during the outage.
    pub fn set_unavailable(&self, unavailable: bool) {
        self.unavailable.store(unavailable, Ordering::Relaxed);
    }
    fn values(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, BTreeMap<SecretRef, SecretString>>, GatewayError> {
        if self.unavailable.load(Ordering::Relaxed) {
            return Err(GatewayError::SecretUnavailable);
        }
        self.values
            .lock()
            .map_err(|_| GatewayError::SecretUnavailable)
    }
}
#[async_trait::async_trait]
impl SecretStore for FakeSecretStore {
    async fn put(&self, reference: &SecretRef, value: SecretString) -> Result<(), GatewayError> {
        let config = SecretStoreConfig::default();
        config.value(value.expose_secret())?;
        let mut values = self.values()?;
        if values.len() >= config.references as usize && !values.contains_key(reference) {
            return Err(GatewayError::Busy);
        }
        values.insert(reference.clone(), value);
        Ok(())
    }
    async fn get(&self, reference: &SecretRef) -> Result<Option<SecretString>, GatewayError> {
        Ok(self.values()?.get(reference).cloned())
    }
    async fn delete(&self, reference: &SecretRef) -> Result<(), GatewayError> {
        self.values()?.remove(reference);
        Ok(())
    }
    async fn refs(&self) -> Result<Vec<SecretRef>, GatewayError> {
        Ok(self.values()?.keys().cloned().collect())
    }
}
