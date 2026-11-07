//! Explicitly coordinated Docker Desktop restart; never selected by ordinary evidence runs.
#![cfg(target_os = "macos")]
#[path = "support/recovery.rs"]
mod recovery;
#[path = "support/docker.rs"]
mod support;
use recovery::{Cut, Task, exercise};
use std::time::{Duration, Instant};
use support::TestResult;
use tqa_sandbox::{SandboxHandle, docker::DockerBackend};

#[tokio::test]
#[ignore = "restarts Docker Desktop; requires a coordinated window and TQA_DOCKER_RESTART_TEST=1"]
async fn desktop_restart_recovers_a_running_sandbox() -> TestResult {
    if std::env::var("TQA_DOCKER_RESTART_TEST").as_deref() != Ok("1") {
        return Err("a coordinated Docker restart requires TQA_DOCKER_RESTART_TEST=1".into());
    }
    exercise(Task::Arithmetic, Cut::Tool, restart).await
}

#[allow(
    clippy::print_stderr,
    reason = "Manual evidence records the observed daemon interruption."
)]
async fn restart(_: &DockerBackend, docker: &bollard::Docker, _: &SandboxHandle) -> TestResult {
    let started = Instant::now();
    let mut command = tokio::process::Command::new("docker");
    command
        .args(["desktop", "restart", "--timeout", "120"])
        .kill_on_drop(true);
    let restart = tokio::time::timeout(Duration::from_secs(130), command.output());
    tokio::pin!(restart);
    let mut observed_unavailable = false;
    let mut interval = tokio::time::interval(Duration::from_millis(100));
    let output = loop {
        tokio::select! {
            output = &mut restart => break output??,
            _ = interval.tick() => {
                if !matches!(tokio::time::timeout(Duration::from_secs(1), docker.ping()).await, Ok(Ok(_))) {
                    observed_unavailable = true;
                }
            }
        }
    };
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(observed_unavailable, "no daemon interruption was observed");
    eprintln!(
        "daemon_unavailable=true restart_elapsed_ms={}",
        started.elapsed().as_millis()
    );
    Ok(())
}
