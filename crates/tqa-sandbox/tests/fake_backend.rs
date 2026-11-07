//! SBX-C-001: the in-memory backend supports caller tests without Docker.

use async_trait::async_trait;
use std::{
    error::Error,
    time::{Duration, SystemTime},
};
use tokio_util::sync::CancellationToken;
use tqa_sandbox::{
    BackendKind, FakeSandboxBackend, ReadinessSource, SandboxBackend, SandboxError, reconcile,
};
use tqa_types::ConversationId;

#[path = "support/spec.rs"]
mod support;
struct Ready;
#[async_trait]
impl ReadinessSource for Ready {
    async fn is_ready(&self, _: &ConversationId) -> bool {
        true
    }
}

#[tokio::test]
async fn sbx_c_001_fake_backend_obeys_lifecycle_and_cancellation() -> Result<(), Box<dyn Error>> {
    let backend = FakeSandboxBackend::new(SystemTime::UNIX_EPOCH);
    assert_eq!(backend.kind(), BackendKind::Docker);
    let handle = backend
        .create(support::spec()?, CancellationToken::new())
        .await?;
    assert_eq!(handle.created_at, SystemTime::UNIX_EPOCH);
    assert_eq!(
        backend.address(&handle).await?,
        std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
    );
    backend
        .wait_ready(
            &handle,
            &Ready,
            Duration::from_secs(1),
            CancellationToken::new(),
        )
        .await?;
    assert_eq!(
        reconcile(&[], &backend.list().await?).orphans,
        vec![handle.clone()]
    );
    assert_eq!(
        backend.stats(&handle).await?,
        tqa_sandbox::SandboxStats::default()
    );
    backend.destroy(&handle).await?;
    backend.destroy(&handle).await?;
    assert_eq!(backend.stats(&handle).await, Err(SandboxError::NotFound));
    assert_eq!(backend.address(&handle).await, Err(SandboxError::NotFound));
    assert!(backend.list().await?.is_empty());
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        backend.create(support::spec()?, cancel).await,
        Err(SandboxError::Cancelled)
    );
    Ok(())
}
