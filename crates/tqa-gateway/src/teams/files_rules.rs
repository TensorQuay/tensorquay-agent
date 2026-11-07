//! The rules of handing files back (ADR-0034 point 6), apart from their I/O: which of a report's files a take places,
//! and the name each goes by in the lead's `uploads/`. The 100 % class (ENGINEERING-GUARDRAILS §2.2): every branch is
//! unit-tested here, and the I/O around them in `files` keeps its integration tests (GW-I-110).
use tqa_types::HandedFile;

use crate::GatewayError;

/// The indexes of the report's files to place: all of them, or each named one by its path in the report.
///
/// # Errors
/// `404 not_found` naming a file not in the report, or the first one wanted once the member's task is deleted
/// (`present` false): a deleted member's files can't be taken.
pub(super) fn wanted(
    files: &[HandedFile],
    present: bool,
    names: Option<&[String]>,
) -> Result<Vec<usize>, GatewayError> {
    let wanted: Vec<usize> = match names {
        None => (0..files.len()).collect(),
        Some(names) => names
            .iter()
            .map(|name| {
                files
                    .iter()
                    .position(|file| &file.name == name)
                    .ok_or_else(|| GatewayError::NotFound.for_file(name))
            })
            .collect::<Result<_, _>>()?,
    };
    if !present && let Some(file) = wanted.first().and_then(|first| files.get(*first)) {
        return Err(GatewayError::NotFound.for_file(&file.name));
    }
    Ok(wanted)
}

/// The name a handed-back file goes by in the lead's `uploads/`, before the upload's own sanitising and next free
/// name: `<member>-<path>`, every `/` a `-`.
pub(super) fn placed_name(member: &str, path: &str) -> String {
    format!("{member}-{}", path.replace('/', "-"))
}

#[cfg(test)]
#[path = "files_rules_tests.rs"]
mod tests;
