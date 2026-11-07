//! The run's screenshots for the model (ADR-0039 section 3, slice 3). A `computer` screenshot's PNG is kept in the
//! runtime's own state directory, never the workspace, where the agent's tools or a helper's copy could read it; only
//! the latest [`SENT_SCREENSHOTS`] are kept, since only they are ever sent, and earlier runs' are removed when a new run starts. The
//! bytes are never journaled: the request reads them when it is built, and a screenshot without its file (an older
//! one, or one from a lost sandbox) goes to the model as a fixed line.
use std::path::{Path, PathBuf};

use tqa_types::{RunId, ToolCallId};

/// How many of a run's screenshots a request carries, its latest.
pub const SENT_SCREENSHOTS: usize = 3;

/// The run screenshots' home: `<state>/screenshots/<run>/<order>-<call>.png`, and beside a marked one its clean copy,
/// `<order>-<call>.plain.png`.
#[derive(Clone, Debug)]
pub struct Screenshots {
    root: PathBuf,
}

impl Screenshots {
    /// The store under `root` (the runtime's `<state>/screenshots`).
    #[must_use]
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_owned(),
        }
    }

    fn run_dir(&self, run: &RunId) -> PathBuf {
        self.root.join(run.as_str())
    }

    /// Keeps a call's screenshot (and its clean copy, when it is marked), then drops all but the run's latest
    /// [`SENT_SCREENSHOTS`].
    ///
    /// # Errors
    /// The file system's, when the screenshot couldn't be written.
    pub async fn keep(
        &self,
        run: &RunId,
        call: &ToolCallId,
        png: &[u8],
        plain: Option<&[u8]>,
    ) -> std::io::Result<()> {
        let dir = self.run_dir(run);
        tokio::fs::create_dir_all(&dir).await?;
        let mut kept = listed(&dir).await;
        let order = kept.last().map_or(0, |(order, _, _)| order + 1);
        let path = dir.join(format!("{order:06}-{}.png", call.as_str()));
        if let Some(plain) = plain {
            write_whole(&dir, &with_plain(&path), plain).await?;
        }
        write_whole(&dir, &path, png).await?;
        kept.push((order, call.as_str().to_owned(), path));
        let excess = kept.len().saturating_sub(SENT_SCREENSHOTS);
        for (_, _, old) in kept.into_iter().take(excess) {
            let _ = tokio::fs::remove_file(with_plain(&old)).await;
            let _ = tokio::fs::remove_file(old).await;
        }
        Ok(())
    }

    /// A call's screenshot while it is among the run's latest: as it was taken, or `plain`, its clean copy when it has
    /// one (an unmarked screenshot is its own clean copy).
    pub async fn read(&self, run: &RunId, call: &ToolCallId, plain: bool) -> Option<Vec<u8>> {
        let (_, _, path) = listed(&self.run_dir(run))
            .await
            .into_iter()
            .find(|(_, kept, _)| kept == call.as_str())?;
        if plain && let Ok(bytes) = tokio::fs::read(with_plain(&path)).await {
            return Some(bytes);
        }
        tokio::fs::read(path).await.ok()
    }

    /// Removes every run's screenshots but `keep`'s (a run that ended, or one a crash left behind).
    pub async fn forget_all_but(&self, keep: Option<&RunId>) {
        let Ok(mut entries) = tokio::fs::read_dir(&self.root).await else {
            return;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            if keep.is_some_and(|run| entry.file_name().to_str() == Some(run.as_str())) {
                continue;
            }
            let _ = tokio::fs::remove_dir_all(entry.path()).await;
        }
    }
}

/// A screenshot's clean copy beside it.
fn with_plain(path: &Path) -> PathBuf {
    path.with_extension("plain.png")
}

/// Writes a file whole: to a partial name first, then renamed, so a reader never sees half of it.
async fn write_whole(dir: &Path, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let partial = dir.join(format!(
        ".{}.partial",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("screenshot")
    ));
    let written = match tokio::fs::write(&partial, bytes).await {
        Ok(()) => tokio::fs::rename(&partial, path).await,
        Err(error) => Err(error),
    };
    // A failed write leaves no partial file behind to count against the state quota.
    if written.is_err() {
        let _ = tokio::fs::remove_file(&partial).await;
    }
    written
}

/// A run directory's screenshots in the order they were kept: (order, call, path); clean copies aren't listed.
async fn listed(dir: &Path) -> Vec<(u64, String, PathBuf)> {
    let mut found = Vec::new();
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return found;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name();
        let Some((order, call)) = name
            .to_str()
            .filter(|name| !name.ends_with(".plain.png"))
            .and_then(|name| name.strip_suffix(".png"))
            .and_then(|stem| stem.split_once('-'))
        else {
            continue;
        };
        if let Ok(order) = order.parse::<u64>() {
            found.push((order, call.to_owned(), entry.path()));
        }
    }
    found.sort_by_key(|(order, _, _)| *order);
    found
}
