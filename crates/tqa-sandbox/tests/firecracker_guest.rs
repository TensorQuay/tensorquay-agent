//! The sandbox contract inside real microVMs, beyond posture (SBX-C on firecracker): persistence, limits, clock, a full
//! workspace, a derived template, create and destroy, and an unknown image. The probe template reports each verdict
//! through the exit record; requirements and how to run are in tests/firecracker/support.rs.

use std::{path::PathBuf, process::Command, time::SystemTime};

use tokio_util::sync::CancellationToken;
use tqa_sandbox::{ByteSize, CpuLimit, ImageRef, SandboxBackend, SandboxError, WorkspaceSource};

#[path = "firecracker/support.rs"]
mod support;

use support::{Template, TestResult, host};

fn passed(suite: &str, (code, report): (i32, String)) -> TestResult {
    if code == 0 {
        Ok(())
    } else {
        Err(format!("the {suite} probe failed inside the guest (exit {code}): {report}").into())
    }
}

#[tokio::test]
#[ignore = "needs KVM and host setup"]
async fn sbx_c_002_the_workspace_image_persists_across_vms() -> TestResult {
    let host = host(&Template::Probe).await?;
    let image: PathBuf = host.storage.path().join("workspaces/persisted.ext4");
    std::fs::create_dir_all(host.storage.path().join("workspaces"))?;
    for suite in ["persist-write", "persist-read"] {
        let reuse = |spec: &mut tqa_sandbox::SandboxSpec| {
            spec.workspace.source = WorkspaceSource::BlockImage(image.clone());
        };
        passed(suite, host.probe_with(suite, &[], reuse).await?)?;
    }
    Ok(())
}

#[tokio::test]
#[ignore = "needs KVM and host setup"]
async fn sbx_c_003_memory_is_killed_inside_the_guest_and_cpu_stays_within_the_vcpus() -> TestResult
{
    let host = host(&Template::Probe).await?;
    let report = host
        .probe_with("limits", &[("PROBE_VCPUS", "2")], |spec| {
            spec.cpu = CpuLimit::new(2000).unwrap_or(spec.cpu);
        })
        .await?;
    passed("limits", report)
}

#[tokio::test]
#[ignore = "needs KVM and host setup"]
async fn sbx_c_012_the_guest_clock_is_utc_and_close_to_the_host() -> TestResult {
    let host = host(&Template::Probe).await?;
    let epoch = || {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|now| now.as_secs_f64())
    };
    let before = epoch()?;
    let report = host.probe("clock").await?;
    let after = epoch()?;
    let guest: f64 = report
        .1
        .lines()
        .find_map(|line| {
            line.split_once("tqa-probe: clock ")
                .map(|(_, time)| time.trim().parse())
        })
        .ok_or("no guest time")??;
    passed("clock", report)?;
    // The guest read its clock between our two readings; 5 s of skew is allowed either way.
    assert!(
        (before - 5.0..=after + 5.0).contains(&guest),
        "guest {guest}, host {before}..{after}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "needs KVM and host setup"]
async fn sbx_c_015_a_full_workspace_image_fails_with_enospc_inside_the_guest() -> TestResult {
    let host = host(&Template::Probe).await?;
    let small = ByteSize::new(64 << 20)?;
    passed(
        "fill",
        host.probe_with("fill", &[], |spec| spec.disk = Some(small))
            .await?,
    )
}

#[tokio::test]
#[ignore = "needs KVM and host setup"]
async fn sbx_c_014_a_derived_template_keeps_the_posture() -> TestResult {
    let host = host(&Template::DerivedProbe).await?;
    passed("posture", host.probe("posture").await?)
}

#[tokio::test]
#[ignore = "needs KVM and host setup"]
async fn sbx_c_001_sbx_f_004_destroy_is_idempotent_and_leaves_no_vm_host_or_files() -> TestResult {
    let host = host(&Template::Probe).await?;
    let handle = host
        .backend
        .create(
            host.spec(&[("PROBE_CHECK", "hold")])?,
            CancellationToken::new(),
        )
        .await?;
    host.backend.destroy(&handle).await?;
    host.backend.destroy(&handle).await?;
    assert!(
        host.backend
            .list()
            .await?
            .iter()
            .all(|listed| listed.id != handle.id)
    );
    // The VM host and its network namespace (tap and rules) are gone with the container; no per-VM files remain.
    let inspect = Command::new("docker")
        .args(["inspect", &handle.id])
        .output()?;
    assert!(!inspect.status.success(), "the VM host container is gone");
    let leftovers: Vec<_> = std::fs::read_dir(host.storage.path())?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().contains(&handle.id))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "needs KVM and host setup"]
