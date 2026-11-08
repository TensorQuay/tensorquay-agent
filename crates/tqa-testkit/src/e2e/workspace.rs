//! The workspace copy the check runs on: downloaded through the files API, never read
//! from the sandbox, and compared with the seed for protected paths before the check restores them.
use crate::client::{ApiClient, ClientError, FileEntry};
use std::{
    io,
    path::{Component, Path},
};
use tqa_types::ConversationId;

/// Files per copy; a task repository has at most a few hundred.
const MAX_FILES: usize = 5000;
/// Directory depth per copy.
const MAX_DEPTH: usize = 32;

/// Why a copy could not be made.
#[derive(Debug)]
pub enum CopyError {
    /// The files API failed.
    Client(ClientError),
    /// The workspace exceeded a bound or named an entry that is not one normal path component.
    Unsafe,
    /// Writing the local copy failed.
    Io(io::Error),
}
impl From<ClientError> for CopyError {
    fn from(error: ClientError) -> Self {
        Self::Client(error)
    }
}
impl From<io::Error> for CopyError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Copies the conversation's whole workspace into the empty directory `into`.
///
/// # Errors
/// Any [`CopyError`].
pub async fn download(
    client: &ApiClient,
    id: &ConversationId,
    into: &Path,
) -> Result<usize, CopyError> {
    let mut pending = vec![(String::new(), 0)];
    let mut files = 0;
    while let Some((directory, depth)) = pending.pop() {
        for entry in list(client, id, &directory).await? {
            if !single_component(&entry.name) || depth >= MAX_DEPTH {
                return Err(CopyError::Unsafe);
            }
            let path = if directory.is_empty() {
                entry.name
            } else {
                format!("{directory}/{}", entry.name)
            };
            match entry.kind.as_str() {
                "dir" => {
                    std::fs::create_dir_all(into.join(&path))?;
                    pending.push((path, depth + 1));
                }
                "file" if files < MAX_FILES => {
                    files += 1;
                    std::fs::write(into.join(&path), client.download(id, &path).await?)?;
                }
                _ => return Err(CopyError::Unsafe),
            }
        }
    }
    Ok(files)
}

/// Every entry of one directory, across pages, bounded like the whole copy.
async fn list(
    client: &ApiClient,
    id: &ConversationId,
    directory: &str,
) -> Result<Vec<FileEntry>, CopyError> {
    let mut entries = Vec::new();
    let mut after = None;
    loop {
        let page = client.files(id, directory, after.as_deref()).await?;
        entries.extend(page.items);
        if entries.len() > MAX_FILES {
            return Err(CopyError::Unsafe);
        }
        match page.next_after {
            Some(next) => after = Some(next),
            None => return Ok(entries),
        }
    }
}

/// Whether any protected path differs between the seed and the copy; a path absent from both is equal. A symbolic
/// link at or above a protected path in the copy counts as a change, and is never read through.
///
/// # Errors
/// An I/O failure other than a missing file.
pub fn tampered(seed: &Path, copy: &Path, protected: &[String]) -> io::Result<bool> {
    for path in protected {
        if linked(copy, path)?
            || read_optional(&seed.join(path))? != read_optional(&copy.join(path))?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Puts the seed's version of each protected path back into the copy, so the check runs the
/// pristine tests even when an unchanged copy was expected. It never writes through a symbolic link.
///
/// # Errors
/// An I/O failure, or a symbolic link at or above a protected path (which [`tampered`] reports first).
pub fn restore(seed: &Path, copy: &Path, protected: &[String]) -> io::Result<()> {
    for path in protected {
        if linked(copy, path)? {
            return Err(io::Error::other(format!(
                "a symbolic link at or above the protected path {path}"
            )));
        }
        let target = copy.join(path);
        match read_optional(&seed.join(path))? {
            Some(bytes) => {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&target, bytes)?;
            }
            None if target.exists() => std::fs::remove_file(&target)?,
            None => {}
        }
    }
    Ok(())
}

/// Opens the disposable copy to the check container's user, which differs from the host user under
/// rootless Docker; builds and test caches must be able to write into it.
///
/// # Errors
/// An I/O failure.
pub fn share(copy: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    // The entry's own metadata: a symbolic link is left alone, never followed to its target.
    let metadata = std::fs::symlink_metadata(copy)?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    let added = if metadata.is_dir() { 0o777 } else { 0o666 };
    let mode = metadata.permissions().mode() | added;
    std::fs::set_permissions(copy, std::fs::Permissions::from_mode(mode))?;
    if metadata.is_dir() {
        for entry in std::fs::read_dir(copy)? {
            share(&entry?.path())?;
        }
    }
    Ok(())
}

/// Whether any component of `path` below `copy`, the path itself included, is a symbolic link.
fn linked(copy: &Path, path: &str) -> io::Result<bool> {
    let mut current = copy.to_path_buf();
    for component in Path::new(path).components() {
        current.push(component);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Ok(true),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        }
    }
    Ok(false)
}

fn read_optional(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn single_component(name: &str) -> bool {
    let mut parts = Path::new(name).components();
    matches!(parts.next(), Some(Component::Normal(_))) && parts.next().is_none()
}

#[cfg(test)]
mod tests;
