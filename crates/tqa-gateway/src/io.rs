use crate::GatewayError;
use std::{future::Future, path::Path, time::Duration};
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

pub(crate) async fn bounded<T>(
    duration: Duration,
    cancel: &CancellationToken,
    operation: impl Future<Output = Result<T, GatewayError>>,
) -> Result<T, GatewayError> {
    tokio::select! {
        biased;
        () = cancel.cancelled() => Err(GatewayError::Cancelled),
        result = tokio::time::timeout(duration, operation) => result.map_err(|_| GatewayError::Timeout)?,
    }
}

pub(crate) async fn read_bounded(path: &Path, cap: usize) -> Result<Vec<u8>, GatewayError> {
    let file = tokio::fs::File::open(path).await?;
    let limit = u64::try_from(cap)
        .map_err(|_| GatewayError::Io)?
        .saturating_add(1);
    let mut bytes = Vec::new();
    file.take(limit).read_to_end(&mut bytes).await?;
    if bytes.len() > cap {
        return Err(GatewayError::Io);
    }
    Ok(bytes)
}
