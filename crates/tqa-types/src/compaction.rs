//! The bounded structured summary carried by a `compaction` event (ADR-0009).

use schemars::{JsonSchema, Schema, generate::SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

use crate::{
    COMPACTION_VERSION, MAX_COMPACTION_ITEM_CHARS, MAX_COMPACTION_ITEMS,
    MAX_COMPACTION_SUMMARY_BYTES,
};

/// Who produced a compaction summary.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompactionSource {
    /// The route's model, from the fixed summary prompt.
    Model,
    /// The deterministic summary built from events when the model summary was unusable.
    Fallback,
}

/// Fixed summary fields replacing older turns. Unknown fields are ignored and never kept.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CompactionSummary {
    /// The task being worked on.
    #[schemars(length(max = 512))]
    pub goal: String,
    /// Work already completed.
    #[schemars(length(max = 32), inner(length(max = 512)))]
    pub done: Vec<String>,
    /// Work still outstanding.
    #[schemars(length(max = 32), inner(length(max = 512)))]
    pub pending: Vec<String>,
    /// Decisions made and their reasons.
    #[schemars(length(max = 32), inner(length(max = 512)))]
    pub decisions: Vec<String>,
    /// Workspace paths that matter for the remaining work.
    #[schemars(length(max = 32), inner(length(max = 512)))]
    pub key_paths: Vec<String>,
    /// Errors that were not yet resolved.
    #[schemars(length(max = 32), inner(length(max = 512)))]
    pub open_errors: Vec<String>,
}

/// A summary breaks one of the compaction bounds; the field is named, never its text.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum InvalidCompactionSummary {
    /// A list holds more than [`MAX_COMPACTION_ITEMS`] items.
    #[error("{field}: expected at most 32 items")]
    TooManyItems {
        /// The summary field.
        field: &'static str,
    },
    /// The goal or an item exceeds [`MAX_COMPACTION_ITEM_CHARS`].
    #[error("{field}: expected at most 512 characters per item")]
    ItemTooLong {
        /// The summary field.
        field: &'static str,
    },
    /// All strings together exceed [`MAX_COMPACTION_SUMMARY_BYTES`].
    #[error("summary: expected at most 16384 UTF-8 bytes in total")]
    TooLarge,
}

impl CompactionSummary {
    /// Returns every list with its wire field name, in field order.
    #[must_use]
    pub fn lists(&self) -> [(&'static str, &Vec<String>); 5] {
        [
            ("done", &self.done),
            ("pending", &self.pending),
            ("decisions", &self.decisions),
            ("key_paths", &self.key_paths),
            ("open_errors", &self.open_errors),
        ]
    }

    /// Checks the item count, per-item length and total size bounds.
    ///
    /// # Errors
    /// Returns the first failing bound, checking the goal, then each list in field order, then
    /// the total.
    pub fn validate(&self) -> Result<(), InvalidCompactionSummary> {
        if self.goal.chars().count() > MAX_COMPACTION_ITEM_CHARS {
            return Err(InvalidCompactionSummary::ItemTooLong { field: "goal" });
        }
        let mut total = self.goal.len();
        for (field, items) in self.lists() {
            if items.len() > MAX_COMPACTION_ITEMS {
                return Err(InvalidCompactionSummary::TooManyItems { field });
            }
            if items
                .iter()
                .any(|item| item.chars().count() > MAX_COMPACTION_ITEM_CHARS)
            {
                return Err(InvalidCompactionSummary::ItemTooLong { field });
            }
            total = items
                .iter()
                .map(String::len)
                .fold(total, usize::saturating_add);
        }
        if total > MAX_COMPACTION_SUMMARY_BYTES {
            return Err(InvalidCompactionSummary::TooLarge);
        }
        Ok(())
    }
}

pub(crate) fn summary<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<CompactionSummary, D::Error> {
    let summary = CompactionSummary::deserialize(deserializer)?;
    summary
        .validate()
        .map_err(|error| D::Error::custom(format!("summary: {error}")))?;
    Ok(summary)
}

pub(crate) fn version<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let version = u32::deserialize(deserializer)?;
    if version != COMPACTION_VERSION {
        return Err(D::Error::custom("version: expected 1"));
    }
    Ok(version)
}

pub(crate) fn version_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "integer", "const": COMPACTION_VERSION})
}
