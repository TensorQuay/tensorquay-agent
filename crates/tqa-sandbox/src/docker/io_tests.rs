use super::io;
use crate::SandboxError;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[tokio::test(start_paused = true)]
async fn sbx_c_010_transport_errors_and_timeouts_are_bounded_and_redacted() {
    let missing = bollard::errors::Error::DockerResponseServerError {
        status_code: 404,
        message: "synthetic-sensitive-data".into(),
    };
    assert_eq!(
        io::request(Duration::from_secs(1), async { Err::<(), _>(missing) }).await,
        Err(SandboxError::NotFound)
    );
    let failure = bollard::errors::Error::DockerResponseServerError {
        status_code: 500,
        message: "synthetic-sensitive-data".into(),
    };
    let error = io::map_error(&failure);
    assert!(error.retryable());
    assert!(!format!("{error:?} {error}").contains("synthetic-sensitive-data"));
    assert_eq!(
        io::request(
            Duration::from_secs(1),
            std::future::pending::<Result<(), bollard::errors::Error>>()
        )
        .await,
        Err(SandboxError::Timeout)
    );
    assert_eq!(
        io::request(Duration::from_secs(1), async {
            Ok::<_, bollard::errors::Error>(123)
        })
        .await,
        Ok(123)
    );
}

#[tokio::test(start_paused = true)]
async fn sbx_c_001_cancellation_precedes_success_and_interrupts_a_pending_read() {
    let cancel = CancellationToken::new();
    assert_eq!(io::check_cancel(&cancel), Ok(()));
    assert_eq!(io::cancellable(&cancel, async { Ok(123) }).await, Ok(123));
    cancel.cancel();
    assert_eq!(io::check_cancel(&cancel), Err(SandboxError::Cancelled));
    assert_eq!(
        io::cancellable(&cancel, std::future::pending::<Result<(), SandboxError>>()).await,
        Err(SandboxError::Cancelled)
    );
}
