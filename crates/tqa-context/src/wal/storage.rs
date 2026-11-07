//! Private files and staged rewrites, with a lock inode that is never replaced.
use super::WalError;
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use tqa_types::{Seq, ServerFrame};

pub(super) fn open(directory: &Path) -> Result<(File, File), WalError> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    builder.mode(0o700);
    builder.create(directory)?;
    #[cfg(unix)]
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    let lock = private_file(&directory.join(".lock"), false)?;
    lock.try_lock().map_err(|error| match error {
        TryLockError::WouldBlock => WalError::Locked,
        TryLockError::Error(error) => WalError::Io(error),
    })?;
    let file = private_file(&directory.join("events.jsonl"), false)?;
    Ok((lock, file))
}
fn private_file(path: &Path, exclusive: bool) -> Result<File, WalError> {
    if let Ok(metadata) = fs::symlink_metadata(path)
        && !metadata.is_file()
    {
        return Err(WalError::Corrupt);
    }
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(!exclusive)
        .create_new(exclusive);
    #[cfg(unix)]
    options.mode(0o600);
    let file = options.open(path)?;
    #[cfg(unix)]
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    Ok(file)
}
pub(super) fn stage(
    directory: &Path,
    records: &[(Seq, ServerFrame)],
) -> Result<(File, PathBuf), WalError> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = directory.join(format!(
        ".events-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = private_file(&path, true)?;
    let result = (|| {
        for (_, frame) in records {
            file.write_all(&super::record::encode(frame)?)?;
        }
        file.sync_data()?;
        Ok::<_, WalError>(())
    })();
    if result.is_err() {
        fs::remove_file(&path)?;
    }
    result?;
    Ok((file, path))
}
