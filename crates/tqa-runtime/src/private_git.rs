//! `.tqa/` holds a task's private state (spill files, history, the person's digests, notes): never a project's work.
//! A `git add -A` in the workspace must not stage it, whatever the project's own `.gitignore` says, so the runtime
//! keeps a `.tqa/.gitignore` of `*` there (decided on 21 Nov 2026). It ignores everything in the folder, itself
//! included, and changes no file of the project.
use std::{fs, io, path::Path};

/// The folder kept out, relative to the workspace root.
const PRIVATE: &str = ".tqa";
/// What `.tqa/.gitignore` says: everything here is ignored, this file too.
const EVERYTHING: &str = "*\n";

/// Makes sure `.tqa/` exists as a real folder holding a `.gitignore` of `*`. A link at `.tqa`, or a `.gitignore` there
/// that is not a regular file, is left alone (never followed, never replaced); an existing regular one is left as it
/// is. Called at each run's start; a failure is logged and the run goes on.
pub(crate) fn keep_private_out_of_git(workspace: &Path) {
    if let Err(error) = keep(workspace) {
        tracing::warn!(error = %error, "couldn't keep .tqa out of the workspace's git commits");
    }
}

fn keep(workspace: &Path) -> io::Result<()> {
    let private = workspace.join(PRIVATE);
    match fs::symlink_metadata(&private) {
        Ok(metadata) if !metadata.is_dir() => return Ok(()),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => fs::create_dir(&private)?,
        Err(error) => return Err(error),
    }
    let ignore = private.join(".gitignore");
    match fs::symlink_metadata(&ignore) {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            use std::io::Write as _;
            // `create_new` never follows or replaces whatever appears there in between.
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&ignore)?;
            file.write_all(EVERYTHING.as_bytes())
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::keep_private_out_of_git;
    use std::{error::Error, fs, process::Command};

    fn git(directory: &std::path::Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
        let output = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
            .args(args)
            .current_dir(directory)
            .output()?;
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(String::from_utf8(output.stdout)?)
    }

    // TEST: RT-I-065
    #[test]
    fn rt_i_065_git_add_all_never_stages_the_private_folder() -> Result<(), Box<dyn Error>> {
        let directory = tempfile::tempdir()?;
        let root = directory.path();
        git(root, &["init", "-q"])?;
        fs::write(root.join("main.py"), "print(1)\n")?;
        keep_private_out_of_git(root);
        // What a run leaves there: spill files, its history, a digest.
        fs::create_dir_all(root.join(".tqa/task/spill"))?;
        fs::write(root.join(".tqa/task/spill/1.txt"), "spilled")?;
        fs::write(root.join(".tqa/task/history.md"), "# History\n")?;
        git(root, &["add", "-A"])?;
        let staged = git(root, &["diff", "--cached", "--name-only"])?;
        assert_eq!(staged.lines().collect::<Vec<_>>(), ["main.py"], "{staged}");
        assert_eq!(fs::read_to_string(root.join(".tqa/.gitignore"))?, "*\n");
        Ok(())
    }

    // TEST: RT-I-065
    #[test]
    fn rt_i_065_an_existing_file_or_a_link_is_left_alone() -> Result<(), Box<dyn Error>> {
        let directory = tempfile::tempdir()?;
        let root = directory.path();
        fs::create_dir(root.join(".tqa"))?;
        fs::write(root.join(".tqa/.gitignore"), "spill/\n")?;
        keep_private_out_of_git(root);
        assert_eq!(
            fs::read_to_string(root.join(".tqa/.gitignore"))?,
            "spill/\n"
        );
        // A link at .tqa is never followed: nothing is written where it points.
        let other = tempfile::tempdir()?;
        let linked = tempfile::tempdir()?;
        std::os::unix::fs::symlink(other.path(), linked.path().join(".tqa"))?;
        keep_private_out_of_git(linked.path());
        assert!(!other.path().join(".gitignore").exists());
        Ok(())
    }
}
