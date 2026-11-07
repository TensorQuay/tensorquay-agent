//! Opt-in process and database recovery contracts with a real runtime and owned installation.
use std::{error::Error, process::Stdio, time::Duration};

async fn contract(case: &str) -> Result<(), Box<dyn Error>> {
    if std::env::var("TQA_DOCKER_TESTS").ok().as_deref() != Some("1") {
        return Err("Docker tests require TQA_DOCKER_TESTS=1".into());
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or("workspace root")?;
    let status = tokio::time::timeout(
        Duration::from_mins(10),
        tokio::process::Command::new("python3")
            .args(["deploy/tests/robustness.py", case])
            .current_dir(root)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .status(),
    )
    .await??;
    assert!(
        status.success(),
        "real-stack robustness contract failed: {case}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires TQA_DOCKER_TESTS=1 and built gateway, runtime, web and model images"]
async fn gw_i_018_sigterm_with_live_stream_and_runtime_link() -> Result<(), Box<dyn Error>> {
    contract("shutdown").await
}

#[tokio::test]
#[ignore = "requires TQA_DOCKER_TESTS=1 and built gateway, runtime, web and model images"]
async fn gw_e_020_real_postgres_outage_replays_runtime_wal() -> Result<(), Box<dyn Error>> {
    contract("outage").await
}

#[tokio::test]
#[ignore = "requires TQA_DOCKER_TESTS=1 and built gateway, runtime, web and model images"]
async fn gw_i_066_a_busy_sandbox_is_retired_at_the_lifetime_and_its_run_resumes_on_the_real_stack()
-> Result<(), Box<dyn Error>> {
    contract("lifetime").await
}
