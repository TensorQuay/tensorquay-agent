//! GW-I-326 and GW-I-327: the administrator's Sandboxes page on a real Docker engine with the testkit's fake model.
//! Each runs `deploy/tests/sandboxes_stack.py` against an owned stack (its own name, pool, port and installation).
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
        Duration::from_mins(15),
        tokio::process::Command::new("python3")
            .args(["deploy/tests/sandboxes_stack.py", case])
            .env("PYTHONPATH", "deploy/tests")
            .current_dir(root)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .status(),
    )
    .await??;
    assert!(
        status.success(),
        "real-stack Sandboxes contract failed: {case}"
    );
    Ok(())
}

// TEST: GW-I-326
#[tokio::test]
#[ignore = "requires TQA_DOCKER_TESTS=1 and built gateway, runtime, web and model images"]
async fn gw_i_326_the_listing_names_three_sandboxes_of_two_people_and_measures_them_on_the_real_engine()
-> Result<(), Box<dyn Error>> {
    contract("listing").await
}

// TEST: GW-I-327
#[tokio::test]
#[ignore = "requires TQA_DOCKER_TESTS=1 and built gateway, runtime, web and model images"]
async fn gw_i_327_an_administrators_stop_on_the_real_engine_and_the_task_resumes_on_its_files()
-> Result<(), Box<dyn Error>> {
    contract("stop").await
}
