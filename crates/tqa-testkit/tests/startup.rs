//! LLM-C-002/019: the standalone server's pre-test priming fails closed.
use std::{error::Error, time::Duration};

#[tokio::test]
async fn llm_c_002_standalone_priming_success_and_failure_contract() -> Result<(), Box<dyn Error>> {
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/test_prime_model.py");
    // A liveness bound on the whole Python suite (nine cases, some starting a server), so it scales with the load
    // setting as the harness's other waits do: on CI's 2-core runner, beside the workspace's other tests, it ran
    // past a fixed minute.
    let output = tokio::time::timeout(
        tqa_testkit::deadline::scaled(Duration::from_mins(1)),
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
