//! The credential-file scan of a template image through the engine (ADR-0032): the engine's export streamed into
//! [`super::secret_files::export_hits`] on a blocking thread, under its own time bound. An engine failure or the bound
//! is `Unavailable` whatever the scan saw, since a cut stream can look like a short one.
use std::{collections::BTreeMap, time::Duration};

use futures::StreamExt;
use tokio::sync::mpsc;

use super::{
    ImageSource,
    runtime_binary::{CHUNKS_IN_FLIGHT, ChannelReader},
    secret_files::{Bound, Bounds, ExportError, Rule, ScanError, export_hits},
};

/// The scan's own time bound: 14 times its measured wall time on the Workstation image (4.3 s on 27 Sep 2026).
pub const SCAN_TIMEOUT: Duration = Duration::from_mins(1);

/// Why a scan has no hits to report. Neither is ever a clean result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ScanFailure {
    /// An own layer is past this bound (`image_scan_limit`).
    Limit(Bound),
    /// Nothing is known: the engine failed, the bound passed, or an own layer could not be read.
    Unavailable,
}

/// Every credential file in `image`'s `own_layers` (its layer IDs beyond its base's, in order), with its rule.
pub(super) async fn scan_image(
    source: &dyn ImageSource,
    image: &str,
    own_layers: Vec<String>,
    bounds: Bounds,
    bound: Duration,
) -> Result<BTreeMap<String, Rule>, ScanFailure> {
    if own_layers.is_empty() {
        return Ok(BTreeMap::new());
    }
    let (sender, receiver) = mpsc::channel::<Vec<u8>>(CHUNKS_IN_FLIGHT);
    let reader = tokio::task::spawn_blocking(move || {
        export_hits(ChannelReader::new(receiver), &own_layers, bounds)
    });
    let pump = async move {
        let mut stream = source.export(image);
        while let Some(chunk) = stream.next().await {
            let Ok(chunk) = chunk else { return false };
            // The reader stopped early (a limit): nothing more to pull.
            if sender.send(chunk).await.is_err() {
                break;
            }
        }
        true
    };
    // A timeout drops the pump and its sender, so the reader sees the end and returns.
    let streamed = tokio::time::timeout(bound, pump).await.unwrap_or(false);
    let scanned = reader.await.map_err(|_| ScanFailure::Unavailable)?;
    match scanned {
        Err(ExportError::Scan(ScanError::Limit(bound))) => Err(ScanFailure::Limit(bound)),
        _ if !streamed => Err(ScanFailure::Unavailable),
        Ok(hits) => Ok(hits),
        Err(ExportError::Scan(ScanError::Malformed) | ExportError::Unavailable) => {
            Err(ScanFailure::Unavailable)
        }
    }
}
