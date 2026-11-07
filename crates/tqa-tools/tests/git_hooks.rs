//! SEC-045: a hook and a file-system monitor planted in a shared repository don't run under the shell's git
//! (ADR-0035 §1). One member's agent writes them into `.git`; another member's agent runs git in the shell, and
//! nothing planted runs. The same commands without the tool's environment do run them, so the plant is real.
#[path = "support/context.rs"]
mod support;
use serde_json::json;
use std::{error::Error, fs, os::unix::fs::PermissionsExt, path::Path, process::Command};
use tqa_testkit::workspace_fixture;
use tqa_tools::{Tool, builtin::Shell};

type TestResult = Result<(), Box<dyn Error>>;

fn git(root: &Path, args: &[&str]) -> Result<(), Box<dyn Error>> {
    let status = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdout(std::process::Stdio::null())
        .status()?;
    assert!(status.success(), "git {args:?}");
    Ok(())
}

/// A repository with one commit, a `post-checkout` hook and an fsmonitor command, each leaving a marker file.
fn plant(root: &Path) -> Result<(), Box<dyn Error>> {
    git(root, &["init", "--quiet", "--initial-branch=main"])?;
    fs::write(root.join("README.md"), "shared\n")?;
    git(root, &["add", "README.md"])?;
    git(
        root,
        &[
            "-c",
            "user.name=A",
            "-c",
            "user.email=a@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "one",
        ],
    )?;
    let hook = root.join(".git/hooks/post-checkout");
    fs::write(
        &hook,
        "#!/bin/sh\ntouch \"$(git rev-parse --show-toplevel)/hook-ran\"\n",
    )?;
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755))?;
    let monitor = root.join(".git/monitor.sh");
    fs::write(
        &monitor,
        "#!/bin/sh\ntouch \"$(dirname \"$0\")/../monitor-ran\"\nexit 1\n",
    )?;
    fs::set_permissions(&monitor, fs::Permissions::from_mode(0o755))?;
    git(
        root,
        &["config", "core.fsmonitor", monitor.to_str().ok_or("path")?],
    )?;
    Ok(())
}

#[tokio::test]
async fn sec_045_a_planted_hook_and_fsmonitor_do_not_run_under_shell_git() -> TestResult {
    let fixture = workspace_fixture()?;
    // A repository of its own, inside the workspace (the fixture's `.git` is not a real one).
    let root = &fixture.root().join("shared");
    fs::create_dir(root)?;
    plant(root)?;
    let cx = support::context(&fixture);
    let output = Shell::new()
        .call(
            json!({"command":"cd shared && git status --short && git checkout --quiet -b other"}),
            &cx,
        )
        .await?;
    assert_eq!(output.exit_code, Some(0), "{}", output.text);
    assert!(!root.join("hook-ran").exists(), "the planted hook ran");
    assert!(
        !root.join("monitor-ran").exists(),
        "the planted monitor ran"
    );
    // The contrast: plain git, without the tool's environment, runs both.
    git(root, &["status", "--short"])?;
    git(root, &["checkout", "--quiet", "main"])?;
    assert!(root.join("hook-ran").exists(), "the fixture's hook is real");
    assert!(
        root.join("monitor-ran").exists(),
        "the fixture's monitor is real"
    );
    Ok(())
}
