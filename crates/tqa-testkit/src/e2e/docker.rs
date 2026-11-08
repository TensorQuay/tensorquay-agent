//! The two containers the harness runs itself. The seed step writes the task's
//! seed into a new workspace directory, which rootless Docker can only do from a container because the
//! storage belongs to the runtime's uid 1000. The check runs on the downloaded copy with no network.
use super::classify::CheckOutcome;
use std::{
    ffi::OsString,
    io,
    path::Path,
    process::Stdio,
    time::{Duration, SystemTime},
};
use tokio::{io::AsyncReadExt, process::Command};

/// The runtime image's user, which owns every workspace file.
const WORKSPACE_OWNER: &str = "1000:1000";
/// Fixed so the seed commit, and so the agent's baseline, is identical on every run.
const SEED_DATE: &str = "2026-01-01T00:00:00Z";
/// Check output kept for the result; a verdict line is short.
const CHECK_OUTPUT_BYTES: u64 = 64 * 1024;

/// Arguments that copy `seed` into the empty `workspace` and commit it as the agent's baseline.
#[must_use]
pub fn seed_args(image: &str, seed: &Path, workspace: &Path) -> Vec<OsString> {
    // `cp -a /seed/.` also copies the seed directory's mode onto the workspace root; the gateway requires the
    // root to stay private, so its mode is restored.
    let script = "set -eu; cp -a /seed/. /workspace/; chmod 0700 /workspace; cd /workspace; \
        git -c safe.directory=/workspace init -q -b main; \
        git -c safe.directory=/workspace add -A; \
        git -c safe.directory=/workspace -c user.name=seed -c user.email=seed@example.invalid \
        commit -q -m seed; chown -R 1000:1000 /workspace";
    let mut args = run_args("0:0");
    args.extend(mount(seed, "/seed", true));
    args.extend(mount(workspace, "/workspace", false));
    args.extend([
        "--env".into(),
        format!("GIT_AUTHOR_DATE={SEED_DATE}").into(),
    ]);
    args.extend([
        "--env".into(),
        format!("GIT_COMMITTER_DATE={SEED_DATE}").into(),
    ]);
    args.extend([
        "--entrypoint".into(),
        "sh".into(),
        image.into(),
        "-c".into(),
        script.into(),
    ]);
    args
}

/// Arguments that run `check/run.sh` on the copy as the workspace owner, under the name `name`.
#[must_use]
pub fn check_args(name: &str, image: &str, check: &Path, copy: &Path) -> Vec<OsString> {
    // Created, then started attached, and removed only once its verdict is in: `run --rm` would make the client wait
    // for the removal too, which a loaded engine can stretch past the check's bound (E2E-CMP-003's two misses).
    let mut args = run_args(WORKSPACE_OWNER);
    args[0] = "create".into();
    args.remove(1);
    args.extend([
        "--name".into(),
        name.into(),
        "--cpus".into(),
        "2".into(),
        "--memory".into(),
        "2g".into(),
    ]);
    args.extend(mount(check, "/check", true));
    args.extend(mount(copy, "/workspace", false));
    args.extend(["--workdir".into(), "/workspace".into()]);
    args.extend([
        "--entrypoint".into(),
        "sh".into(),
        image.into(),
        "/check/run.sh".into(),
    ]);
    args
}

/// Arguments that hand everything in `copy` back to the host user. Under rootless Docker the container's
/// root is the host user, while files a check writes belong to a subordinate id the host cannot remove.
#[must_use]
pub fn reclaim_args(image: &str, copy: &Path) -> Vec<OsString> {
    let mut args = run_args("0:0");
    args.extend(mount(copy, "/workspace", false));
    let script = "chown -R 0:0 /workspace";
    args.extend([
        "--entrypoint".into(),
        "sh".into(),
        image.into(),
        "-c".into(),
        script.into(),
    ]);
    args
}

/// Makes a copy removable by the host user again after a check.
///
/// # Errors
/// A failure to start Docker or a non-zero exit.
pub async fn reclaim(image: &str, copy: &Path) -> io::Result<()> {
    let status = Command::new("docker")
        .args(reclaim_args(image, copy))
        .status()
        .await?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other("the reclaim container failed"))
    }
}

/// Seeds a workspace directory.
///
/// # Errors
/// A failure to start Docker or a non-zero exit.
pub async fn seed(image: &str, seed: &Path, workspace: &Path) -> io::Result<()> {
    let status = Command::new("docker")
        .args(seed_args(image, seed, workspace))
        .status()
        .await?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other("the seed container failed"))
    }
}

/// Runs the check: exit 0 passes, 1 fails, anything else, a timeout or a start failure is broken.
/// Returns the verdict, the start of the check's output, and what a broken verdict needs to name its cause.
pub async fn check(image: &str, check: &Path, copy: &Path, timeout: Duration) -> CheckRun {
    check_with("docker".as_ref(), image, (check, copy), timeout).await
}

