//! Why a tool call failed, as data for clients (revision 10, C1): the model reads the tool's own words, and a client
//! words the failure itself from this, never from the model's text.
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

use crate::{ErrorCode, ErrorTarget, FrameError};

/// A failed tool call's stable code and, for a refused destination, where.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolFailure {
    /// The tool's failure code, such as `egress_denied`: 1 to 64 lowercase letters and underscores.
    #[serde(deserialize_with = "deserialize_code")]
    #[schemars(length(min = 1, max = 64), regex(pattern = r"^[a-z_]{1,64}$"))]
    code: String,
    /// The destination a refusal names; absent for every other failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    target: Option<ErrorTarget>,
}

impl ToolFailure {
    /// A failure with its code and, when a destination was refused, the destination.
    ///
    /// # Errors
    /// Rejects a code outside `^[a-z_]{1,64}$`.
    pub fn new(code: impl Into<String>, target: Option<ErrorTarget>) -> Result<Self, FrameError> {
        let code = code.into();
        if !valid_code(&code) {
            return Err(FrameError {
                code: ErrorCode::InvalidFrame,
                path: "failure.code".into(),
                detail: "expected 1 to 64 lowercase letters and underscores".into(),
            });
        }
        Ok(Self { code, target })
    }

    /// The stable code.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    /// The refused destination, when there is one.
    #[must_use]
    pub const fn target(&self) -> Option<&ErrorTarget> {
        self.target.as_ref()
    }
}

fn valid_code(code: &str) -> bool {
    (1..=64).contains(&code.len())
        && code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
}

fn deserialize_code<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let code = String::deserialize(deserializer)?;
    if valid_code(&code) {
        Ok(code)
    } else {
        Err(D::Error::custom(
            "expected 1 to 64 lowercase letters and underscores",
        ))
    }
}
