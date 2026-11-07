//! Process services are supplied explicitly; only the binary chooses production defaults.
use crate::{
    ProcessCleanup, ProcessReaper, RuntimeConfig, RuntimeError, RuntimeLimits, SystemWallClock,
    WallClock,
};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tqa_llm::{Jitter, SystemClock, ThreadRngJitter, TokioSleeper};
use tqa_types::{Clock, Sleeper};

/// A successful runtime shutdown, distinct from a fatal process error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeExit {
    /// A shutdown signal or a normal link stop ended the process.
    Shutdown,
}

/// Dependencies for one conversation process, including all clocks and both random sources.
pub struct RuntimeDeps {
    /// Validated origin, credential and mounts.
    pub config: RuntimeConfig,
    /// Explicit bounds for queues, history and I/O.
    pub limits: RuntimeLimits,
    /// Monotonic active time and connection deadlines.
    pub clock: Arc<dyn Clock>,
    /// Sleeper driven by the same monotonic clock.
    pub sleeper: Arc<dyn Sleeper>,
    /// UTC event timestamps.
    pub wall_clock: Arc<dyn WallClock>,
    /// Reconnect randomness, owned only by the link driver.
    pub reconnect_jitter: Box<dyn Jitter>,
    /// Model retry randomness, shared between sequential core invocations.
    pub model_jitter: Box<dyn Jitter>,
    /// Escaped-child cleanup after each shell call and run.
    pub cleanup: Arc<dyn ProcessCleanup>,
    /// Cancellable repository transport used only by workspace initialization.
    pub repository_cloner: Arc<dyn tqa_tools::RepositoryCloner>,
    /// Fault gates absent from default production builds.
    #[cfg(feature = "testing")]
    pub testing: crate::testing::TestingHooks,
}
impl RuntimeDeps {
    /// Uses the approved limits, system clocks and Linux process hygiene for the runtime binary.
    ///
    /// # Errors
    /// Reports an inability to enable Linux child-subreaper ownership.
    pub fn production(config: RuntimeConfig) -> Result<Self, RuntimeError> {
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let sleeper: Arc<dyn Sleeper> = Arc::new(TokioSleeper);
        let limits = RuntimeLimits::RECOMMENDED;
        let cleanup = Arc::new(ProcessReaper::new(clock.clone(), sleeper.clone(), limits)?);
        let repository_cloner = Arc::new(tqa_tools::GitCloner::new(sleeper.clone()));
        Ok(Self {
            config,
            limits,
            clock,
            sleeper,
            wall_clock: Arc::new(SystemWallClock),
            reconnect_jitter: Box::new(ThreadRngJitter),
            model_jitter: Box::new(ThreadRngJitter),
            cleanup,
            repository_cloner,
            #[cfg(feature = "testing")]
            testing: crate::testing::TestingHooks::default(),
        })
    }
}
/// One outbound-only runtime process. It owns every link, core and storage worker it starts.
pub struct Runtime {
    deps: RuntimeDeps,
}
impl Runtime {
    /// Validates process limits before creating any worker.
    ///
    /// # Errors
    /// Rejects invalid explicit limits.
    pub fn new(deps: RuntimeDeps) -> Result<Self, RuntimeError> {
        deps.limits.validate()?;
        Ok(Self { deps })
    }
    /// Connects, reconstructs history and supervises runs until shutdown, parking or token revocation.
    ///
    /// # Errors
    /// Reports fatal history, storage, configuration-change or startup-deadline failures.
    pub async fn run(self, shutdown: CancellationToken) -> Result<(), RuntimeError> {
        self.run_until_exit(shutdown).await.map(|_| ())
    }
    /// Supervises runs and reports whether a clean exit parked a pending approval.
    ///
    /// # Errors
    /// Reports the same fatal boundary failures as `run`; parking succeeds only after WAL sync.
    pub async fn run_until_exit(
        self,
        shutdown: CancellationToken,
    ) -> Result<RuntimeExit, RuntimeError> {
        crate::supervisor::run(self.deps, shutdown).await
    }
}