async fn sbx_c_010_an_unknown_image_is_a_typed_error_that_leaves_nothing() -> TestResult {
    let host = host(&Template::Probe).await?;
    let mut spec = host.spec(&[("PROBE_CHECK", "hold")])?;
    spec.image = ImageRef::local_id(format!("sha256:{}", "0".repeat(64)))?;
    let before = host.backend.list().await?.len();
    let refused = host.backend.create(spec, CancellationToken::new()).await;
    // Refused by name before any container starts: no verified rootfs exists for that image.
    assert!(
        matches!(&refused, Err(SandboxError::Preflight(reason)) if reason.contains("rootfs")),
        "{refused:?}"
    );
    assert_eq!(
        host.backend.list().await?.len(),
        before,
        "no VM host was left"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "needs KVM and host setup"]
async fn sbx_c_014_an_image_secret_the_spec_does_not_set_is_refused() -> TestResult {
    // The real builder records the key (never its value); the backend refuses, as Docker does, before any VM host.
    let host = host(&Template::SecretEnvProbe).await?;
    let mut spec = host.spec(&[("PROBE_CHECK", "hold")])?;
    spec.env.remove("API_TOKEN");
    let before = host.backend.list().await?.len();
    let refused = host.backend.create(spec, CancellationToken::new()).await;
    assert!(
        matches!(&refused, Err(SandboxError::InvalidSpec { field: "image", reason }) if reason.contains("API_TOKEN")),
        "{refused:?}"
    );
    assert!(
        !format!("{refused:?}").contains("synthetic-image-secret"),
        "the value is never echoed"
    );
    assert_eq!(
        host.backend.list().await?.len(),
        before,
        "no VM host was started"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "needs KVM and host setup"]
async fn sbx_f_022_a_template_pinned_by_registry_digest_boots() -> TestResult {
    // Regression, not discriminating on the containerd image store: there a registry digest equals the image ID, so this
    // passes with or without SBX-F-022's fix. The unit and backend tests cover the classic-store case, where they differ.
    // What it does prove: the pin is located through the manifest's recorded repository digests and the guest boots.
    let host = host(&Template::Probe).await?;
    let pinned = ImageRef::new("tqa-fc-probe", host.image.digest())?;
    passed(
        "posture",
        host.probe_with("posture", &[], move |spec| spec.image = pinned)
            .await?,
    )
}

#[tokio::test]
#[ignore = "needs KVM and host setup"]
async fn sbx_f_023_create_refuses_a_rootfs_built_by_another_vm_host_image() -> TestResult {
    // The probe's rootfs was built by TQA_VMM_IMAGE. After a VM host image upgrade (here: the same image with a
    // label added, so a new ID) that build's guest init may be stale: refused by name before any VM host starts.
    let host = host(&Template::Probe).await?;
    let upgraded = support::backend_with_vmm(&host, support::upgraded_vmm()?).await?;
    let before = upgraded.list().await?.len();
    let refused = upgraded
        .create(
            host.spec(&[("PROBE_CHECK", "hold")])?,
            CancellationToken::new(),
        )
        .await;
    assert!(
        matches!(&refused, Err(SandboxError::Preflight(reason)) if reason.contains("another VM host image")),
        "{refused:?}"
    );
    assert_eq!(
        upgraded.list().await?.len(),
        before,
        "no VM host was started"
    );
    // The status agrees, and the backend with the builder's own VM host image still boots the template.
    assert_eq!(
        upgraded.template_status(&host.image).await?,
        tqa_sandbox::firecracker::RootfsStatus::Outdated
    );
    passed("posture", host.probe("posture").await?)
}

/// A number from `dumpe2fs -h` output: the value after `label:`.
fn superblock(text: &str, label: &str) -> Result<u64, Box<dyn std::error::Error>> {
    Ok(text
        .lines()
        .find_map(|line| line.strip_prefix(label)?.strip_prefix(':'))
        .ok_or(format!("dumpe2fs has no {label}"))?
        .trim()
        .parse()?)
}

// TEST: SBX-F-025
#[tokio::test]
#[ignore = "needs KVM and host setup"]
async fn sbx_f_025_a_new_workspace_image_holds_its_quota() -> TestResult {
    let host = host(&Template::Probe).await?;
    let quota: u64 = 1 << 30;
    let mut spec = host.spec(&[("PROBE_CHECK", "hold")])?;
    spec.disk = Some(ByteSize::new(quota)?);
    let WorkspaceSource::BlockImage(image) = spec.workspace.source.clone() else {
        return Err("the Firecracker spec has a block image".into());
    };
    let handle = host.backend.create(spec, CancellationToken::new()).await?;
    host.backend.destroy(&handle).await?;
    // The quota plus max(5%, 128 MiB) of ext4 metadata.
    assert_eq!(std::fs::metadata(&image)?.len(), 1152 << 20);
    // What the guest (uid 1000) can use, read from the superblock the backend's own mkfs wrote.
    let directory = image.parent().ok_or("image directory")?;
    let name = image
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("image name")?;
    let vmm = std::env::var("TQA_VMM_IMAGE")?;
    let mount = format!("type=bind,src={},dst=/img,readonly", directory.display());
    let header = support::run(Command::new("docker").args([
        "run",
        "--rm",
        "--network",
        "none",
        "--mount",
        &mount,
        "--entrypoint",
        "/usr/sbin/dumpe2fs",
        &vmm,
        "-h",
        &format!("/img/{name}"),
    ]))?;
    let block = superblock(&header, "Block size")?;
    let (free, reserved) = (
        superblock(&header, "Free blocks")?,
        superblock(&header, "Reserved block count")?,
    );
    assert_eq!(reserved, 0, "no root reserve");
    let usable = (free - reserved) * block;
    assert!(usable >= quota, "usable {usable} < quota {quota}");
    Ok(())
}
