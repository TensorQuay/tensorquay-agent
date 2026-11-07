//! Upload references attached to a user message (PROTO-U-007).

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, generate::SchemaGenerator, json_schema};

use crate::MAX_ATTACHMENTS;

/// The directory, relative to the workspace root, that holds uploads.
const UPLOADS_PREFIX: &str = "uploads/";
/// Upload names hold 1 through 128 bytes (Attachment contract, item 1).
const MAX_NAME_BYTES: usize = 128;
/// The same rule as [`valid_path`], for schema consumers. `$` without the multiline flag
/// ends the input in ECMAScript and in the Rust validator, so a trailing newline is refused.
const PATH_PATTERN: &str = "^uploads/[A-Za-z0-9_-][A-Za-z0-9._-]{0,127}$";

/// An attachment list breaks the shared upload-reference rule.
///
/// Variants name the failing item by position, never by value, so an error message can
/// not carry text from the rejected reference.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum InvalidAttachments {
    /// The list holds more than [`MAX_ATTACHMENTS`] references.
    #[error("expected at most 32 upload paths")]
    TooMany,
    /// The item is not `uploads/<name>` with an allowed name.
    #[error("item {index} is not an upload path uploads/<name>")]
    Path {
        /// Zero-based position of the item in the list.
        index: usize,
    },
    /// The item repeats an earlier reference.
    #[error("item {index} repeats an earlier upload path")]
    Duplicate {
        /// Zero-based position of the repeated item.
        index: usize,
    },
}

/// Checks a message's attachment list: at most [`MAX_ATTACHMENTS`] distinct references,
/// each `uploads/<name>` where the name is 1-128 bytes of `A-Z a-z 0-9 . _ -` and does not
/// start with a dot. Order is not changed or checked.
///
/// # Errors
/// Returns the first failing rule: the count, then each item in order.
pub fn validate_attachments(paths: &[String]) -> Result<(), InvalidAttachments> {
    if paths.len() > MAX_ATTACHMENTS {
        return Err(InvalidAttachments::TooMany);
    }
    for (index, path) in paths.iter().enumerate() {
        if !valid_path(path) {
            return Err(InvalidAttachments::Path { index });
        }
        if paths[..index].contains(path) {
            return Err(InvalidAttachments::Duplicate { index });
        }
    }
    Ok(())
}

fn valid_path(path: &str) -> bool {
    let Some(name) = path.strip_prefix(UPLOADS_PREFIX) else {
        return false;
    };
    let Some(first) = name.bytes().next() else {
        return false;
    };
    name.len() <= MAX_NAME_BYTES
        && first != b'.'
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// Schema-only type that names the shared `AttachmentPath` definition.
pub(crate) struct AttachmentPath;

impl JsonSchema for AttachmentPath {
    fn schema_name() -> Cow<'static, str> {
        "AttachmentPath".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "maxLength": UPLOADS_PREFIX.len() + MAX_NAME_BYTES,
            "pattern": PATH_PATTERN,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::valid_path;

    #[test]
    fn name_bounds_are_inclusive() {
        assert!(valid_path(&format!("uploads/{}", "a".repeat(128))));
        assert!(!valid_path(&format!("uploads/{}", "a".repeat(129))));
        assert!(!valid_path("uploads/"));
        assert!(!valid_path("uploads/.a"));
        assert!(valid_path("uploads/a."));
    }
}
