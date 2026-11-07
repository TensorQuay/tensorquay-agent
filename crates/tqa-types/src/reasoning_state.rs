//! Opaque provider reasoning state (ADR-0027): one provider output item, kept as received and never
//! interpreted, logged or shown.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use std::fmt;

/// One provider reasoning item as JSON object text, at most [`crate::MAX_REASONING_ITEM_BYTES`].
/// Its `Debug` form shows only the size.
#[derive(Clone, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct ReasoningItem(String);

/// An item that is too large or not a JSON object.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("reasoning item: expected a JSON object of at most 131072 bytes")]
pub struct InvalidReasoningItem;

impl ReasoningItem {
    /// Accepts JSON object text within the item bound.
    ///
    /// # Errors
    /// Returns [`InvalidReasoningItem`] for oversized text or anything but a JSON object.
    pub fn new(text: String) -> Result<Self, InvalidReasoningItem> {
        if text.len() > crate::MAX_REASONING_ITEM_BYTES
            || serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&text).is_err()
        {
            return Err(InvalidReasoningItem);
        }
        Ok(Self(text))
    }

    /// The item text, for replay to the provider only.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The item's size in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Always false: an item is a non-empty JSON object.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for ReasoningItem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "[reasoning state, {} bytes]", self.0.len())
    }
}

impl<'de> Deserialize<'de> for ReasoningItem {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}
