//! SBX-U-020: `destroy` removes a container of this instance even when its credential label (`tqa.token_id`) is
//! missing. Without it a stray the sweep found could never be removed: the destroy failed before the removal, every
//! sweep, and the leak verdict stayed amber until someone removed it by hand (the real-stack run of 18 Nov 2026).
use super::{TestResult, scripted, scripted_backend, step};
use crate::{BackendKind, SandboxHandle};
use std::time::SystemTime;

const CONVERSATION: &str = "01a1ecc0-1d93-76f2-a9b9-b27b39aafd95";

fn handle() -> Result<SandboxHandle, Box<dyn std::error::Error>> {
    Ok(SandboxHandle {
        kind: BackendKind::Docker,
        id: "c9".into(),
        conversation_id: CONVERSATION.parse()?,
        created_at: SystemTime::UNIX_EPOCH,
    })
}

// TEST: SBX-U-020
#[tokio::test]
async fn sbx_u_020_a_container_without_its_credential_label_is_still_removed() -> TestResult {
    let (backend, listener, _storage) = scripted_backend(None).await?;
    let handle = handle()?;
    let inspected = format!(
        r#"{{"Id":"c9","Name":"/stray","Config":{{"Labels":{{"tqa.instance":"tqa-unit","tqa.conversation":"{CONVERSATION}"}}}}}}"#
    );
    let (destroyed, seen) = scripted(
        listener,
        vec![
            step("GET /containers/c9/json", "200 OK", &inspected),
            step("POST /containers/c9/stop", "204 No Content", ""),
            step("DELETE /containers/c9", "204 No Content", ""),
        ],
        Box::pin(backend.destroy_sandbox(&handle)),
    )
    .await?;
    assert!(destroyed.is_ok(), "{destroyed:?} {seen:?}");
    assert!(
        seen.iter()
            .any(|request| request.starts_with("DELETE /containers/c9")),
        "removed: {seen:?}"
    );
    Ok(())
}