/// [`check`] with the Docker client `program`, so a test can stand in for a slow engine.
pub(super) async fn check_with(
    program: &std::ffi::OsStr,
    image: &str,
    (check, copy): (&Path, &Path),
    timeout: Duration,
) -> CheckRun {
    let started = std::time::Instant::now();
    let broken = |exit, timed_out| CheckRun {
        outcome: CheckOutcome::Broken,
        output: String::new(),
        exit,
        timed_out,
        elapsed: started.elapsed(),
        bound: timeout,
    };
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let name = format!("tqa-e2e-check-{}-{nanos}", std::process::id());
    let created = Command::new(program)
        .args(check_args(&name, image, check, copy))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;
    if !created.is_ok_and(|status| status.success()) {
        remove(program, &name).await;
        return broken(None, false);
    }
    let mut command = Command::new(program);
    command
        .args(["start", "--attach", &name])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let Ok(mut child) = command.spawn() else {
        remove(program, &name).await;
        return broken(None, false);
    };
    let Some(stdout) = child.stdout.take() else {
        remove(program, &name).await;
        return broken(None, false);
    };
    let mut bytes = Vec::new();
    let finished = tokio::time::timeout(timeout, async {
        // A check that writes more than the bound gets a closed pipe and is reported as broken.
        let read = stdout
            .take(CHECK_OUTPUT_BYTES)
            .read_to_end(&mut bytes)
            .await;
        (read, child.wait().await)
    })
    .await;
    let elapsed = started.elapsed();
    // `start --attach` returns the container's own exit code as soon as it stops.
    let (outcome, exit, timed_out) = if let Ok((Ok(_), Ok(status))) = finished {
        let outcome = match status.code() {
            Some(0) => CheckOutcome::Passed,
            Some(1) => CheckOutcome::Failed,
            _ => CheckOutcome::Broken,
        };
        (outcome, status.code(), false)
    } else {
        (CheckOutcome::Broken, None, finished.is_err())
    };
    // Removed after the verdict, stopped or not, and outside the check's bound.
    remove(program, &name).await;
    CheckRun {
        outcome,
        output: String::from_utf8_lossy(&bytes).into_owned(),
        exit,
        timed_out,
        elapsed,
        bound: timeout,
    }
}

/// Removes a check's container, whatever its state; a failure leaves it to the harness's label sweep.
async fn remove(program: &std::ffi::OsStr, name: &str) {
    let _removed = Command::new(program)
        .args(["rm", "-f", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;
}

/// One check run: the verdict, the start of the output, and the facts a broken verdict names.
#[derive(Clone, Debug)]
pub struct CheckRun {
    /// The verdict.
    pub outcome: CheckOutcome,
    /// The start of the check's output.
    pub output: String,
    /// The `docker run` exit status, when there was one.
    pub exit: Option<i32>,
    /// Whether the check's time bound ran out.
    pub timed_out: bool,
    /// How long the check took.
    pub elapsed: Duration,
    /// The check's time bound.
    pub bound: Duration,
}

impl CheckRun {
    /// For a broken verdict, its cause in words (the attempt's detail); `None` otherwise.
    #[must_use]
    pub fn broken_note(&self) -> Option<String> {
        if self.outcome != CheckOutcome::Broken {
            return None;
        }
        let cause = match (self.timed_out, self.exit) {
            (true, _) => "timed out".to_owned(),
            (false, Some(code)) => format!("docker exited {code}"),
            (false, None) => {
                "no exit status (the container did not start or was killed)".to_owned()
            }
        };
        Some(format!(
            "check broken: {cause} after {:.1} s (bound {} s)",
            self.elapsed.as_secs_f64(),
            self.bound.as_secs()
        ))
    }
    /// For a broken verdict, one JSON line appended to the check's saved output, so the file names its cause too.
    #[must_use]
    pub fn harness_line(&self) -> Option<String> {
        (self.outcome == CheckOutcome::Broken).then(|| {
            serde_json::json!({"harness": {
                "exit": self.exit,
                "timed_out": self.timed_out,
                "elapsed_ms": u64::try_from(self.elapsed.as_millis()).unwrap_or(u64::MAX),
                "bound_ms": u64::try_from(self.bound.as_millis()).unwrap_or(u64::MAX),
            }})
            .to_string()
        })
    }
}

/// Every container the harness starts: removed on exit, without network, and labelled so a run's own containers can be
/// counted (TK-I-006 samples that at most two seed or check containers run at once).
fn run_args(user: &str) -> Vec<OsString> {
    [
        "run",
        "--rm",
        "--network",
        "none",
        "--user",
        user,
        "--label",
        "tqa.e2e.container=1",
    ]
    .map(OsString::from)
    .to_vec()
}

fn mount(source: &Path, target: &str, read_only: bool) -> [OsString; 2] {
    let mut spec = OsString::from("type=bind,source=");
    spec.push(source);
    spec.push(format!(",target={target}"));
    if read_only {
        spec.push(",readonly");
    }
    ["--mount".into(), spec]
}

#[cfg(test)]
mod tests;
