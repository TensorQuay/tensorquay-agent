//! Journaled workspace initialization, separate from provider and prompt construction.
mod files;
use crate::RuntimeError;
use files::{Initial, Locations};
use std::{future::Future, path::PathBuf, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use tqa_core::{EventContext, EventSink, LOCAL_PROJECTION_TIMESTAMP};
use tqa_tools::{CloneError, CloneRequest, ProxyEndpoint, RepositoryCloner};
use tqa_types::{ErrorCode, Event, ServerFrame, Sleeper, WorkspaceClone, WorkspaceCloneStatus};

/// Trusted runtime mounts and injected transport; callers supply the validated process directories.
pub struct WorkspaceInitializer {
    /// Existing canonical workspace mount.
    pub workspace: PathBuf,
    /// Existing canonical private state mount, outside the workspace.
    pub state_dir: PathBuf,
    /// Explicit policy proxy; an absent proxy prohibits a new clone.
    pub proxy: Option<ProxyEndpoint>,
    /// Owned, cancellable repository transport.
    pub cloner: Arc<dyn RepositoryCloner>,
    /// Injected two-minute work deadline, also backed by a real-time ceiling.
    pub sleeper: Arc<dyn Sleeper>,
}
/// A durable initialization failure suitable for the pending run's terminal event.
pub struct WorkspaceFailure {
    /// Stable failure classification.
    pub code: ErrorCode,
    /// Fixed status text, never process output or a repository URL.
    pub reason: &'static str,
}
impl WorkspaceFailure {
    fn from_error(error: CloneError) -> Self {
        let (code, reason) = match error {
            CloneError::EgressDenied => (
                ErrorCode::EgressDenied,
                "Clone failed: network access was denied",
            ),
            CloneError::Timeout => (
                ErrorCode::Io,
                "Clone failed: repository initialization timed out",
            ),
            CloneError::Stopped => (
                ErrorCode::Io,
                "Clone failed: repository initialization was interrupted",
            ),
            CloneError::Io => (
                ErrorCode::Io,
                "Clone failed: repository initialization failed",
            ),
        };
        Self { code, reason }
    }
}
enum AttemptError {
    Fatal(RuntimeError),
    Clone(CloneError),
}
impl From<CloneError> for AttemptError {
    fn from(value: CloneError) -> Self {
        Self::Clone(value)
    }
}
impl From<RuntimeError> for AttemptError {
    fn from(value: RuntimeError) -> Self {
        Self::Fatal(value)
    }
}
impl WorkspaceInitializer {
    /// Initializes at most once, appending status before any model work can begin.
    /// Invocation-local projection receives the appended events; only the sink delivers wire frames.
    ///
    /// # Errors
    /// Returns fatal journal errors or shutdown; ordinary clone failures are durable returned values.
    pub async fn initialize(
        &self,
        repository: Option<&WorkspaceClone>,
        history: &mut Vec<ServerFrame>,
        sink: &dyn EventSink,
        cancel: &CancellationToken,
    ) -> Result<Option<WorkspaceFailure>, RuntimeError> {
        let previous = history.iter().rev().find_map(|frame| match frame {
            ServerFrame::Event {
                event: Event::WorkspaceClone { status, code, .. },
                ..
            } => Some((*status, *code)),
            _ => None,
        });
        match previous {
            Some((WorkspaceCloneStatus::Completed | WorkspaceCloneStatus::Skipped, _)) => {
                return Ok(None);
            }
            Some((WorkspaceCloneStatus::Failed, code)) => {
                return Ok(Some(WorkspaceFailure::from_error(
                    if code == Some(ErrorCode::EgressDenied) {
                        CloneError::EgressDenied
                    } else {
                        CloneError::Io
                    },
                )));
            }
            Some((WorkspaceCloneStatus::Started, _)) => {
                return fail(history, sink, cancel, CloneError::Stopped).await;
            }
            Some(_) => return Err(RuntimeError::History),
            None => {}
        }
        let Some(repository) = repository else {
            return Ok(None);
        };
        repository.validate().map_err(|_| RuntimeError::Frame)?;
        let child = cancel.child_token();
        let result = self
            .bounded(
                self.attempt(repository, history, sink, &child),
                &child,
                cancel,
            )
            .await;
        match result {
            Ok(status) => {
                let text = if status == WorkspaceCloneStatus::Skipped {
                    "Existing workspace preserved"
                } else {
                    "Repository cloned"
                };
                append(history, sink, cancel, status_event(status, None, text)).await?;
                Ok(None)
            }
            Err(AttemptError::Fatal(error)) => Err(error),
            Err(AttemptError::Clone(_)) if cancel.is_cancelled() => Err(RuntimeError::Stopped),
            Err(AttemptError::Clone(error)) => fail(history, sink, cancel, error).await,
        }
    }
    async fn attempt(
        &self,
        repository: &WorkspaceClone,
        history: &mut Vec<ServerFrame>,
        sink: &dyn EventSink,
        cancel: &CancellationToken,
    ) -> Result<WorkspaceCloneStatus, AttemptError> {
        let paths = Locations {
            workspace: self.workspace.clone(),
            state: self.state_dir.clone(),
        };
        let inspect_paths = paths.clone();
        match filesystem(move || files::inspect(&inspect_paths)).await? {
            Initial::Existing => return Ok(WorkspaceCloneStatus::Skipped),
            Initial::Attempted => return Err(CloneError::Stopped.into()),
            Initial::Empty => {}
        }
        let proxy = self.proxy.as_ref().ok_or(CloneError::EgressDenied)?;
        append(
            history,
            sink,
            cancel,
            status_event(
                WorkspaceCloneStatus::Started,
                None,
                "Cloning the repository",
            ),
        )
        .await?;
        let begin_paths = paths.clone();
        let token = cancel.clone();
        let destination = filesystem(move || files::begin(&begin_paths, &token)).await?;
        self.cloner
            .clone_repository(
                CloneRequest {
                    repository,
                    destination: &destination,
                    proxy,
                },
                cancel,
            )
            .await?;
        let token = cancel.clone();
        filesystem(move || files::install(&paths, &token)).await?;
        Ok(WorkspaceCloneStatus::Completed)
    }
    async fn bounded<T>(
        &self,
        future: impl Future<Output = Result<T, AttemptError>>,
        child: &CancellationToken,
        cancel: &CancellationToken,
    ) -> Result<T, AttemptError> {
        const DEADLINE: Duration = Duration::from_mins(2);
        tokio::pin!(future);
        tokio::select! {biased;
            result = &mut future => return result,
            () = cancel.cancelled() => {},
            () = self.sleeper.sleep(DEADLINE) => {},
            () = tokio::time::sleep(DEADLINE) => {},
        }
        child.cancel();
        if let Err(AttemptError::Fatal(error)) = future.await
            && !matches!(error, RuntimeError::Stopped)
        {
            return Err(AttemptError::Fatal(error));
        }
        Err(AttemptError::Clone(if cancel.is_cancelled() {
            CloneError::Stopped
        } else {
            CloneError::Timeout
        }))
    }
}
async fn filesystem<T: Send + 'static>(
    operation: impl FnOnce() -> std::io::Result<T> + Send + 'static,
) -> Result<T, CloneError> {
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|_| CloneError::Io)?
        .map_err(|_| CloneError::Io)
}
async fn fail(
    history: &mut Vec<ServerFrame>,
    sink: &dyn EventSink,
    cancel: &CancellationToken,
    error: CloneError,
) -> Result<Option<WorkspaceFailure>, RuntimeError> {
    let failure = WorkspaceFailure::from_error(error);
    append(
        history,
        sink,
        cancel,
        status_event(
            WorkspaceCloneStatus::Failed,
            Some(failure.code),
            failure.reason,
        ),
    )
    .await?;
    Ok(Some(failure))
}
fn status_event(status: WorkspaceCloneStatus, code: Option<ErrorCode>, message: &str) -> Event {
    Event::WorkspaceClone {
        status,
        code,
        message: message.into(),
    }
}
async fn append(
    history: &mut Vec<ServerFrame>,
    sink: &dyn EventSink,
    cancel: &CancellationToken,
    event: Event,
) -> Result<(), RuntimeError> {
    if cancel.is_cancelled() {
        return Err(RuntimeError::Stopped);
    }
    let seq = sink
        .emit(event.clone(), EventContext::default())
        .await
        .map_err(|_| RuntimeError::io("workspace event append"))?;
    history.push(ServerFrame::Event {
        seq,
        run_id: None,
        step: None,
        ts: LOCAL_PROJECTION_TIMESTAMP.into(),
        event,
    });
    Ok(())
}
