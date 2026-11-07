//! LLM-C-002/019: the standalone server's pre-test priming fails closed.
use std::{error::Error, time::Duration};

#[tokio::test]
async fn llm_c_002_standalone_priming_success_and_failure_contract() -> Result<(), Box<dyn Error>> {
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/test_prime_model.py");
    let output = tokio::time::timeout(
        Duration::from_mins(1),
        tokio::process::Command::new("python3")
            .arg("-B")
            .arg(script)
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
