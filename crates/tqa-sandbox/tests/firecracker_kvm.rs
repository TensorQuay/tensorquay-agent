//! Firecracker backend suites that need the container engine or KVM (ADR-0006). Every KVM test starts from
//! `prepared_backend`, which runs the real host preflight: on an unprepared host it returns the preflight's reason and
//! the test fails with it. No test here can pass without the host it claims to exercise (SBX-F-015).

use std::{error::Error, net::Ipv4Addr, path::PathBuf};

use tqa_sandbox::{
    ImageRef, SandboxError,
    firecracker::{FirecrackerBackend, FirecrackerBackendConfig},
};

#[path = "firecracker/support.rs"]
mod support;

type TestResult = Result<(), Box<dyn Error>>;

const NETWORK: &str = "tqa-fc-test";

/// The suite's internal network, created on first use; an existing one is reused.
fn internal_network() -> Result<(), Box<dyn Error>> {
    let inspect = std::process::Command::new("docker")
        .args(["network", "inspect", "-f", "{{.Internal}}", NETWORK])
        .output()?;
    if inspect.status.success() {
        return if String::from_utf8(inspect.stdout)?.trim() == "true" {
            Ok(())
        } else {
            Err(format!("{NETWORK} exists but is not internal").into())
        };
    }
    let created = std::process::Command::new("docker")
        .args(["network", "create", "--internal", NETWORK])
        .output()?;
    if created.status.success() {
        Ok(())
    } else {
        Err("creating the internal test network failed".into())
    }
}

fn config(storage: PathBuf, vmm_image: ImageRef) -> Result<FirecrackerBackendConfig, SandboxError> {
    let mut config = FirecrackerBackendConfig::new(storage, vmm_image, NETWORK.into(), vec![8080])?;
    config.gateway_address = Ipv4Addr::new(192, 0, 2, 10);
    Ok(config)
}

/// The backend for KVM tests, from `TQA_VMM_IMAGE` (a built images/vmm image ID) and `TQA_FC_KERNEL_DIR` (vmlinux and
/// its manifest). Any missing requirement is returned as an error, so the calling test fails with the reason.
async fn prepared_backend(storage: PathBuf) -> Result<FirecrackerBackend, Box<dyn Error>> {
    let image = std::env::var("TQA_VMM_IMAGE")
        .map_err(|_| "TQA_VMM_IMAGE must name a built images/vmm image ID")?;
    let kernel = std::env::var("TQA_FC_KERNEL_DIR")
        .map_err(|_| "TQA_FC_KERNEL_DIR must name a built kernel directory")?;
    let mut config = config(storage, ImageRef::local_id(image)?)?;
    config.kernel_dir = kernel.into();
    internal_network()?;
    Ok(support::connect(config).await?)
}

#[tokio::test]
#[ignore = "docker"]
async fn sbx_f_015_the_kvm_guard_fails_loudly_on_an_unprepared_host() -> TestResult {
    let storage = tempfile::tempdir()?;
    let absent = ImageRef::local_id(format!("sha256:{}", "0".repeat(64)))?;
    let refused = FirecrackerBackend::connect(config(storage.path().into(), absent)?).await;
    let error = refused
        .err()
        .ok_or("the guard passed on a host without the VM host image")?;
    assert_eq!(
        error.to_string(),
        "sandbox preflight failed: the VM host image is not present (build images/vmm)"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "needs KVM and host setup"]
async fn sbx_f_001_the_real_preflight_passes_on_a_prepared_host() -> TestResult {
    // Opens /dev/kvm read-write as the VMM uid and creates a tap inside VM host containers, proves ACL support on
    // the storage root and verifies the kernel against its manifest.
    let storage = tempfile::tempdir_in(
        std::env::var("TQA_FC_STORAGE").unwrap_or_else(|_| "/var/tmp".into()),
    )?;
    prepared_backend(storage.path().into()).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "needs KVM and host setup"]
async fn sbx_f_001_a_missing_sandbox_network_is_refused_by_name() -> TestResult {
    let image = std::env::var("TQA_VMM_IMAGE")
        .map_err(|_| "TQA_VMM_IMAGE must name a built images/vmm image ID")?;
    let storage = tempfile::tempdir()?;
    let mut config = config(storage.path().into(), ImageRef::local_id(image)?)?;
    config.network = "tqa-fc-absent-network".into();
    let error = FirecrackerBackend::connect(config)
        .await
        .err()
        .ok_or("accepted a missing network")?;
    assert_eq!(
        error.to_string(),
        "sandbox preflight failed: the sandbox network does not exist (start the gateway on its internal network first)"
    );
    Ok(())
}

/// A container left as a gateway stopped mid-helper leaves it: the instance's helper labels, created or running.
fn leave_helper(name: &str, image: &str, start: bool) -> TestResult {
    let mut args = vec![if start { "run" } else { "create" }];
    if start {
        args.push("--detach");
    }
    args.extend(["--name", name, "--label", "tqa.role=helper", "--label"]);
    let instance = format!("tqa.instance={NETWORK}");
    args.push(&instance);
    args.extend([
        "--network",
        "none",
        "--entrypoint",
        "/bin/sleep",
        image,
        "300",
    ]);
    let made = std::process::Command::new("docker").args(&args).output()?;
    if made.status.success() {
        Ok(())
    } else {
        Err(format!(
            "leaving {name} failed: {}",
            String::from_utf8_lossy(&made.stderr)
        )
        .into())
    }
}

fn exists(name: &str) -> Result<bool, Box<dyn Error>> {
    Ok(std::process::Command::new("docker")
        .args(["container", "inspect", name])
        .output()?
        .status
        .success())
}

// TEST: SBX-F-024
#[tokio::test]
#[ignore = "needs KVM and host setup"]
async fn sbx_f_024_a_real_start_removes_this_instances_leftover_helpers() -> TestResult {
    let image = std::env::var("TQA_VMM_IMAGE")
        .map_err(|_| "TQA_VMM_IMAGE must name a built images/vmm image ID")?;
    let (created, running) = (
        "tqa-fc-test-helper-left-created",
        "tqa-fc-test-helper-left-running",
    );
    for name in [created, running] {
        let _ = std::process::Command::new("docker")
            .args(["rm", "--force", name])
            .output();
    }
    leave_helper(created, &image, false)?;
    leave_helper(running, &image, true)?;
    assert!(exists(created)? && exists(running)?);
    let storage = tempfile::tempdir_in(
        std::env::var("TQA_FC_STORAGE").unwrap_or_else(|_| "/var/tmp".into()),
    )?;
    prepared_backend(storage.path().into()).await?;
    assert!(
        !exists(created)?,
        "a created, never started helper is removed"
    );
    assert!(!exists(running)?, "a running leftover helper is removed");
    Ok(())
}
