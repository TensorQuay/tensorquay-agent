//! SBX-C-006/007/016 and SEC-014: isolation in the production runtime image.
use std::{collections::BTreeMap, time::Duration};

use bollard::exec::{CreateExecOptions, StartExecResults};
use futures::StreamExt;
use tokio::{io::AsyncWriteExt, time::timeout};
use tokio_util::sync::CancellationToken;
use tqa_sandbox::{
    ByteSize, CpuLimit, ImageRef, SandboxBackend, SandboxHandle, docker::DockerBackend,
};

#[path = "support/docker.rs"]
mod support;
use support::{TestResult, bounded, run};

async fn runtime(fixture: &support::Fixture) -> Result<SandboxHandle, Box<dyn std::error::Error>> {
    let selected = match std::env::var("TQA_RUNTIME_TEST_IMAGE") {
        Ok(value) if !value.is_empty() => value,
        Err(std::env::VarError::NotPresent) => "tqa-runtime:workstation".into(),
        _ => return Err("TQA_RUNTIME_TEST_IMAGE must be nonempty UTF-8".into()),
    };
    let image = bounded(fixture.docker.inspect_image(&selected))
        .await?
        .id
        .ok_or("runtime image ID missing")?;
    let mut spec = fixture.spec("posture", true)?;
    spec.image = ImageRef::local_id(&image)?;
    assert!(fixture.backend.image_present(&spec.image).await?);
    spec.gateway_url = "http://gateway.invalid".parse()?;
    spec.env = BTreeMap::from([
        (
            "PATH".into(),
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".into(),
        ),
        ("HOME".into(), "/tmp".into()),
    ]);
    spec.cpu = CpuLimit::new(1000)?;
    spec.memory = ByteSize::new(512 * 1024 * 1024)?;
    spec.pids = 256;
    let backend = DockerBackend::connect(fixture.backend_config()?).await?;
    let handle = backend.create(spec, CancellationToken::new()).await?;
    let inspect = bounded(fixture.docker.inspect_container(&handle.id, None)).await?;
    assert_eq!(inspect.image.as_deref(), Some(image.as_str()));
    assert_eq!(inspect.state.and_then(|state| state.running), Some(true));
    Ok(handle)
}

#[tokio::test]
#[ignore = "needs Docker, TQA_DOCKER_TESTS=1 and a Workstation runtime image"]
async fn sbx_c_006_runtime_root_is_read_only_with_writable_workspace_and_tmp() -> TestResult {
    run(async |fixture| {
        let handle = runtime(fixture).await?;
        for path in ["/usr/root-probe", "/etc/root-probe"] {
            let (exit, output) = fixture.exec(&handle.id, &["touch", path]).await?;
            assert_ne!(exit, 0, "{path}");
            assert!(output.contains("Read-only file system"), "{path}: {output}");
        }
        let script = "printf inside > /workspace/probe; printf temporary > /tmp/probe; cat /workspace/probe /tmp/probe";
        assert_eq!(
            fixture.exec(&handle.id, &["sh", "-ec", script]).await?,
            (0, "insidetemporary".into())
        );
        assert_eq!(std::fs::read(fixture.workspace.path().join("probe"))?, b"inside");
        Ok(())
    })
    .await
}

#[tokio::test]
#[ignore = "needs Docker, TQA_DOCKER_TESTS=1 and a Workstation runtime image"]
async fn sbx_c_007_runtime_refuses_mount_and_exposes_only_its_private_pid_namespace() -> TestResult
{
    run(async |fixture| {
        let handle = runtime(fixture).await?;
        let host = bounded(fixture.docker.inspect_container(&handle.id, None))
            .await?
            .host_config
            .ok_or("missing host configuration")?;
        assert!(matches!(
            host.pid_mode.as_deref(),
            None | Some("" | "private")
        ));
        private_pid_witness(fixture, &handle.id).await
    })
    .await
}

async fn private_pid_witness(fixture: &support::Fixture, id: &str) -> TestResult {
    let created = bounded(fixture.docker.create_exec(
        id,
        CreateExecOptions {
            attach_stdin: Some(true),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            cmd: Some(vec![
                "python3",
                "-c",
                include_str!("support/privilege_probe.py"),
            ]),
            ..Default::default()
        },
    ))
    .await?;
    let StartExecResults::Attached {
        mut output,
        mut input,
    } = bounded(fixture.docker.start_exec(&created.id, None)).await?
    else {
        return Err("privilege probe was not attached".into());
    };
    timeout(Duration::from_secs(30), async {
        let mut snapshot = String::new();
        while !snapshot.ends_with('\n') {
            let chunk = output
                .next()
                .await
                .ok_or("probe exited before its snapshot")??;
            snapshot.push_str(&chunk.to_string());
            assert!(
                snapshot.len() <= 4096,
                "privilege probe output exceeded its bound"
            );
        }
        let inside: Vec<u32> = serde_json::from_str(&snapshot)?;
        let outside = bounded(fixture.docker.top_processes(id, None))
            .await?
            .processes
            .ok_or("missing host-side process list")?;
        assert!(inside.len() >= 2, "init and the live probe must be present");
        assert_eq!(
            inside.len(),
            outside.len(),
            "/proc exposes processes outside the sandbox"
        );
        input.write_all(b"release\n").await?;
        while let Some(chunk) = output.next().await {
            assert!(
                chunk?.to_string().is_empty(),
                "probe failed after its snapshot"
            );
        }
        let exit = bounded(fixture.docker.inspect_exec(&created.id))
            .await?
            .exit_code;
        assert_eq!(exit, Some(0));
        Ok::<_, Box<dyn std::error::Error>>(())
    })
    .await?
}

#[tokio::test]
#[ignore = "needs Docker, TQA_DOCKER_TESTS=1 and a Workstation runtime image"]
async fn sbx_c_016_sec_014_runtime_sees_one_workspace_and_no_host_path_or_block_device()
-> TestResult {
    // TEST: SEC-014
    run(async |fixture| {
        let other = tempfile::tempdir()?;
        let canary = other.path().join("canary");
        std::fs::write(&canary, "other-workspace-unchanged")?;
        std::fs::write(fixture.workspace.path().join("witness"), "bound-workspace")?;
        let handle = runtime(fixture).await?;
        let (exit, output) = fixture
            .exec(
                &handle.id,
                &[
                    "python3",
                    "-c",
                    include_str!("support/workspace_probe.py"),
                    canary.to_str().ok_or("host canary path encoding")?,
                    fixture
                        .workspace
                        .path()
                        .to_str()
                        .ok_or("workspace path encoding")?,
                ],
            )
            .await?;
        assert_eq!(exit, 0, "{output}");
        let observed: serde_json::Value = serde_json::from_str(&output)?;
        assert_eq!(
            observed["workspace_mounts"],
            serde_json::json!(["/workspace"])
        );
        assert_eq!(observed["block_devices"], serde_json::json!([]));
        assert_eq!(observed["host_paths_refused"], serde_json::json!(2));
        assert_eq!(
            std::fs::read_to_string(canary)?,
            "other-workspace-unchanged"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.workspace.path().join("witness"))?,
            "bound-workspace"
        );
        Ok(())
    })
    .await
}
