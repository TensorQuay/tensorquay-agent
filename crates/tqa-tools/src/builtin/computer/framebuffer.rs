//! The screen as Xvfb keeps it (`-fbdir`, ADR-0039 section 10): an XWD file the runtime reads directly, for the settle
//! rule's "did anything change" in any application (and slice 3's screenshots), with no tool in the image.
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

use super::{driver::DriverError, raster::Raster};

/// Where the Desktop template's Xvfb keeps screen 0.
pub const SCREEN_FILE: &str = "/run/tqa-display/Xvfb_screen0";
/// The largest file read: 1280×800 at 32 bits is about 4.1 MB, and the mount is 8 MiB.
const MAX_BYTES: u64 = 8 * 1024 * 1024;

/// The screen file, read when asked.
#[derive(Clone, Debug)]
pub struct Framebuffer {
    path: PathBuf,
}

impl Framebuffer {
    /// The screen file at `path` ([`SCREEN_FILE`] in a sandbox).
    #[must_use]
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_owned(),
        }
    }

    /// The screen's pixels.
    ///
    /// # Errors
    /// `Unavailable` when the file isn't there, is too large, or isn't an image Xvfb writes.
    pub async fn raster(&self) -> Result<Raster, DriverError> {
        let unavailable =
            |reason: &str| DriverError::Unavailable(format!("the screen file {reason}"));
        let size = tokio::fs::metadata(&self.path)
            .await
            .map_err(|_| unavailable("isn't there"))?
            .len();
        if size > MAX_BYTES {
            return Err(unavailable("is too large"));
        }
        let bytes = tokio::fs::read(&self.path)
            .await
            .map_err(|_| unavailable("can't be read"))?;
        Raster::from_xwd(&bytes).map_err(|error| DriverError::Unavailable(error.to_string()))
    }

    /// A digest of everything the screen shows, or `None` when the file isn't there or is too large (the settle rule
    /// then falls back to the window's title).
    pub async fn digest(&self) -> Option<u64> {
        let size = tokio::fs::metadata(&self.path).await.ok()?.len();
        if size > MAX_BYTES {
            return None;
        }
        let bytes = tokio::fs::read(&self.path).await.ok()?;
        let mut hasher = DefaultHasher::new();
        bytes.hash(&mut hasher);
        Some(hasher.finish())
    }
}
