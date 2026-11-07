//! Exclusive staging and installation on the workspace filesystem; never follows checkout symlinks.
use std::{fs, io, path::PathBuf};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub(super) struct Locations {
    pub workspace: PathBuf,
    pub state: PathBuf,
}
impl Locations {
    fn marker(&self) -> PathBuf {
        self.state.join("workspace-clone-attempted")
    }
    fn staging(&self) -> PathBuf {
        self.workspace.join(".tqa-clone-stage")
    }
    pub fn checkout(&self) -> PathBuf {
        self.staging().join("checkout")
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Initial {
    Empty,
    Existing,
    Attempted,
}

pub(super) fn inspect(paths: &Locations) -> io::Result<Initial> {
    match fs::symlink_metadata(paths.marker()) {
        Ok(_) => return Ok(Initial::Attempted),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(
        if fs::read_dir(&paths.workspace)?
            .next()
            .transpose()?
            .is_some()
        {
            Initial::Existing
        } else {
            Initial::Empty
        },
    )
}
pub(super) fn begin(paths: &Locations, cancel: &CancellationToken) -> io::Result<PathBuf> {
    check(cancel)?;
    if inspect(paths)? != Initial::Empty {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "workspace initialization is not empty",
        ));
    }
    fs::File::create_new(paths.marker())?.sync_all()?;
    fs::File::open(&paths.state)?.sync_all()?;
    fs::create_dir(paths.staging())?;
    Ok(paths.checkout())
}
pub(super) fn install(paths: &Locations, cancel: &CancellationToken) -> io::Result<()> {
    check(cancel)?;
    for entry in fs::read_dir(&paths.workspace)? {
        if entry?.path() != paths.staging() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "workspace changed during clone",
            ));
        }
    }
    let mut pending = vec![(fs::read_dir(paths.checkout())?, paths.workspace.clone())];
    while let Some((entries, destination)) = pending.last_mut() {
        check(cancel)?;
        let Some(entry) = entries.next() else {
            fs::File::open(destination)?.sync_all()?;
            pending.pop();
            continue;
        };
        let entry = entry?;
        let target = destination.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            fs::create_dir(&target)?;
            if pending.len() >= tqa_tools::MAX_PATH_BYTES {
                return Err(io::Error::other("checkout nesting exceeds the path bound"));
            }
            pending.push((fs::read_dir(entry.path())?, target));
        } else if kind.is_symlink() {
            std::os::unix::fs::symlink(fs::read_link(entry.path())?, target)?;
        } else if kind.is_file() {
            fs::hard_link(entry.path(), &target)?;
            fs::File::open(target)?.sync_all()?;
        } else {
            return Err(io::Error::other(
                "checkout contains an unsupported file type",
            ));
        }
    }
    fs::File::open(&paths.workspace)?.sync_all()?;
    fs::remove_dir_all(paths.staging())?;
    fs::File::open(&paths.workspace)?.sync_all()
}
fn check(cancel: &CancellationToken) -> io::Result<()> {
    if cancel.is_cancelled() {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "checkout interrupted",
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
