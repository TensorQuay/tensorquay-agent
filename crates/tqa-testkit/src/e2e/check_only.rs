//! `tqa-e2e --check-only` (E2E-CMP-003): one task's check on a workspace another tool left, run the way an attempt's
//! check runs, so a comparison scores both sides with the same check. The workspace is another program's output, so it
//! is copied first and never changed, and the copy holds only directories and regular files: symbolic links and
//! anything else are dropped and counted, as the files API's download (an attempt's copy) never makes a link. On the
//! copy, a changed protected path is `tampered`; otherwise the seed's protected paths are restored, the check runs in
//! its image, and a broken check names its cause as an attempt's does. `--seed-only` gives the other tool the same
//! starting point: the task's seed committed as an attempt's baseline, by the driver's own seeding, then handed back to
//! the host user.
use super::{
    classify::CheckOutcome,
    docker,
    driver::{self, DriverError, Images},
    manifest::Manifest,
    workspace,
};
use std::{
    io,
    path::{Path, PathBuf},
    time::Duration,
};

/// The flags `--check-only` takes, all of them required except the Workstation image.
#[derive(Clone, Debug, Default)]
pub struct CheckOnly {
    /// The directory holding one directory per task.
    pub tasks: PathBuf,
    /// The task's id.
    pub task: String,
    /// The workspace to check; it is copied, never changed.
    pub workspace: PathBuf,
    /// The record directory; it must not exist yet.
    pub out: PathBuf,
    /// The runtime image checks run in.
    pub runtime_image: String,
    /// The Workstation image, for tasks whose check needs it.
    pub workstation_image: Option<String>,
}

/// The check's verdict, and for a broken check the sentence that names its cause.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Verdict {
    /// The verdict.
    pub outcome: CheckOutcome,
    /// Why a broken check broke.
    pub note: Option<String>,
}

impl CheckOnly {
    /// Parses the flags after `--check-only`; every flag takes one value, and unknown flags are refused.
    ///
    /// # Errors
    /// [`DriverError::Usage`] naming the problem.
    pub fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self, DriverError> {
        let mut options = Self::default();
        let mut arguments = arguments.into_iter();
        while let Some(flag) = arguments.next() {
            let value = arguments
                .next()
                .ok_or_else(|| DriverError::Usage(format!("{flag} needs a value")))?;
            match flag.as_str() {
                "--tasks" => options.tasks = value.into(),
                "--task" => options.task = value,
                "--workspace" => options.workspace = value.into(),
                "--out" => options.out = value.into(),
                "--runtime-image" => options.runtime_image = value,
                "--workstation-image" => options.workstation_image = Some(value),
                _ => {
                    return Err(DriverError::Usage(format!(
                        "--check-only does not take {flag}"
                    )));
                }
            }
        }
        for (missing, flag) in [
            (options.tasks.as_os_str().is_empty(), "--tasks"),
            (options.task.is_empty(), "--task"),
            (options.workspace.as_os_str().is_empty(), "--workspace"),
            (options.out.as_os_str().is_empty(), "--out"),
            (options.runtime_image.is_empty(), "--runtime-image"),
        ] {
            if missing {
                return Err(DriverError::Usage(format!("--check-only needs {flag}")));
            }
        }
        Ok(options)
    }
}

/// Copies the workspace, then checks the copy and writes `check.txt` and `check.json` to the record directory.
///
/// # Errors
/// An existing record directory, an unknown task, or a local or Docker failure outside the check itself.
pub async fn run(options: &CheckOnly) -> Result<Verdict, DriverError> {
    let tasks = driver::load_tasks(&options.tasks, std::slice::from_ref(&options.task))?;
    let (directory, manifest) = tasks
        .first()
        .ok_or_else(|| DriverError::Usage(format!("no task named {}", options.task)))?;
    if options.out.exists() {
        return Err(DriverError::Usage(format!(
            "{} already exists; a record is never overwritten",
            options.out.display()
        )));
    }
    std::fs::create_dir_all(&options.out)?;
    // Docker bind mounts need an absolute source.
    let copy = std::path::absolute(options.out.join("work"))?;
    let dropped = copy_workspace(&options.workspace, &copy)?;
    let images = Images {
        runtime: options.runtime_image.clone(),
        workstation: options.workstation_image.clone(),
    };
    let (outcome, note) = check_copy(
        directory,
        manifest,
        &images,
        &copy,
        &options.out.join("check.txt"),
    )
    .await?;
    let record = serde_json::json!({"task": manifest.id, "outcome": outcome, "note": note, "dropped": dropped});
    std::fs::write(options.out.join("check.json"), format!("{record}\n"))?;
    Ok(Verdict { outcome, note })
}

