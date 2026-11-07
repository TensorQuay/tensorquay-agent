//! Disposable tool workspaces with explicit outside and protected-path fixtures.
use std::{fs, path::Path};
use tempfile::TempDir;
use tqa_tools::{ToolError, WorkspaceRoot};

/// Owns both temporary directories until the fixture is dropped.
pub struct WorkspaceFixture {
    _directory: TempDir,
    outside: TempDir,
    workspace: WorkspaceRoot,
}
impl WorkspaceFixture {
    /// Borrows the canonical tool workspace.
    #[must_use]
    pub fn workspace(&self) -> &WorkspaceRoot {
        &self.workspace
    }
    /// Returns the canonical fixture workspace path.
    #[must_use]
    pub fn root(&self) -> &Path {
        self.workspace.path()
    }
    /// Returns the separate outside directory, used to prove confinement.
    #[must_use]
    pub fn outside(&self) -> &Path {
        self.outside.path()
    }
}
/// Creates text, binary, nested, protected and outside-symlink test cases.
///
/// # Errors
/// Returns a typed filesystem failure if fixture creation fails.
pub fn workspace_fixture() -> Result<WorkspaceFixture, ToolError> {
    let directory = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    let workspace = WorkspaceRoot::open(directory.path())?;
    fs::create_dir(directory.path().join("nested"))?;
    fs::create_dir(directory.path().join(".git"))?;
    for (path, bytes) in [
        ("hello.txt", b"alpha\nbeta\n".as_slice()),
        ("nested/child.txt", b"child\n"),
        (".git/config", b"fixture\n"),
        ("binary.bin", b"\0\x01\xff"),
    ] {
        fs::write(directory.path().join(path), bytes)?;
    }
    fs::write(outside.path().join("secret.txt"), "outside\n")?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.path(), directory.path().join("escape"))?;
    Ok(WorkspaceFixture {
        _directory: directory,
        outside,
        workspace,
    })
}
