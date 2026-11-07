//! A call's identity: the digest and execution context the Git relay binds one operation to.
use crate::{ConversationId, RunId, ToolCallId};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

/// Exactly 64 lowercase hexadecimal characters encoding a SHA-256 action digest.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct ActionHash(
    #[serde(deserialize_with = "hash")]
    #[schemars(length(min = 64, max = 64), regex(pattern = "^[0-9a-f]{64}$"))]
    String,
);
impl ActionHash {
    /// Encodes a complete digest; callers own canonicalization and hashing.
    #[must_use]
    pub fn from_digest(digest: [u8; 32]) -> Self {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        Self(
            digest
                .into_iter()
                .flat_map(|byte| {
                    [
                        char::from(HEX[usize::from(byte >> 4)]),
                        char::from(HEX[usize::from(byte & 15)]),
                    ]
                })
                .collect(),
        )
    }
    /// Borrows the canonical hexadecimal digest.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
fn hash<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(D::Error::custom(
            "action_hash: expected 64 lowercase hexadecimal characters",
        ));
    }
    Ok(value)
}

/// The exact execution location included in an action digest; never supplied by a model.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PermissionContext {
    /// The conversation that owns the binding.
    pub conversation_id: ConversationId,
    /// The run that produced the call.
    pub run_id: RunId,
    /// One-based step number.
    #[serde(deserialize_with = "positive")]
    #[schemars(range(min = 1))]
    pub step: u32,
    /// One-based authoritative model attempt.
    #[serde(deserialize_with = "positive")]
    #[schemars(range(min = 1))]
    pub attempt: u32,
    /// The original model call identifier.
    pub call_id: ToolCallId,
}
fn positive<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let value = u32::deserialize(deserializer)?;
    if value == 0 {
        return Err(D::Error::custom(
            "permission context counters must be positive",
        ));
    }
    Ok(value)
}
