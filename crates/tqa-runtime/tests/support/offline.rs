use super::container::{ContainerGuard, docker, image};
use std::time::Duration;

pub async fn run(script: &str) -> Result<String, Box<dyn std::error::Error>> {
    let unique = tempfile::tempdir()?;
    let name = format!(
        "tqa-workstation-{}",
        unique
            .path()
            .file_name()
            .ok_or("test directory")?
            .to_string_lossy()
            .replace('.', "")
    );
    let guard = ContainerGuard(name);
    let selected = image("tqa-runtime:workstation")?;
    let script = format!("{}\n{script}", include_str!("offline_environment.sh"));
    let output = docker(
        &[
            "run",
            "--name",
            &guard.0,
            "--network=none",
            "--read-only",
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges:true",
            "--security-opt=seccomp=builtin",
            "--pids-limit=256",
            "--memory=512m",
            "--cpus=2",
            "--tmpfs=/tmp:rw,nosuid,nodev,noexec,size=67108864,mode=1777",
            "--tmpfs=/workspace:rw,nosuid,nodev,exec,size=268435456,uid=1000,gid=1000,mode=0700",
            "--entrypoint=/usr/bin/tini",
            &selected,
            "--",
            "bash",
            "-c",
            &script,
        ],
        Duration::from_secs(90),
    )
    .await?;
    let configuration: serde_json::Value =
        serde_json::from_str(&docker(&["inspect", &guard.0], Duration::from_secs(10)).await?)?;
    assert_eq!(configuration[0]["Config"]["User"], "1000:1000");
    assert_eq!(configuration[0]["HostConfig"]["NetworkMode"], "none");
    assert_eq!(configuration[0]["HostConfig"]["ReadonlyRootfs"], true);
    assert_eq!(
        configuration[0]["HostConfig"]["CapDrop"],
        serde_json::json!(["ALL"])
    );
    docker(&["rm", &guard.0], Duration::from_secs(10)).await?;
    Ok(output)
}
