//! GW-I-017/SBX-D-002: the one-box stack reaches readiness with a synthetic model route.
use std::{error::Error, process::Stdio, time::Duration};

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn gw_i_017_compose_stack_reaches_administrator_readiness() -> Result<(), Box<dyn Error>> {
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
            .arg("deploy/tests/compose.py")
            .current_dir(root)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .status(),
    )
    .await??;
    assert!(status.success(), "compose readiness contract failed");
    Ok(())
}

#[test]
fn gw_i_012_compose_never_publishes_the_egress_proxy() -> Result<(), Box<dyn Error>> {
    // The proxy listens only on the sandbox network (ADR-0021); no compose file may publish its port.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/compose");
    for entry in std::fs::read_dir(root)? {
        let path = entry?.path();
        if path.extension().is_some_and(|extension| extension == "yml") {
            let text = std::fs::read_to_string(&path)?;
            assert!(
                !text.contains("3128"),
                "{} publishes the egress proxy",
                path.display()
            );
        }
    }
    Ok(())
}
