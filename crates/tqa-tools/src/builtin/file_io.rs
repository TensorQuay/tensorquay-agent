//! Owned blocking filesystem jobs; cancellation is checked between bounded operations.
use crate::{ToolContext, ToolError};
use std::{
    fs::{File, Metadata, OpenOptions},
    io::Read,
    path::Path,
};
use tokio_util::sync::CancellationToken;

pub(super) fn open_regular(path: &Path) -> Result<(File, Metadata), ToolError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // A FIFO must not block open before its descriptor can be checked.
        options.custom_flags(nix::libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(ToolError::PathInvalid("expected regular file".into()));
    }
    Ok((file, metadata))
}

pub(super) fn cancelled(cancel: &CancellationToken) -> Result<(), ToolError> {
    if cancel.is_cancelled() {
        Err(ToolError::Cancelled)
    } else {
        Ok(())
    }
}

pub(super) async fn run<T: Send + 'static>(
    cx: &ToolContext,
    operation: impl FnOnce(&CancellationToken) -> Result<T, ToolError> + Send + 'static,
) -> Result<T, ToolError> {
    cx.check()?;
    let cancel = cx.cancel.child_token();
    let worker_cancel = cancel.clone();
    let _cancel_on_drop = cancel.clone().drop_guard();
    let mut job =
        tokio_util::task::AbortOnDropHandle::new(tokio::task::spawn_blocking(move || {
            operation(&worker_cancel)
        }));
    let result = cx
        .io(async {
            (&mut job)
                .await
                .map_err(|_| ToolError::Io("file worker failed".into()))
        })
        .await;
    match result {
        Ok(completed) => completed,
        Err(error @ (ToolError::Cancelled | ToolError::Timeout)) => {
            cancel.cancel();
            // Running filesystem syscalls cannot be aborted; retain and join their owner.
            match job
                .await
                .map_err(|_| ToolError::Io("file worker cleanup failed".into()))?
            {
                Ok(_) | Err(ToolError::Cancelled) => Err(error),
                Err(cleanup_error) => Err(cleanup_error),
            }
        }
        Err(error) => Err(error),
    }
}

pub(super) fn read_bounded(
    file: &mut File,
    max: usize,
    cancel: &CancellationToken,
) -> Result<Vec<u8>, ToolError> {
    const READ_CHUNK_BYTES: usize = 8192;
    let mut bytes = Vec::new();
    let mut buffer = [0; READ_CHUNK_BYTES];
    while bytes.len() < max {
        cancelled(cancel)?;
        let length = buffer.len().min(max - bytes.len());
        let count = file.read(&mut buffer[..length])?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ToolLimits, WorkspaceRoot};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use tqa_testkit::{FakeClock, FakeSleeper};

    #[cfg(unix)]
    #[test]
    fn tool_u_001_regular_file_open_rejects_fifo_without_waiting_for_a_writer()
    -> Result<(), ToolError> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("fifo");
        let status = std::process::Command::new("mkfifo").arg(&path).status()?;
        assert!(status.success());
        assert!(matches!(
            open_regular(&path),
            Err(ToolError::PathInvalid(_))
        ));
        Ok(())
    }

    #[tokio::test]
    async fn tool_u_003_file_worker_is_joined_on_timeout_and_cancel() -> Result<(), ToolError> {
        joined(false).await?;
        joined(true).await
    }
    async fn joined(timeout: bool) -> Result<(), ToolError> {
        let directory = tempfile::tempdir()?;
        let clock = FakeClock::new();
        let cx = ToolContext {
            workspace: WorkspaceRoot::open(directory.path())?,
            cancel: CancellationToken::new(),
            clock: Arc::new(clock.clone()),
            sleeper: Arc::new(FakeSleeper::new(clock.clone())),
            env_allow: vec![],
            limits: ToolLimits::RECOMMENDED,
        };
        let started = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let (worker_start, worker_finish) = (started.clone(), finished.clone());
        let call = run(&cx, move |cancel| {
            worker_start.store(true, Ordering::SeqCst);
            while !cancel.is_cancelled() {
                std::thread::yield_now();
            }
            worker_finish.store(true, Ordering::SeqCst);
            Err::<(), _>(ToolError::Cancelled)
        });
        let control = async {
            wait_started(&started).await;
            if timeout {
                clock.advance(cx.limits.file_timeout)?;
            } else {
                cx.cancel.cancel();
            }
            Ok::<_, ToolError>(())
        };
        let (result, control) = tokio::join!(call, control);
        control?;
        assert!(finished.load(Ordering::SeqCst));
        assert_eq!(
            result,
            Err(if timeout {
                ToolError::Timeout
            } else {
                ToolError::Cancelled
            })
        );
        Ok(())
    }
    async fn wait_started(started: &AtomicBool) {
        while !started.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    }
}
