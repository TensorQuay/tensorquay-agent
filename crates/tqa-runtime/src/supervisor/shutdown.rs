//! Shutdown stops core writes before syncing and closing the owned link and storage workers.
use super::Supervisor;
use crate::{RuntimeError, RuntimeExit, link::Control};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use tqa_types::{ErrorCode, ServerFrame};

impl Supervisor {
    pub(super) async fn finish(
        &mut self,
        result: &Result<RuntimeExit, RuntimeError>,
    ) -> Result<(), RuntimeError> {
        // A stop with no run going is the sandbox stopping idle: nothing will be sent its screenshots again, so they go
        // now rather than wait in its state for the next run. An interrupted run resumes, and keeps its own.
        let idle = self.runs.is_empty();
        self.run_shutdown.cancel();
        #[cfg(feature = "testing")]
        {
            self.testing.before_append.resume();
            self.testing.supervisor.resume();
        }
        let time = self.services.time.clone();
        let timeout = self.services.limits.shutdown_timeout;
        let cleanup = async {
            self.finish_workers(result).await?;
            if idle {
                tqa_core::Screenshots::new(&self.services.config.state_dir.join("screenshots"))
                    .forget_all_but(None)
                    .await;
            }
            Ok(())
        };
        time.wait(timeout, &CancellationToken::new(), cleanup)
            .await?
    }
    async fn finish_workers(
        &mut self,
        result: &Result<RuntimeExit, RuntimeError>,
    ) -> Result<(), RuntimeError> {
        let mut failure = None;
        while !self.runs.is_empty() {
            tokio::select! {biased;
                frame=self.frames.recv()=>{if frame.is_none(){failure=Some(RuntimeError::Worker);break;}},
                joined=self.runs.join_next()=>match joined {
                    Some(Ok(Ok(_)|Err(RuntimeError::Stopped)))=>{},
                    Some(Ok(Err(error)))=>failure=Some(error),
                    _=>failure=Some(RuntimeError::Worker),
                },
            }
        }
        if matches!(result, Err(RuntimeError::Io(_)))
            || matches!(failure, Some(RuntimeError::Io(_)))
        {
            self.report_io().await;
        }
        self.link_shutdown.cancel();
        #[cfg(feature = "testing")]
        self.testing
            .link_stopping
            .store(true, std::sync::atomic::Ordering::SeqCst);
        while let Some(joined) = self.link.join_next().await {
            if !matches!(joined, Ok(Ok(()) | Err(RuntimeError::Stopped))) {
                tracing::debug!("link stopped during runtime cleanup");
            }
        }
        self.services.journal.shutdown().await?;
        if let Some(error) = failure {
            return Err(error);
        }
        Ok(())
    }
    async fn report_io(&self) {
        let (sent, receive) = oneshot::channel();
        let frame = ServerFrame::Error {
            target: None,
            seq: None,
            code: ErrorCode::Io,
            message: "runtime storage I/O failed; the run can be resumed".into(),
            retryable: false,
        };
        if self
            .controls
            .try_send(Control {
                frame,
                sent: Some(sent),
            })
            .is_err()
        {
            return;
        }
        if !matches!(
            self.services
                .time
                .wait(
                    self.services.limits.io_timeout,
                    &CancellationToken::new(),
                    receive
                )
                .await,
            Ok(Ok(Ok(())))
        ) {
            tracing::debug!("storage error could not be delivered on the runtime link");
        }
    }
}
