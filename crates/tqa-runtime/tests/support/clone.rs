//! Deterministic checkout transport; no network or child process is needed by bootstrap tests.
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use tokio::sync::{Notify, oneshot};
use tokio_util::sync::CancellationToken;
use tqa_tools::{CloneError, CloneRequest, RepositoryCloner};

pub struct FakeClone {
    pub seen: Mutex<Vec<Option<String>>>,
    pub entered: Notify,
    pub release: Mutex<Option<oneshot::Receiver<()>>>,
    pub failure: Option<CloneError>,
}
impl FakeClone {
    pub fn new(failure: Option<CloneError>) -> Arc<Self> {
        Arc::new(Self {
            seen: Mutex::new(vec![]),
            entered: Notify::new(),
            release: Mutex::new(None),
            failure,
        })
    }
}
#[async_trait]
impl RepositoryCloner for FakeClone {
    async fn clone_repository(
        &self,
        input: CloneRequest<'_>,
        cancel: &CancellationToken,
    ) -> Result<(), CloneError> {
        self.seen
            .lock()
            .map_err(|_| CloneError::Io)?
            .push(input.repository.r#ref.clone());
        self.entered.notify_one();
        let release = self.release.lock().map_err(|_| CloneError::Io)?.take();
        if let Some(release) = release {
            tokio::select! {
                () = cancel.cancelled() => return Err(CloneError::Stopped),
                ready = release => { ready.map_err(|_| CloneError::Io)?; }
            }
        }
        std::fs::create_dir(input.destination).map_err(|_| CloneError::Io)?;
        std::fs::write(input.destination.join("README.md"), "checkout fixture\n")
            .map_err(|_| CloneError::Io)?;
        if let Some(failure) = self.failure {
            return Err(failure);
        }
        std::fs::write(
            input.destination.join("AGENTS.md"),
            "Repository fixture instruction\n",
        )
        .map_err(|_| CloneError::Io)?;
        Ok(())
    }
}
