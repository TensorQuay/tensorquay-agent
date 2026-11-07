use crate::SandboxError;
use std::{future::Future, time::Duration};
use tokio_util::sync::CancellationToken;

pub(super) fn missing(error: &bollard::errors::Error) -> bool {
    matches!(
        error,
        bollard::errors::Error::DockerResponseServerError {
            status_code: 404,
            ..
        }
    )
}

/// Whether the daemon refused a request because the name is taken (409).
pub(super) fn conflict(error: &bollard::errors::Error) -> bool {
    matches!(
        error,
        bollard::errors::Error::DockerResponseServerError {
            status_code: 409,
            ..
        }
    )
}

pub(super) fn map_error(error: &bollard::errors::Error) -> SandboxError {
    if missing(error) {
        SandboxError::NotFound
    } else {
        SandboxError::Backend("Docker API request failed".into())
    }
}

pub(super) async fn request<T>(
    bound: Duration,
    future: impl Future<Output = Result<T, bollard::errors::Error>>,
) -> Result<T, SandboxError> {
    tokio::time::timeout(bound, future)
        .await
        .map_err(|_| SandboxError::Timeout)?
        .map_err(|error| map_error(&error))
}

pub(super) async fn cancellable<T>(
    cancel: &CancellationToken,
    future: impl Future<Output = Result<T, SandboxError>>,
) -> Result<T, SandboxError> {
    tokio::select! {
        biased;
        () = cancel.cancelled() => Err(SandboxError::Cancelled),
        result = future => result,
    }
}

pub(super) fn check_cancel(cancel: &CancellationToken) -> Result<(), SandboxError> {
    if cancel.is_cancelled() {
        Err(SandboxError::Cancelled)
    } else {
        Ok(())
    }
}
