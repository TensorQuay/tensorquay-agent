//! Credential-free repository initialization inputs and durable status vocabulary.
use crate::{ErrorCode, ErrorTarget, FrameError};
use schemars::{JsonSchema, Schema, generate::SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

/// A public HTTPS repository to initialize once, before the first model run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, try_from = "CloneFields")]
pub struct WorkspaceClone {
    /// Credential-free ASCII HTTPS URL within 4096 bytes, including a repository path.
    #[schemars(schema_with = "url_schema")]
    pub url: String,
    /// Optional branch or tag name within 256 ASCII bytes; revision expressions are rejected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "optional_ref_schema")]
    pub r#ref: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CloneFields {
    #[schemars(schema_with = "url_schema")]
    url: String,
    #[serde(default)]
    #[schemars(schema_with = "optional_ref_schema")]
    r#ref: Option<String>,
}
impl TryFrom<CloneFields> for WorkspaceClone {
    type Error = FrameError;
    fn try_from(fields: CloneFields) -> Result<Self, Self::Error> {
        let value = Self {
            url: fields.url,
            r#ref: fields.r#ref,
        };
        value.validate()?;
        Ok(value)
    }
}
impl WorkspaceClone {
    /// Checks the same URL and branch/tag bounds used by deserialization.
    ///
    /// # Errors
    /// Rejects credentials, non-HTTPS URLs, invalid authorities or paths, and invalid ref names.
    pub fn validate(&self) -> Result<(), FrameError> {
        if !valid_url(&self.url) {
            return Err(invalid(
                "workspace.clone.url",
                "expected a public ASCII HTTPS repository URL within 4096 bytes",
            ));
        }
        if self.r#ref.as_deref().is_some_and(|name| !valid_ref(name)) {
            return Err(invalid(
                "workspace.clone.ref",
                "expected a branch or tag name within 256 ASCII bytes",
            ));
        }
        Ok(())
    }
}

/// Durable initialization progress; completed and skipped permit the first model run.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum WorkspaceCloneStatus {
    /// Repository initialization has begun.
    Started,
    /// The requested checkout is installed in the workspace.
    Completed,
    /// Existing workspace content was preserved without cloning.
    Skipped,
    /// Initialization failed; model execution must not start.
    Failed,
}

fn invalid(path: &str, detail: &str) -> FrameError {
    FrameError {
        code: ErrorCode::InvalidFrame,
        path: path.into(),
        detail: detail.into(),
    }
}
pub(crate) fn valid_url(url: &str) -> bool {
    if url.len() > 4096 || !url.is_ascii() {
        return false;
    }
    let Some((authority, path)) = url
        .strip_prefix("https://")
        .and_then(|rest| rest.split_once('/'))
    else {
        return false;
    };
    valid_authority(authority).is_some() && valid_path(path)
}
fn valid_authority(authority: &str) -> Option<()> {
    let (host, port) = if let Some(bracketed) = authority.strip_prefix('[') {
        let (host, suffix) = bracketed.split_once(']')?;
        host.parse::<std::net::Ipv6Addr>().ok()?;
        (
            host,
            if suffix.is_empty() {
                None
            } else {
                Some(suffix.strip_prefix(':')?)
            },
        )
    } else {
        let (host, port) = authority
            .rsplit_once(':')
            .map_or((authority, None), |(host, port)| (host, Some(port)));
        if host.contains(':') {
            return None;
        }
        (host, port)
    };
    let port = match port {
        Some(port) if !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit()) => {
            port.parse().ok()?
        }
        Some(_) => return None,
        None => 443,
    };
    ErrorTarget::new(host, port).ok()?;
    Some(())
}
fn valid_path(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    let mut bytes = path.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            if !bytes.next().is_some_and(|byte| byte.is_ascii_hexdigit())
                || !bytes.next().is_some_and(|byte| byte.is_ascii_hexdigit())
            {
                return false;
            }
        } else if !byte.is_ascii_alphanumeric() && !b"-._~!$&'()*+,;=:@/".contains(&byte) {
            return false;
        }
    }
    true
}
pub(crate) fn valid_ref(name: &str) -> bool {
    (1..=256).contains(&name.len())
        && name.is_ascii()
        && !name.starts_with('-')
        && !name.ends_with('.')
        && name != "@"
        && !name.contains("..")
        && !name.contains("@{")
        && !name
            .bytes()
            .any(|byte| byte.is_ascii_control() || b" ~^:?*[\\".contains(&byte))
        && name.split('/').all(|part| {
            !part.is_empty() && !part.starts_with('.') && !part.as_bytes().ends_with(b".lock")
        })
}
pub(crate) fn url_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type":"string", "minLength":1, "maxLength":4096,
        "pattern":"^https://[A-Za-z0-9.\\[\\]:-]+/(?:[A-Za-z0-9._~!$&'()*+,;=:@/-]|%[A-Fa-f0-9]{2})+$"})
}
fn optional_ref_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"anyOf":[{"type":"null"}, {"type":"string", "minLength":1, "maxLength":256,
    "not":{"anyOf":[
        {"pattern":"[^!-~]|[~^:?*\\[\\\\]"},
        {"pattern":"^-|^@$|@\\{|\\.\\.|\\.$|(^|/)\\.|\\.lock(/|$)|^/|/$|//"}
    ]}}]})
}
pub(crate) fn status_message<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    let message = String::deserialize(deserializer)?;
    if message.is_empty() || message.len() > 256 || message.chars().any(char::is_control) {
        return Err(D::Error::custom(
            "expected status text within 256 UTF-8 bytes without controls",
        ));
    }
    Ok(message)
}
pub(crate) fn status_message_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type":"string", "minLength":1, "maxLength":256,
        "pattern":"^[^\\u0000-\\u001f\\u007f-\\u009f]+$"})
}
