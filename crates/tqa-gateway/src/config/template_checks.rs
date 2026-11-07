//! `[template_checks]`: the bounds of the credential-file scan a template check runs over the template's own layers
//! (ADR-0032). File-only: past either bound a template is refused (`image_scan_limit`), never counted clean, and the
//! way to admit a larger one is raising these here, explicitly. The defaults are 4x the Workstation template's own
//! layers or more.
use serde::Deserialize;
use tqa_sandbox::template::secret_files::{Bounds, DEFAULT_MAX_BYTES, DEFAULT_MAX_ENTRIES};

use crate::GatewayError;

const MIB: u64 = 1024 * 1024;

#[derive(Clone, Copy, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct TemplateChecksInput {
    /// Tar entries across the template's own layers.
    max_scan_entries: u64,
    /// Regular-file content across the template's own layers, in bytes.
    max_scan_bytes: u64,
}

impl Default for TemplateChecksInput {
    fn default() -> Self {
        Self {
            max_scan_entries: DEFAULT_MAX_ENTRIES,
            max_scan_bytes: DEFAULT_MAX_BYTES,
        }
    }
}

/// # Errors
/// Rejects an entry bound outside 1000..=100,000,000 and a content bound outside 1 MiB..=1 TiB.
pub(super) fn template_checks(input: TemplateChecksInput) -> Result<Bounds, GatewayError> {
    if !(1000..=100_000_000).contains(&input.max_scan_entries) {
        return Err(GatewayError::Configuration(
            "template_checks.max_scan_entries must be 1000 through 100000000",
        ));
    }
    if !(MIB..=1024 * 1024 * MIB).contains(&input.max_scan_bytes) {
        return Err(GatewayError::Configuration(
            "template_checks.max_scan_bytes must be 1 MiB through 1 TiB",
        ));
    }
    Ok(Bounds {
        max_entries: input.max_scan_entries,
        max_bytes: input.max_scan_bytes,
    })
}

#[cfg(test)]
#[path = "template_checks_tests.rs"]
mod tests;
