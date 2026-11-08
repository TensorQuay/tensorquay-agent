//! `[files]`: the workspace file view and upload limits and the run-changes snapshot bounds. Bytes and seconds in TOML.
use crate::GatewayError;
use serde::Deserialize;

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;

const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;
const GIB: u64 = 1024 * MIB;

/// Upload, preview and download bounds.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct FilesConfig {
    /// One uploaded file.
    pub upload_bytes: u64,
    /// Everything in one workspace's `uploads/`.
    pub uploads_bytes: u64,
    /// The inline text preview; larger files are returned truncated.
    pub preview_bytes: u64,
    /// One download.
    pub download_bytes: u64,
    /// An HTML page shown in the panel's sandboxed frame (`/files/view`); larger pages are only downloaded.
    pub preview_html_bytes: u64,
    /// A PDF the panel renders; larger ones are only downloaded.
    pub preview_pdf_bytes: u64,
    /// A spreadsheet the panel shows as a table (`/files/sheet`); larger ones are only downloaded.
    pub preview_sheet_bytes: u64,
    // Each preview limit is served no larger than `download_bytes` (see [`FilesConfig::preview`]): a preview never
    // shows more than a download would, and a small download limit needs no matching preview settings.
    /// How long one upload request may take (a slow network moving the largest file).
    pub upload_seconds: u64,
    /// Files recorded per snapshot; a larger workspace is recorded in part and marked truncated.
    pub snapshot_files: usize,
    /// Directory entries of any kind a snapshot may list: the bound on its work, whatever the workspace holds.
    pub snapshot_entries: usize,
    /// Text files up to this size are copied, so their changes can be shown as a diff.
    pub snapshot_text_bytes: u64,
    /// New copies per snapshot at most; unchanged files reuse earlier copies.
    pub snapshot_copy_bytes: u64,
    /// Snapshots kept per workspace; older ones and their unreferenced copies are removed.
    pub snapshots_kept: usize,
    /// How long one snapshot may take before delivery goes ahead without it.
    pub snapshot_seconds: u64,
    /// One unified diff's text at most; a longer diff is cut and marked truncated.
    pub diff_bytes: u64,
    /// Workspace requests waiting on one runtime link at once (microVM installs, ADR-0030); more answer `busy`.
    pub link_requests: usize,
}

impl Default for FilesConfig {
    fn default() -> Self {
        Self {
            upload_bytes: 25 * MIB,
            uploads_bytes: GIB,
            preview_bytes: 512 * KIB,
            download_bytes: 100 * MIB,
            preview_html_bytes: 2 * MIB,
            preview_pdf_bytes: 20 * MIB,
            preview_sheet_bytes: 10 * MIB,
            upload_seconds: 120,
            snapshot_files: 5000,
            snapshot_entries: 20_000,
            snapshot_text_bytes: 256 * KIB,
            snapshot_copy_bytes: 32 * MIB,
            snapshots_kept: 20,
            snapshot_seconds: 30,
            diff_bytes: 512 * KIB,
            link_requests: 8,
        }
    }
}

impl FilesConfig {
    /// A preview limit as served: never above the download limit.
    pub(crate) fn preview(&self, limit: u64) -> u64 {
        limit.min(self.download_bytes)
    }

    /// # Errors
    /// Rejects bounds outside their ranges or out of order (a file larger than the upload space, a preview larger
    /// than a download).
    pub(crate) fn validate(self) -> Result<Self, GatewayError> {
        let valid = (KIB..=GIB).contains(&self.upload_bytes)
            && (self.upload_bytes..=10 * GIB).contains(&self.uploads_bytes)
            && (KIB..=16 * MIB).contains(&self.preview_bytes)
            && (self.preview_bytes..=10 * GIB).contains(&self.download_bytes)
            && (KIB..=10 * GIB).contains(&self.preview_html_bytes)
            && (KIB..=10 * GIB).contains(&self.preview_pdf_bytes)
            && (KIB..=10 * GIB).contains(&self.preview_sheet_bytes)
            && (10..=3600).contains(&self.upload_seconds)
            && (1..=100_000).contains(&self.snapshot_files)
            && (100..=1_000_000).contains(&self.snapshot_entries)
            && (KIB..=16 * MIB).contains(&self.snapshot_text_bytes)
            && (self.snapshot_text_bytes..=GIB).contains(&self.snapshot_copy_bytes)
            && (2..=1000).contains(&self.snapshots_kept)
            && (1..=600).contains(&self.snapshot_seconds)
            && (KIB..=16 * MIB).contains(&self.diff_bytes)
            && (1..=64).contains(&self.link_requests);
        if valid {
            Ok(self)
        } else {
            Err(GatewayError::Configuration(
                "files limits must be ordered and within their documented ranges",
            ))
        }
    }
}
