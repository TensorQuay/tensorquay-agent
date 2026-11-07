//! What a run leaves on the server (TK-U-026, TK-U-027): each attempt's task is deleted as soon as its evidence is
//! saved, so it never reaches anyone's task list, and each attempt's workspace at the end of the run. A workspace can't
//! be deleted while its task's sandbox still idles on it, so the sweep retries until a bound and names what is left.
//! Only the run's own workspaces, by id, are ever deleted.
use crate::client::{ApiClient, ClientError};
use std::time::Duration;

/// How often the sweep retries a busy workspace.
pub const RETRY_EVERY: Duration = Duration::from_secs(15);

/// Deletes `workspaces` with their tasks, retrying a busy one (`409`) every `every` until `wait` has passed; one
/// already gone (`404`) counts as deleted. Returns those left on the server, in the order given.
pub async fn sweep(
    client: &ApiClient,
    workspaces: &[String],
    wait: Duration,
    every: Duration,
) -> Vec<String> {
    let started = tokio::time::Instant::now();
    let mut left: Vec<String> = workspaces.to_vec();
    loop {
        let mut busy = Vec::new();
        for workspace in left {
            match client.delete_workspace(&workspace).await {
                Ok(()) | Err(ClientError::Status(404)) => {}
                Err(_) => busy.push(workspace),
            }
        }
        left = busy;
        if left.is_empty() || started.elapsed() >= wait {
            return left;
        }
        tokio::time::sleep(every.min(wait.saturating_sub(started.elapsed()))).await;
    }
}

#[cfg(test)]
mod tests;
