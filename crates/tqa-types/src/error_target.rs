//! Validated destination metadata for errors; no resolution or network access.
use std::{net::IpAddr, num::NonZeroU16};

use schemars::{JsonSchema, Schema, generate::SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

use crate::{ErrorCode, FrameError};

/// A bounded destination for client-authored error copy, preserving the supplied ASCII spelling.
/// Unknown fields are ignored for compatibility with future server frames.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ErrorTarget {
    #[serde(deserialize_with = "deserialize_host")]
    #[schemars(schema_with = "host_schema")]
    host: String,
    port: NonZeroU16,
}

impl ErrorTarget {
    /// Accepts an ASCII hostname (including IDNA A-labels) or a bare IP literal and a nonzero port.
    /// The caller converts Unicode names to IDNA before constructing this wire value.
    ///
    /// # Errors
    /// Rejects hosts outside 1..=253 bytes, invalid labels or literals, and port zero.
    pub fn new(host: impl Into<String>, port: u16) -> Result<Self, FrameError> {
        let host = host.into();
        if !valid_host(&host) {
            return Err(invalid(
                "target.host",
                "expected a bounded ASCII hostname or bare IP literal",
            ));
        }
        let port = NonZeroU16::new(port)
            .ok_or_else(|| invalid("target.port", "expected a port from 1 through 65535"))?;
        Ok(Self { host, port })
    }

    /// Returns the validated host without brackets or an embedded port.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Returns a port from 1 through 65535.
    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port.get()
    }
}

fn invalid(path: &str, detail: &str) -> FrameError {
    FrameError {
        code: ErrorCode::InvalidFrame,
        path: path.into(),
        detail: detail.into(),
    }
}

fn valid_host(host: &str) -> bool {
    if !(1..=253).contains(&host.len()) || !host.is_ascii() {
        return false;
    }
    if host.parse::<IpAddr>().is_ok() {
        return true;
    }
    host.strip_suffix('.')
        .unwrap_or(host)
        .split('.')
        .all(|label| {
            (1..=63).contains(&label.len())
                && label
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .as_bytes()
                    .last()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

fn deserialize_host<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let host = String::deserialize(deserializer)?;
    if valid_host(&host) {
        Ok(host)
    } else {
        Err(D::Error::custom(
            "expected a bounded ASCII hostname or bare IP literal",
        ))
    }
}

fn host_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "type":"string", "minLength":1, "maxLength":253,
        "not":{"pattern":"[^A-Za-z0-9.:-]"},
        "anyOf":[
            {"pattern":"^[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?(?:\\.[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?)*\\.?$"},
            {"format":"ipv6", "pattern":":"}
        ]
    })
}
