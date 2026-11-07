//! RT-I-015: real Git checkout through the policy proxy inside an admitted sandbox.
#[tokio::test]
#[ignore = "needs Docker opt-in and gateway, runtime and clone-probe image pins"]
async fn rt_i_015_workspace_clone_uses_the_real_policy_proxy()
-> Result<(), Box<dyn std::error::Error>> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let status = tokio::process::Command::new("python3")
        .args(["-B", "crates/tqa-sandbox/tests/workspace_clone.py"])
        .current_dir(root)
        .kill_on_drop(true)
        .status()
        .await?;
    assert!(status.success(), "real-stack clone contract failed");
    Ok(())
}
