//! The workspace service (ADR-0030, RT-I-025): the gateway's list, read and stat requests about the workspace this
//! sandbox mounts, answered over the link. Paths go through the shared no-follow walk (`tqa-tools` `workspace_tree`),
//! so the answers equal the gateway's view of a Docker workspace and nothing outside the workspace is ever opened.
//! Each request runs on the blocking pool, a few at a time; a cancelled one answers with exactly one `cancelled` error.
//! Uploads write through `upload`; manifests and blobs (run changes) through `changes`.
use std::{
    collections::HashMap,
    io::{Read as _, Seek as _},
    os::fd::AsFd,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use base64::Engine as _;
use tokio::sync::{Semaphore, mpsc};
use tokio_util::sync::CancellationToken;
use tqa_tools::workspace_tree::{
    TreeError,
    path::{self as tree, Entry, Kind, WorkspacePath},
};
use tqa_types::{
    EntryKind, ListItem, ServerFrame, StatItem, WORKSPACE_PART_BYTES, WorkspaceErrorCode,
    WorkspaceOp, WorkspaceReply, WorkspaceResult,
};

use crate::link::Control;

mod changes;
#[cfg(test)]
mod tests;
mod upload;

pub(crate) use changes::Memory;

/// The directory mkfs.ext4 makes at the root of a workspace image.
const LOST_AND_FOUND: &str = "lost+found";
/// Requests worked on at once; more wait for a slot (the gateway bounds its side per link too).
const CONCURRENT: usize = 8;

/// The runtime's workspace service.
pub(crate) struct Service {
    root: PathBuf,
    controls: mpsc::Sender<Control>,
    running: Arc<Mutex<HashMap<u64, CancellationToken>>>,
    slots: Arc<Semaphore>,
    /// The last manifest's hashes, which `blobs` may serve.
    memory: Arc<Memory>,
}

impl Service {
    pub(crate) fn new(root: PathBuf, controls: mpsc::Sender<Control>) -> Self {
        Self {
            root,
            controls,
            running: Arc::new(Mutex::new(HashMap::new())),
            slots: Arc::new(Semaphore::new(CONCURRENT)),
            memory: Arc::new(Memory::default()),
        }
    }

    /// Answers one request in the background.
    pub(crate) fn request(&self, request_id: u64, op: WorkspaceOp) {
        let (root, memory) = (self.root.clone(), self.memory.clone());
        self.spawn(request_id, move || answer_with(&root, op, &memory));
    }

    /// Stops a running request; it answers with one `cancelled` error. An id that is not running is ignored.
    pub(crate) fn cancel(&self, request_id: u64) {
        if let Ok(running) = self.running.lock()
            && let Some(token) = running.get(&request_id)
        {
            token.cancel();
        }
    }

    fn spawn(&self, request_id: u64, work: impl FnOnce() -> Vec<WorkspaceResult> + Send + 'static) {
        let token = CancellationToken::new();
        if let Ok(mut running) = self.running.lock() {
            running.insert(request_id, token.clone());
        }
        let (controls, running, slots) = (
            self.controls.clone(),
            self.running.clone(),
            self.slots.clone(),
        );
        tokio::spawn(async move {
            let results = tokio::select! {
                results = work_in_slot(&slots, work) => results,
                () = token.cancelled() => vec![error(WorkspaceErrorCode::Cancelled, "cancelled by the gateway")],
            };
            if let Ok(mut running) = running.lock() {
                running.remove(&request_id);
            }
            send(&controls, request_id, results).await;
        });
    }
}

/// Runs the work on the blocking pool once a slot is free.
async fn work_in_slot(
    slots: &Arc<Semaphore>,
    work: impl FnOnce() -> Vec<WorkspaceResult> + Send + 'static,
) -> Vec<WorkspaceResult> {
    let _slot = slots.clone().acquire_owned().await;
    tokio::task::spawn_blocking(work)
        .await
        .unwrap_or_else(|_| vec![error(WorkspaceErrorCode::Internal, "the request failed")])
}

/// Sends the results as the request's parts, in order, the last one marked; stops when the link task is gone.
async fn send(controls: &mpsc::Sender<Control>, request_id: u64, results: Vec<WorkspaceResult>) {
    let count = results.len();
    for (part, result) in results.into_iter().enumerate() {
        let reply = WorkspaceReply {
            request_id,
            part: u32::try_from(part).unwrap_or(u32::MAX),
            last: part + 1 == count,
            result,
        };
        let frame = ServerFrame::WorkspaceReply(reply);
        if controls.send(Control { frame, sent: None }).await.is_err() {
            return;
        }
    }
}

fn error(code: WorkspaceErrorCode, message: &str) -> WorkspaceResult {
    WorkspaceResult::Error {
        code,
        message: message.to_owned(),
        retryable: false,
    }
}

fn failed(error: TreeError) -> WorkspaceResult {
    match error {
        TreeError::PathInvalid => {
            self::error(WorkspaceErrorCode::InvalidPath, "the path is invalid")
        }
        TreeError::NotFound => self::error(WorkspaceErrorCode::NotFound, "the path does not exist"),
        TreeError::Io => self::error(WorkspaceErrorCode::Internal, "workspace I/O failed"),
    }
}

/// The answer to one request, as its parts' results.
/// The answer to one request with a fresh memory (the unit tests' entry).
#[cfg(test)]
fn answer(root: &Path, op: WorkspaceOp) -> Vec<WorkspaceResult> {
    answer_with(root, op, &Memory::default())
}

/// The answer to one request, as its parts' results; `memory` holds the last manifest's hashes.
fn answer_with(root: &Path, op: WorkspaceOp, memory: &Memory) -> Vec<WorkspaceResult> {
    let answered = match op {
        WorkspaceOp::List { path, after, limit } => list(
            root,
            &path,
            after.as_deref(),
            usize::try_from(limit).unwrap_or(usize::MAX),
        )
        .map(|(items, next_after)| parts(items, next_after)),
        WorkspaceOp::Read {
            path,
            offset,
            max_bytes,
        } => read(root, &path, offset, max_bytes).map(|content| vec![content]),
        WorkspaceOp::Upload {
            upload_id,
            name,
            offset,
            data,
            last,
        } => {
            let chunk = upload::Chunk {
                upload_id,
                name: &name,
                offset,
                data: &data,
                last,
            };
            Ok(vec![upload::upload(root, &chunk)])
        }
        WorkspaceOp::Stat { paths } => Ok(vec![WorkspaceResult::Stats {
            items: paths.iter().map(|path| stat(root, path)).collect(),
        }]),
        WorkspaceOp::Manifest {
            max_files,
            max_entries,
            max_text_bytes,
        } => changes::manifest(root, (max_files, max_entries, max_text_bytes), memory),
        WorkspaceOp::Blobs { hashes } => Ok(changes::blobs(root, &hashes, memory)),
        _ => {
            return vec![error(
                WorkspaceErrorCode::Internal,
                "not supported by this runtime yet",
            )];
        }
    };
    answered.unwrap_or_else(|error| vec![failed(error)])
}

const fn entry_kind(kind: Kind) -> EntryKind {
    match kind {
        Kind::File => EntryKind::File,
        Kind::Dir => EntryKind::Dir,
        Kind::Link => EntryKind::Link,
        Kind::Other => EntryKind::Other,
    }
}

/// One page of a directory, as the gateway lists a Docker workspace: directories first, then names; `.tqa/` left out
/// at the root; `after` names the entry to continue after (none found: an empty page).
fn list(
    root: &Path,
    path: &str,
    after: Option<&str>,
    limit: usize,
) -> Result<(Vec<ListItem>, Option<String>), TreeError> {
    let target = WorkspacePath::parse(path)?;
    let opened = tree::open_root(root)?;
    let directory = tree::open_dir(opened.as_fd(), &target)?;
    let mut entries = tree::list(directory.as_fd())?;
    tree::hide_private(&mut entries, target.is_root());
    leave_out(&mut entries, target.is_root(), |entry| {
        filesystems_own(entry, || owners(root))
    });
    entries.sort_by(|a, b| (a.kind != Kind::Dir, &a.name).cmp(&(b.kind != Kind::Dir, &b.name)));
    Ok(page(&entries, after, limit))
}

/// The workspace image's own `lost+found` (mkfs.ext4 makes one, owned by root, at the root of every image): a
/// directory of that name that the workspace root's owner does not own. One the user made is theirs, and stays.
fn filesystems_own(entry: &Entry, owners: impl FnOnce() -> Option<(u32, u32)>) -> bool {
    entry.kind == Kind::Dir
        && entry.name == LOST_AND_FOUND
        && owners().is_some_and(|(root, found)| root != found)
}

/// The owners of the workspace root and of its `lost+found`, when both exist.
fn owners(root: &Path) -> Option<(u32, u32)> {
    use std::os::unix::fs::MetadataExt as _;
    let owner = std::fs::symlink_metadata(root).ok()?.uid();
    let found = std::fs::symlink_metadata(root.join(LOST_AND_FOUND))
        .ok()?
        .uid();
    Some((owner, found))
}

/// Leaves out, at the workspace root only, the entries `own` says belong to the filesystem, not the workspace: a
/// Docker workspace has none of them, so both backends list the same names (E2E-VM-013).
fn leave_out(entries: &mut Vec<Entry>, at_root: bool, own: impl Fn(&Entry) -> bool) {
    if at_root {
        entries.retain(|entry| !own(entry));
    }
}

/// One page of sorted entries after the one named `after` (none found: an empty page), and the name to continue
/// after when more follow.
fn page(entries: &[Entry], after: Option<&str>, limit: usize) -> (Vec<ListItem>, Option<String>) {
    let start = after.map_or(0, |after| {
        entries
            .iter()
            .position(|entry| entry.name == after)
            .map_or(entries.len(), |index| index + 1)
    });
    let page: Vec<ListItem> = entries
        .iter()
        .skip(start)
        .take(limit)
        .map(|entry| ListItem {
            name: entry.name.clone(),
            kind: entry_kind(entry.kind),
            size: entry.size,
            modified: entry.modified,
        })
        .collect();
    let next_after = (start + page.len() < entries.len())
        .then(|| page.last().map(|item| item.name.clone()))
        .flatten();
    (page, next_after)
}

/// Items cut into parts whose serialised array stays within [`WORKSPACE_PART_BYTES`]; a part is never empty unless it
/// is the only one.
fn cut<T: serde::Serialize>(items: Vec<T>) -> Vec<Vec<T>> {
    let mut cut: Vec<Vec<T>> = vec![Vec::new()];
    // The array's brackets, then each item and its comma.
    let mut used = 2;
    for item in items {
        let size = serde_json::to_vec(&item).map_or(WORKSPACE_PART_BYTES, |bytes| bytes.len()) + 1;
        if used + size > WORKSPACE_PART_BYTES && cut.last().is_some_and(|part| !part.is_empty()) {
            cut.push(Vec::new());
            used = 2;
        }
        used += size;
        if let Some(part) = cut.last_mut() {
            part.push(item);
        }
    }
    cut
}

/// A listing cut into parts (see [`cut`]); `next_after` goes on the last part.
fn parts(items: Vec<ListItem>, next_after: Option<String>) -> Vec<WorkspaceResult> {
    let cut = cut(items);
    let count = cut.len();
    let mut next_after = next_after;
    cut.into_iter()
        .enumerate()
        .map(|(index, items)| WorkspaceResult::Listing {
            items,
            next_after: if index + 1 == count {
                next_after.take()
            } else {
                None
            },
        })
        .collect()
}

/// At most `max_bytes` of a regular file from `offset`, never past the size it had when opened.
fn read(
    root: &Path,
    path: &str,
    offset: u64,
    max_bytes: u32,
) -> Result<WorkspaceResult, TreeError> {
    let target = WorkspacePath::parse(path)?;
    let root = tree::open_root(root)?;
    let (file, stat) = tree::open_file(root.as_fd(), &target)?;
    let size = u64::try_from(stat.st_size).unwrap_or(0);
    let want = size.saturating_sub(offset).min(u64::from(max_bytes));
    let mut data = Vec::with_capacity(usize::try_from(want).map_err(|_| TreeError::Io)?);
    let mut file = std::fs::File::from(file);
    file.seek(std::io::SeekFrom::Start(offset))
        .and_then(|_| file.take(want).read_to_end(&mut data))
        .map_err(|_| TreeError::Io)?;
    let end = offset.saturating_add(u64::try_from(data.len()).map_err(|_| TreeError::Io)?);
    Ok(WorkspaceResult::Content {
        size,
        modified: stat.st_mtime,
        inode: stat.st_ino,
        data: base64::engine::general_purpose::STANDARD.encode(&data),
        eof: end >= size,
    })
}

/// What one path is, without following it; anything that cannot be reached is absent, including a path below a link
/// (a link in a parent component is never walked through, so what lies beyond it does not exist for the workspace).
fn stat(root: &Path, path: &str) -> StatItem {
    let absent = StatItem {
        exists: false,
        kind: None,
        size: None,
    };
    let Ok(target) = WorkspacePath::parse(path) else {
        return absent;
    };
    let Ok(root) = tree::open_root(root) else {
        return absent;
    };
    let Some((parent, name)) = target.split_last() else {
        return StatItem {
            exists: true,
            kind: Some(EntryKind::Dir),
            size: Some(0),
        };
    };
    let Ok(entry) = tree::walk(root.as_fd(), parent, |_| ())
        .and_then(|directory| tree::entry(directory.as_fd(), name))
    else {
        return absent;
    };
    StatItem {
        exists: true,
        kind: Some(entry_kind(entry.kind)),
        size: Some(if entry.kind == Kind::File {
            entry.size
        } else {
            0
        }),
    }
}
