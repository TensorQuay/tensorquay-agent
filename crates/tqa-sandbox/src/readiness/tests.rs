use std::{
    error::Error,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

use async_trait::async_trait;
use futures::FutureExt;
use tokio_util::sync::CancellationToken;
use tqa_types::ConversationId;

use super::{ReadinessSource, poll};
use crate::SandboxError;

struct Registration {
    calls: AtomicUsize,
    after: usize,
}

#[async_trait]
impl ReadinessSource for Registration {
    async fn is_ready(&self, _: &ConversationId) -> bool {
        self.calls.fetch_add(1, Ordering::Relaxed) >= self.after
    }
}

#[tokio::test(start_paused = true)]
async fn sbx_c_001_readiness_polls_until_registered() -> Result<(), Box<dyn Error>> {
    let source = Registration {
        calls: AtomicUsize::new(0),
        after: 2,
    };
    poll(
        &"conversation_ready".parse()?,
        &source,
        (Duration::from_secs(1), Duration::from_millis(100)),
        &CancellationToken::new(),
        &mut || async { Ok(()) }.boxed(),
    )
    .await?;
    assert_eq!(source.calls.load(Ordering::Relaxed), 3);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn sbx_c_001_readiness_timeout_exit_and_cancellation_are_typed() -> Result<(), Box<dyn Error>>
{
    let source = Registration {
        calls: AtomicUsize::new(0),
        after: usize::MAX,
    };
    let id = "conversation_pending".parse()?;
    let cancel = CancellationToken::new();
    assert_eq!(
        poll(
            &id,
            &source,
            (Duration::from_secs(1), Duration::from_millis(100)),
            &cancel,
            &mut || async { Ok(()) }.boxed()
        )
        .await,
        Err(SandboxError::Timeout)
    );
    assert_eq!(
        poll(
            &id,
            &source,
            (Duration::from_secs(1), Duration::from_millis(100)),
            &cancel,
            &mut || async { Err(SandboxError::RuntimeExited { code: 2 }) }.boxed()
        )
        .await,
        Err(SandboxError::RuntimeExited { code: 2 })
    );
    assert_eq!(
        poll(
            &id,
            &source,
            (Duration::from_secs(1), Duration::from_millis(100)),
            &cancel,
            &mut || async { Err(SandboxError::NotFound) }.boxed()
        )
        .await,
        Err(SandboxError::NotFound)
    );
    cancel.cancel();
    assert_eq!(
        poll(
            &id,
            &source,
            (Duration::from_secs(1), Duration::from_millis(100)),
            &cancel,
            &mut || async { Ok(()) }.boxed()
        )
        .await,
        Err(SandboxError::Cancelled)
    );
    Ok(())
}