/// An attempt's check on a copy of its workspace: `tampered` when a protected path differs from the seed; otherwise the
/// seed's protected paths are restored and the check runs, its output saved to `saved` with a broken check's harness
/// line at the end.
///
/// # Errors
/// A local or Docker failure outside the check itself.
pub async fn check_copy(
    directory: &Path,
    manifest: &Manifest,
    images: &Images,
    copy: &Path,
    saved: &Path,
) -> Result<(CheckOutcome, Option<String>), DriverError> {
    let seed = directory.join("seed");
    if workspace::tampered(&seed, copy, &manifest.protected)? {
        return Ok((CheckOutcome::Tampered, None));
    }
    workspace::restore(&seed, copy, &manifest.protected)?;
    workspace::share(copy)?;
    let timeout = Duration::from_secs(manifest.check.timeout_secs.into());
    let image = images.for_check(manifest)?;
    let run = docker::check(image, &directory.join("check"), copy, timeout).await;
    docker::reclaim(image, copy).await?;
    let text = match run.harness_line() {
        Some(line) => {
            let separator = if run.output.ends_with('\n') || run.output.is_empty() {
                ""
            } else {
                "\n"
            };
            format!("{}{separator}{line}\n", run.output)
        }
        None => run.output.clone(),
    };
    std::fs::write(saved, text)?;
    Ok((run.outcome, run.broken_note()))
}

/// Copies a workspace another program wrote: directories and regular files only. Symbolic links, sockets, pipes and
/// devices are dropped and counted; a link is never read through.
///
/// # Errors
/// A local I/O failure.
pub fn copy_workspace(from: &Path, to: &Path) -> io::Result<u64> {
    std::fs::create_dir_all(to)?;
    let mut dropped = 0;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        // The entry's own type: `DirEntry::file_type` does not follow a link.
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            dropped += copy_workspace(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &target)?;
        } else {
            dropped += 1;
        }
    }
    Ok(dropped)
}

/// The flags `--seed-only` takes, all required.
#[derive(Clone, Debug, Default)]
pub struct SeedOnly {
    /// The directory holding one directory per task.
    pub tasks: PathBuf,
    /// The task's id.
    pub task: String,
    /// The workspace to create; it must be absent or empty.
    pub workspace: PathBuf,
    /// The runtime image that seeds, as for an attempt.
    pub runtime_image: String,
}

impl SeedOnly {
    /// Parses the flags after `--seed-only`; every flag takes one value, and unknown flags are refused.
    ///
    /// # Errors
    /// [`DriverError::Usage`] naming the problem.
    pub fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self, DriverError> {
        let mut options = Self::default();
        let mut arguments = arguments.into_iter();
        while let Some(flag) = arguments.next() {
            let value = arguments
                .next()
                .ok_or_else(|| DriverError::Usage(format!("{flag} needs a value")))?;
            match flag.as_str() {
                "--tasks" => options.tasks = value.into(),
                "--task" => options.task = value,
                "--workspace" => options.workspace = value.into(),
                "--runtime-image" => options.runtime_image = value,
                _ => {
                    return Err(DriverError::Usage(format!(
                        "--seed-only does not take {flag}"
                    )));
                }
            }
        }
        for (missing, flag) in [
            (options.tasks.as_os_str().is_empty(), "--tasks"),
            (options.task.is_empty(), "--task"),
            (options.workspace.as_os_str().is_empty(), "--workspace"),
            (options.runtime_image.is_empty(), "--runtime-image"),
        ] {
            if missing {
                return Err(DriverError::Usage(format!("--seed-only needs {flag}")));
            }
        }
        Ok(options)
    }
}

/// Seeds the workspace as an attempt's is seeded (the seed, one commit with a fixed author and date), then hands it
/// back to the host user.
///
/// # Errors
/// A workspace that is not empty, an unknown task, or a local or Docker failure.
pub async fn seed_only(options: &SeedOnly) -> Result<(), DriverError> {
    let tasks = driver::load_tasks(&options.tasks, std::slice::from_ref(&options.task))?;
    let (directory, _) = tasks
        .first()
        .ok_or_else(|| DriverError::Usage(format!("no task named {}", options.task)))?;
    if options.workspace.exists() && std::fs::read_dir(&options.workspace)?.next().is_some() {
        return Err(DriverError::Usage(format!(
            "{} is not empty; a seed goes into an empty workspace",
            options.workspace.display()
        )));
    }
    std::fs::create_dir_all(&options.workspace)?;
    let workspace = std::path::absolute(&options.workspace)?;
    docker::seed(&options.runtime_image, &directory.join("seed"), &workspace).await?;
    docker::reclaim(&options.runtime_image, &workspace).await?;
    Ok(())
}

/// The workspace's baseline commit.
///
/// # Errors
/// Git failing or a missing commit.
pub fn seed_commit(workspace: &Path) -> io::Result<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(["-c", "safe.directory=*", "rev-parse", "HEAD"])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other("the workspace has no commit"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
