//! Numeric run ceilings carried by the validated runtime-link configuration.
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

/// Explicit run budgets; deserialization rejects every out-of-range field.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunLimits {
    /// Maximum completed steps, from 1 through 500.
    #[serde(deserialize_with = "steps")]
    #[schemars(range(min = 1, max = 500))]
    pub max_steps: u32,
    /// Wall-clock ceiling in seconds, from 1 through 86,400.
    #[serde(deserialize_with = "wall_clock")]
    #[schemars(range(min = 1, max = 86_400))]
    pub wall_clock_secs: u32,
    /// Maximum authoritative tool-result bytes, from 1024 through 262,144.
    #[serde(deserialize_with = "result_bytes")]
    #[schemars(range(min = 1024, max = 262_144))]
    pub tool_result_bytes: u32,
    /// Output tokens across all model attempts, from 1 through 16,777,216.
    /// Older configurations default to 131,072 tokens per run.
    #[serde(default = "default_output_tokens", deserialize_with = "output_tokens")]
    #[schemars(range(min = 1, max = 16_777_216))]
    pub max_output_tokens: u64,
}

impl Default for RunLimits {
    fn default() -> Self {
        Self {
            max_steps: 100,
            wall_clock_secs: 1800,
            tool_result_bytes: 8192,
            max_output_tokens: default_output_tokens(),
        }
    }
}

impl RunLimits {
    /// Checks the same ranges for a programmatically constructed budget.
    ///
    /// # Errors
    /// Rejects any field outside its wire range.
    pub fn validate(&self) -> Result<(), crate::FrameError> {
        if !(1..=16_777_216).contains(&self.max_output_tokens) {
            return Err(crate::FrameError {
                code: crate::ErrorCode::InvalidFrame,
                path: "limits.max_output_tokens".into(),
                detail: "expected 1 through 16777216".into(),
            });
        }
        for (path, value, minimum, maximum) in [
            ("limits.max_steps", self.max_steps, 1, 500),
            ("limits.wall_clock_secs", self.wall_clock_secs, 1, 86_400),
            (
                "limits.tool_result_bytes",
                self.tool_result_bytes,
                1024,
                262_144,
            ),
        ] {
            if !(minimum..=maximum).contains(&value) {
                return Err(crate::FrameError {
                    code: crate::ErrorCode::InvalidFrame,
                    path: path.into(),
                    detail: format!("expected {minimum} through {maximum}"),
                });
            }
        }
        Ok(())
    }
}

const fn default_output_tokens() -> u64 {
    131_072
}

fn output_tokens<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    let value = u64::deserialize(deserializer)?;
    if !(1..=16_777_216).contains(&value) {
        return Err(D::Error::custom("expected 1 through 16777216"));
    }
    Ok(value)
}

fn steps<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    bounded(deserializer, 1, 500)
}
fn wall_clock<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    bounded(deserializer, 1, 86_400)
}
fn result_bytes<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    bounded(deserializer, 1024, 262_144)
}
fn bounded<'de, D: Deserializer<'de>>(
    deserializer: D,
    minimum: u32,
    maximum: u32,
) -> Result<u32, D::Error> {
    let value = u32::deserialize(deserializer)?;
    if !(minimum..=maximum).contains(&value) {
        return Err(D::Error::custom(format!(
            "expected {minimum} through {maximum}"
        )));
    }
    Ok(value)
}
