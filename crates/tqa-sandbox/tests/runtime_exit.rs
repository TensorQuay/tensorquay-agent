//! SBX-C-001: startup failures remain observable while registration is pending.

use async_trait::async_trait;
use std::{
    error::Error,
    time::{Duration, SystemTime},
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tqa_sandbox::{FakeSandboxBackend, ReadinessSource, SandboxBackend, SandboxError};
use tqa_types::ConversationId;

#[path = "support/spec.rs"]
mod support;

struct Stalled(Notify);
#[async_trait]
impl ReadinessSource for Stalled {
    async fn is_ready(&self, _: &ConversationId) -> bool {
        self.0.notify_one();
        std::future::pending().await
    }
}

struct Ready;
#[async_trait]
impl ReadinessSource for Ready {
    async fn is_ready(&self, _: &ConversationId) -> bool {
        true
    }
}

#[tokio::test(start_paused = true)]
async fn sbx_c_001_runtime_exit_interrupts_a_stalled_registration_lookup()
-> Result<(), Box<dyn Error>> {
    let backend = FakeSandboxBackend::new(SystemTime::UNIX_EPOCH);
    let handle = backend
        .create(support::spec()?, CancellationToken::new())
        .await?;
    let source = Stalled(Notify::new());
    let started = tokio::time::Instant::now();
    let (ready, exited) = tokio::join!(
        backend.wait_ready(
            &handle,
            &source,
            Duration::from_secs(30),
            CancellationToken::new()
        ),
        async {
            source.0.notified().await;
            backend.exit(&handle, 2)
        }
    );
    exited?;
    assert_eq!(ready, Err(SandboxError::RuntimeExited { code: 2 }));
    assert!(started.elapsed() <= Duration::from_millis(100));
    assert_eq!(backend.list().await?, vec![handle.clone()]);
    backend.destroy(&handle).await?;
    assert_eq!(
        backend
            .wait_ready(
                &handle,
                &Ready,
                Duration::from_secs(1),
                CancellationToken::new()
            )
            .await,
        Err(SandboxError::NotFound)
    );
    assert_eq!(backend.exit(&handle, 2), Err(SandboxError::NotFound));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn sbx_c_001_exited_runtime_preserves_the_bounded_code_and_cancellation_priority()
-> Result<(), Box<dyn Error>> {
    let backend = FakeSandboxBackend::new(SystemTime::UNIX_EPOCH);
    for code in [0, 2, 137, i32::MIN, i32::MAX] {
        let handle = backend
            .create(support::spec()?, CancellationToken::new())
            .await?;
        let mut foreign = handle.clone();
        foreign.conversation_id = "foreign".parse()?;
        assert_eq!(backend.exit(&foreign, code), Err(SandboxError::NotFound));
        backend.exit(&handle, code)?;
        let error = SandboxError::RuntimeExited { code };
        assert_eq!(error.code(), "sandbox.runtime_exited");
        assert!(!error.retryable());
        assert_eq!(
            error.to_string(),
            format!("runtime exited before registration: code {code}")
        );
        assert_eq!(
            backend
                .wait_ready(
                    &handle,
                    &Ready,
                    Duration::from_secs(1),
                    CancellationToken::new()
                )
                .await,
            Err(error)
        );
        assert_eq!(backend.address(&handle).await, Err(SandboxError::NotFound));
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert_eq!(
            backend
                .wait_ready(&handle, &Ready, Duration::from_secs(1), cancel)
                .await,
            Err(SandboxError::Cancelled)
        );
        backend.destroy(&handle).await?;
    }
    Ok(())
}
