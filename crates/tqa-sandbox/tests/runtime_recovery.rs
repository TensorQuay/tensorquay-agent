//! Persistent workspace and WAL recovery through actual runtime containers.
#[path = "support/recovery.rs"]
mod recovery;
#[path = "support/docker.rs"]
mod support;
use recovery::{Cut, Task, exercise, idle_destroy};
use support::{TestResult, bounded};
use tqa_sandbox::{SandboxBackend, SandboxHandle, docker::DockerBackend};

async fn kill(_: &DockerBackend, docker: &bollard::Docker, handle: &SandboxHandle) -> TestResult {
    bounded(docker.kill_container(
        &handle.id,
        Some(bollard::query_parameters::KillContainerOptions {
            signal: "KILL".into(),
        }),
    ))
    .await?;
    Ok(())
}
async fn destroy(
    backend: &DockerBackend,
    _: &bollard::Docker,
    handle: &SandboxHandle,
) -> TestResult {
    backend.destroy(handle).await?;
    Ok(())
}
async fn both_cuts(task: Task) -> TestResult {
    for cut in [Cut::Model, Cut::Tool] {
        exercise(task, cut, kill).await?;
    }
    Ok(())
}

#[tokio::test]
#[ignore = "needs Docker, a Workstation image, TQA_DOCKER_TESTS=1 and TQA_ALLOW_DEV_HOST_GATEWAY=1"]
async fn sbx_c_013_destroy_busy_runtime_preserves_the_wal_and_resumes() -> TestResult {
    exercise(Task::Arithmetic, Cut::Tool, destroy).await
}

#[tokio::test]
#[ignore = "needs Docker, a Workstation image, TQA_DOCKER_TESTS=1 and TQA_ALLOW_DEV_HOST_GATEWAY=1"]
async fn e2e_resume_005_arithmetic_recovers_mid_model_and_mid_tool() -> TestResult {
    both_cuts(Task::Arithmetic).await
}
#[tokio::test]
#[ignore = "needs Docker, a Workstation image, TQA_DOCKER_TESTS=1 and TQA_ALLOW_DEV_HOST_GATEWAY=1"]
async fn e2e_resume_005_stable_deduplication_recovers_mid_model_and_mid_tool() -> TestResult {
    both_cuts(Task::StableUnique).await
}
#[tokio::test]
#[ignore = "needs Docker, a Workstation image, TQA_DOCKER_TESTS=1 and TQA_ALLOW_DEV_HOST_GATEWAY=1"]
async fn e2e_resume_005_word_count_recovers_mid_model_and_mid_tool() -> TestResult {
    both_cuts(Task::Words).await
}
#[tokio::test]
#[ignore = "needs Docker, a Workstation image, TQA_DOCKER_TESTS=1 and TQA_ALLOW_DEV_HOST_GATEWAY=1"]
async fn e2e_resume_005_csv_totals_recovers_mid_model_and_mid_tool() -> TestResult {
    both_cuts(Task::CsvTotals).await
}
#[tokio::test]
#[ignore = "needs Docker, a Workstation image, TQA_DOCKER_TESTS=1 and TQA_ALLOW_DEV_HOST_GATEWAY=1"]
async fn e2e_resume_005_bounds_checking_recovers_mid_model_and_mid_tool() -> TestResult {
    both_cuts(Task::Bounds).await
}

// TEST: SBX-C-018
#[tokio::test]
#[ignore = "needs Docker, a Workstation image, TQA_DOCKER_TESTS=1 and TQA_ALLOW_DEV_HOST_GATEWAY=1"]
async fn sbx_c_018_an_idle_runtime_is_destroyed_within_three_seconds() -> TestResult {
    // GW-I-158's case: a person's new task waits while their idle sandbox retires; nothing runs in it, so it goes at
    // once.
    let elapsed = idle_destroy().await?;
    assert!(elapsed < std::time::Duration::from_secs(3), "{elapsed:?}");
    Ok(())
}
