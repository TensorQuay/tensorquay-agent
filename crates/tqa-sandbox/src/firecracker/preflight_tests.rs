use std::{
    error::Error,
    fs,
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use serde_json::json;
use sha2::{Digest, Sha256};

use super::{HostProbe, run};
use crate::firecracker::{NetworkState, manifest::hex};

const ZERO: &str = "0000000000000000000000000000000000000000000000000000000000000000";
use crate::{ImageRef, SandboxError, firecracker::FirecrackerBackendConfig};

struct Fake {
    image: bool,
    kvm: bool,
    tun: bool,
    network: NetworkState,
    calls: AtomicUsize,
}

impl Fake {
    fn new(image: bool, kvm: bool, tun: bool) -> Self {
        Self {
            image,
            kvm,
            tun,
            network: NetworkState::Internal,
            calls: AtomicUsize::new(0),
        }
    }

    fn with_network(network: NetworkState) -> Self {
        Self {
            network,
            ..Self::new(true, true, true)
        }
    }

    fn answer(&self, value: bool) -> bool {
        self.calls.fetch_add(1, Ordering::Relaxed);
        value
    }
}

#[async_trait]
impl HostProbe for Fake {
    async fn vmm_image_present(&self) -> Result<bool, SandboxError> {
        Ok(self.answer(self.image))
    }
    async fn sandbox_network(&self) -> Result<NetworkState, SandboxError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(self.network)
    }
    async fn kvm_opens_for_vmm(&self) -> Result<bool, SandboxError> {
        Ok(self.answer(self.kvm))
    }
    async fn tun_available(&self) -> Result<bool, SandboxError> {
        Ok(self.answer(self.tun))
    }
}

fn host(root: &Path, vmlinux: &[u8]) -> Result<FirecrackerBackendConfig, Box<dyn Error>> {
    let kernel = root.join("kernel");
    fs::create_dir_all(&kernel)?;
    fs::write(kernel.join("vmlinux"), b"kernel")?;
    let digest = hex(&Sha256::digest(vmlinux));
    let manifest = json!({
        "schema": 1, "release": "6.18.54", "licence": "GPL-2.0-only", "source_sha256": ZERO, "base_config": "b",
        "base_config_sha256": ZERO, "fragment_sha256": ZERO, "config_sha256": ZERO, "vmlinux_sha256": digest
    });
    fs::write(kernel.join("manifest.json"), serde_json::to_vec(&manifest)?)?;
    let image = ImageRef::local_id(format!("sha256:{}", "a".repeat(64)))?;
    let mut config =
        FirecrackerBackendConfig::new(root.to_path_buf(), image, "tqa-sandbox".into(), vec![8080])?;
    config.gateway_address = std::net::Ipv4Addr::new(192, 0, 2, 10);
    Ok(config)
}

async fn reason(config: &FirecrackerBackendConfig, probe: &Fake) -> Option<&'static str> {
    match run(config, probe).await {
        Err(SandboxError::Preflight(reason)) => Some(reason),
        _ => None,
    }
}

#[tokio::test]
async fn sbx_f_001_each_missing_requirement_is_refused_with_its_reason()
-> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let config = host(directory.path(), b"kernel")?;
    let no_image = Fake::new(false, true, true);
    assert_eq!(
        reason(&config, &no_image).await,
        Some("the VM host image is not present (build images/vmm)")
    );
    assert_eq!(
        no_image.calls.load(Ordering::Relaxed),
        1,
        "nothing is started from a missing image"
    );
    assert_eq!(
        reason(&config, &Fake::new(true, false, true)).await,
        Some(
            "/dev/kvm does not open for the VMM uid (run deploy/firecracker/host-setup.sh --check)"
        )
    );
    assert_eq!(
        reason(&config, &Fake::new(true, true, false)).await,
        Some("/dev/net/tun is not available to the VM host container")
    );
    let wrong_kernel = host(directory.path(), b"another kernel")?;
    assert_eq!(
        reason(&wrong_kernel, &Fake::new(true, true, true)).await,
        Some("the guest kernel does not match its manifest")
    );
    Ok(())
}

#[tokio::test]
async fn sbx_f_001_a_prepared_host_passes_and_leaves_no_probe_file() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let config = host(directory.path(), b"kernel")?;
    assert_eq!(
        run(&config, &Fake::new(true, true, true))
            .await?
            .kernel_release,
        "6.18.54"
    );
    let names: Vec<_> = fs::read_dir(directory.path())?
        .map(|entry| entry.map(|e| e.file_name()))
        .collect::<Result<_, _>>()?;
    assert_eq!(names, ["kernel"]);
    Ok(())
}

#[tokio::test]
async fn sbx_f_001_the_sandbox_network_must_exist_and_be_internal() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let config = host(directory.path(), b"kernel")?;
    for (state, expected) in [
        (
            NetworkState::Missing,
            "the sandbox network does not exist (start the gateway on its internal network first)",
        ),
        (
            NetworkState::NotInternal,
            "the sandbox network is not internal, so it cannot confine a guest",
        ),
    ] {
        let probe = Fake::with_network(state);
        assert_eq!(reason(&config, &probe).await, Some(expected));
        assert_eq!(
            probe.calls.load(Ordering::Relaxed),
            2,
            "no VM host container is started"
        );
    }
    Ok(())
}
