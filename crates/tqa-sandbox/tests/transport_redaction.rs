//! SBX-C-005: debug diagnostics may contain credential paths but never credential bytes.

use std::sync::atomic::{AtomicBool, Ordering};
use tokio_util::sync::CancellationToken;
use tqa_sandbox::SandboxBackend;

#[path = "support/docker.rs"]
mod support;

struct Capture {
    leaked: AtomicBool,
    saw_path: AtomicBool,
}
static CAPTURE: Capture = Capture {
    leaked: AtomicBool::new(false),
    saw_path: AtomicBool::new(false),
};

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }
    fn log(&self, record: &log::Record<'_>) {
        let text = record.args().to_string();
        if text.contains("sandbox-fixture-value") {
            self.leaked.store(true, Ordering::Relaxed);
        }
        if record.target().starts_with("bollard") && text.contains("TQA_RUNTIME_TOKEN_FILE") {
            self.saw_path.store(true, Ordering::Relaxed);
        }
    }
    fn flush(&self) {}
}

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sbx_c_005_bollard_debug_and_trace_never_receive_token_contents() -> support::TestResult {
    log::set_logger(&CAPTURE).map_err(|_| "test logger already installed")?;
    log::set_max_level(log::LevelFilter::Trace);
    support::run(async |fixture| {
        let handle = fixture
            .backend
            .create(fixture.spec("logging", false)?, CancellationToken::new())
            .await?;
        support::bounded(fixture.docker.inspect_container(&handle.id, None)).await?;
        fixture.backend.destroy(&handle).await?;
        assert!(!CAPTURE.leaked.load(Ordering::Relaxed));
        if cfg!(debug_assertions) {
            assert!(
                CAPTURE.saw_path.load(Ordering::Relaxed),
                "Bollard debug logging must remain enabled in development"
            );
        }
        Ok(())
    })
    .await
}

#[test]
fn sbx_c_005_release_log_facade_caps_payload_diagnostics() {
    if cfg!(debug_assertions) {
        assert_eq!(log::STATIC_MAX_LEVEL, log::LevelFilter::Trace);
    } else {
        assert!(log::STATIC_MAX_LEVEL <= log::LevelFilter::Info);
    }
}
