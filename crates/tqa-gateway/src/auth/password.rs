use crate::{GatewayConfig, GatewayError};
use argon2::{Algorithm, Argon2, Params, PasswordHasher, PasswordVerifier, Version};
use rand::TryRngCore;
use secrecy::{ExposeSecret, SecretString};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::Semaphore;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

/// How long start-up waits for its one hash: a loaded host can need more than the I/O bound (GW-I-129, the R2 soak at
/// load 31), and the gateway can't serve without it. Shutdown still stops the wait.
const START_BOUND: std::time::Duration = std::time::Duration::from_mins(5);

/// How many callers may wait for each password worker. Past them a call is refused at once (`429 busy`), so a burst of
/// sign-ins fails fast instead of each waiting out its bound, and the queue stays bounded.
const WAITING_PER_WORKER: usize = 8;

/// Bounds both password work and the blocking tasks which own sensitive input buffers.
#[derive(Clone)]
pub(crate) struct Passwords {
    admission: Arc<Semaphore>,
    /// Callers working or waiting for a worker, and the most allowed.
    callers: Arc<AtomicUsize>,
    capacity: usize,
    tasks: TaskTracker,
    cancel: CancellationToken,
    timeout: std::time::Duration,
}

/// One admitted caller, counted until it returns however it ends.
struct Admitted(Arc<AtomicUsize>);

impl Drop for Admitted {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

impl Passwords {
    pub fn new(config: &GatewayConfig, tasks: TaskTracker, cancel: CancellationToken) -> Self {
        let workers = config.limits.password_workers;
        Self {
            admission: Arc::new(Semaphore::new(workers)),
            callers: Arc::new(AtomicUsize::new(0)),
            capacity: workers * (1 + WAITING_PER_WORKER),
            tasks,
            cancel,
            timeout: config.password_bound(),
        }
    }

    pub async fn hash(&self, password: SecretString) -> Result<String, GatewayError> {
        self.run(self.timeout, hashing(password)).await
    }

    /// The hash start-up makes once (the dummy that keeps an unknown user's login as slow as a known one's), within
    /// [`START_BOUND`] instead of `password_seconds`.
    ///
    /// # Errors
    /// `Timeout` past [`START_BOUND`], `Cancelled` at shutdown, an I/O failure.
    pub async fn hash_at_start(&self, password: SecretString) -> Result<String, GatewayError> {
        self.run(START_BOUND, hashing(password)).await
    }

    pub async fn verify(&self, password: SecretString, hash: String) -> Result<bool, GatewayError> {
        // A corrupted database must not turn verification into unbounded memory or CPU work.
        if hash.len() > 256 || !hash.starts_with("$argon2id$v=19$m=19456,t=2,p=1$") {
            return Ok(false);
        }
        self.run(self.timeout, move || {
            Ok(algorithm()?
                .verify_password(password.expose_secret().as_bytes(), hash.as_str())
                .is_ok())
        })
        .await
    }

    /// `operation` on a worker, within `bound` from now, its wait for the worker included; `Busy` at once when the
    /// queue is full.
    async fn run<T: Send + 'static>(
        &self,
        bound: std::time::Duration,
        operation: impl FnOnce() -> Result<T, GatewayError> + Send + 'static,
    ) -> Result<T, GatewayError> {
        let _admitted = self.admit()?;
        let deadline = tokio::time::Instant::now() + bound;
        let permit = crate::io::bounded(bound, &self.cancel, async {
            self.admission
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| GatewayError::Cancelled)
        })
        .await?;
        let task = self.tasks.spawn_blocking(move || {
            let _permit = permit;
            operation()
        });
        // Blocking cryptographic work cannot be aborted safely; the tracker owns it through shutdown. What is left of
        // the bound, so the wait and the work together never pass it.
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        crate::io::bounded(left, &self.cancel, async {
            task.await.map_err(|_| GatewayError::Io)?
        })
        .await
    }

    /// Counts a caller in, or refuses it with `Busy` when every worker and its waiting places are taken.
    fn admit(&self) -> Result<Admitted, GatewayError> {
        let capacity = self.capacity;
        self.callers
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |callers| {
                (callers < capacity).then_some(callers + 1)
            })
            .map_err(|_| GatewayError::Busy)?;
        Ok(Admitted(self.callers.clone()))
    }
}

/// One password's Argon2id hash with a fresh salt.
fn hashing(
    password: SecretString,
) -> impl FnOnce() -> Result<String, GatewayError> + Send + 'static {
    move || {
        let mut salt = [0_u8; 16];
        rand::rngs::OsRng
            .try_fill_bytes(&mut salt)
            .map_err(|_| GatewayError::Io)?;
        algorithm()?
            .hash_password_with_salt(password.expose_secret().as_bytes(), &salt)
            .map(|hash| hash.to_string())
            .map_err(|_| GatewayError::Io)
    }
}

fn algorithm() -> Result<Argon2<'static>, GatewayError> {
    // Argon2id: 19 MiB, two passes, one lane, 32-byte output; all stored hashes include these parameters.
    let parameters = Params::new(19 * 1024, 2, 1, Some(32))
        .map_err(|_| GatewayError::Configuration("invalid password hashing parameters"))?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, parameters))
}

#[cfg(test)]
#[path = "password_tests.rs"]
mod tests;
