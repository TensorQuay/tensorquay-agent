//! Validated wire identifiers and sequence numbers.

use std::{fmt, str::FromStr};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

/// An identifier does not meet the shared wire constraints.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum InvalidId {
    /// Identifiers contain between one and 64 bytes.
    #[error("identifier must contain 1 through 64 ASCII characters")]
    Length,
    /// Only ASCII letters, digits, underscores and hyphens are accepted.
    #[error("identifier contains a character outside A-Z, a-z, 0-9, underscore and hyphen")]
    Character,
}

pub(crate) fn validate_id(value: &str) -> Result<(), InvalidId> {
    if !(1..=64).contains(&value.len()) {
        return Err(InvalidId::Length);
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(InvalidId::Character);
    }
    Ok(())
}

fn deserialize_id<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    validate_id(&value).map_err(D::Error::custom)?;
    Ok(value)
}

macro_rules! identifier {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize, JsonSchema)]
        #[serde(transparent)]
        pub struct $name(
            #[serde(deserialize_with = "deserialize_id")]
            #[schemars(length(min = 1, max = 64), regex(pattern = "^[A-Za-z0-9_-]+$"))]
            String,
        );

        impl $name {
            /// Borrows the validated identifier without allocating.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = InvalidId;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                validate_id(&value)?;
                Ok(Self(value))
            }
        }

        impl FromStr for $name {
            type Err = InvalidId;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::try_from(value.to_owned())
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

identifier!(
    ConversationId,
    "A validated conversation identifier, encoded as a JSON string."
);
identifier!(
    RunId,
    "A validated run identifier, encoded as a JSON string."
);
identifier!(
    ToolCallId,
    "A validated tool-call identifier, encoded as a JSON string."
);
identifier!(
    WorkspaceId,
    "A validated workspace identifier, encoded as a JSON string."
);

/// A per-conversation sequence number; zero denotes no received events.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(transparent)]
pub struct Seq(u64);

impl Seq {
    /// The sequence before any event has been received.
    pub const ZERO: Self = Self(0);

    /// Returns the next number, or `None` after the maximum representable value.
    #[must_use]
    pub const fn next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

impl From<u64> for Seq {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl fmt::Display for Seq {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}
