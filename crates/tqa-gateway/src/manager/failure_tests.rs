//! GW-U-028: a refused sandbox specification is logged with its field and bounded reason, and a failed preflight with
//! its fixed reason at ERROR, so an operator can see which setting to change; the API error stays the flat cause, and
//! backend details are never logged.
use super::failure;
use crate::GatewayError;
use std::{
    io,
    sync::{Arc, Mutex},
};
use tqa_sandbox::SandboxError;

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);
impl io::Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut buffer = self
            .0
            .lock()
            .map_err(|_| io::Error::other("capture lock"))?;
        buffer.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn logged(error: SandboxError) -> Result<(GatewayError, String), Box<dyn std::error::Error>> {
    let capture = Capture::default();
    let writer = capture.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    let mapped = tracing::subscriber::with_default(subscriber, || failure("create")(error));
    let text = String::from_utf8(capture.0.lock().map_err(|_| "capture lock")?.clone())?;
    Ok((mapped, text))
}

#[test]
fn gw_u_028_a_refused_specification_is_logged_by_field_and_reason()
-> Result<(), Box<dyn std::error::Error>> {
    let reason = "the image declares environment keys the specification does not set: HOME, PATH";
    let (mapped, text) = logged(SandboxError::InvalidSpec {
        field: "image",
        reason: reason.into(),
    })?;
    assert!(matches!(
        mapped,
        GatewayError::Sandbox {
            stage: "create",
            cause: "invalid sandbox specification"
        }
    ));
    for expected in ["create", "image", reason] {
        assert!(text.contains(expected), "{text}");
    }
    let (_, text) = logged(SandboxError::Backend("private engine detail".into()))?;
    assert!(!text.contains("private engine detail"), "{text}");
    Ok(())
}

#[test]
fn gw_u_028_a_failed_preflight_is_logged_at_error_with_its_reason()
-> Result<(), Box<dyn std::error::Error>> {
    let reason = "per-sandbox networks need [sandbox] gateway_container";
    let (mapped, text) = logged(SandboxError::Preflight(reason))?;
    assert!(matches!(
        mapped,
        GatewayError::Sandbox {
            stage: "create",
            cause: "backend preflight failed"
        }
    ));
    for expected in ["ERROR", "create", reason] {
        assert!(text.contains(expected), "{text}");
    }
    Ok(())
}

// TEST: GW-U-086
#[test]
fn gw_u_086_an_environment_that_failed_before_the_runtime_is_a_sandbox_failure()
-> Result<(), Box<dyn std::error::Error>> {
    // Exit 126: the microVM's guest init failed (a workspace disk that won't mount), or Docker couldn't run the
    // image's entrypoint. The agent never started, so it's the environment's failure, and the log keeps the code.
    let (mapped, text) = logged(SandboxError::RuntimeExited {
        code: tqa_sandbox::ENVIRONMENT_FAILED_EXIT,
    })?;
    assert!(
        matches!(
            mapped,
            GatewayError::Sandbox {
                stage: "create",
                cause: "environment failed before the runtime started"
            }
        ),
        "{mapped:?}"
    );
    assert_eq!(mapped.code(), "sandbox_failed");
    assert!(text.contains("126"), "{text}");
    // The runtime's own exits (0 to 4) are still the agent stopping while it started.
    for code in [0, 1, 2, 3, 4, -1] {
        let (mapped, _) = logged(SandboxError::RuntimeExited { code })?;
        assert!(
            matches!(mapped, GatewayError::RuntimeExited { code: exited } if exited == code),
            "{mapped:?}"
        );
    }
    Ok(())
}

// TEST: GW-I-208
#[test]
fn gw_i_208_only_a_failed_create_is_retried() {
    use super::recovery::create_failed;
    // A create the backend couldn't make: a full disk, an engine past its bound.
    assert!(create_failed(&failure("create")(SandboxError::Backend(
        "disk full".into()
    ))));
    assert!(create_failed(&failure("create")(SandboxError::Timeout)));
    // A runtime that dies at start (candidate 14a), a start that failed, a deleted task, a store failure: not a create's.
    assert!(!create_failed(&failure("start")(
        SandboxError::RuntimeExited { code: 2 }
    )));
    assert!(!create_failed(&failure("create")(
        SandboxError::RuntimeExited { code: 2 }
    )));
    assert!(!create_failed(&failure("start")(SandboxError::Timeout)));
    assert!(!create_failed(&GatewayError::Conflict(
        "conversation_deleted"
    )));
    assert!(!create_failed(&GatewayError::StoreUnavailable));
}
