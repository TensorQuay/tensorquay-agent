//! The connector record (ADR-0031 decision 7): a remote MCP server the gateway proxies to, stored as one document
//! and validated by one function whether it comes from the `[[connectors]]` seed or the admin API. Credentials never
//! appear in it: `secret_ref` names a secret in the store.
use std::{collections::BTreeSet, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::InvalidReason;

/// The longest connector URL.
const MAX_URL_BYTES: usize = 2048;
/// Tool names in one allow or deny list.
const MAX_TOOL_FILTER: usize = 256;
const MAX_TOOL_NAME_BYTES: usize = 128;
const DEFAULT_CALL_TIMEOUT_SECONDS: u32 = 60;
const MAX_CALL_TIMEOUT_SECONDS: u32 = 600;
/// A custom credential header: `X-` and at most this many characters in all.
const MAX_HEADER_CHARS: usize = 64;
/// Query keys that would carry a credential in the URL, where it could be logged; matched as substrings.
const CREDENTIAL_QUERY_KEYS: [&str; 8] = [
    "key",
    "token",
    "secret",
    "password",
    "credential",
    "auth",
    "signature",
    "sig",
];

/// Where a record comes from: the file may use the file-only fields (`insecure_http`, `ca_file`); the API may not.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Origin {
    File,
    Admin,
}

/// How the proxy sends the credential.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Scheme {
    /// No credential.
    #[default]
    None,
    /// `Authorization: Bearer <credential>`.
    Bearer,
    /// `<header>: <credential>`.
    Header,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Auth {
    #[serde(default)]
    pub scheme: Scheme,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    /// A call without any credential (per-user or shared) fails with `424 connector_credential_missing`.
    #[serde(default)]
    pub required: bool,
}

/// Exactly one of the lists.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ToolFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deny: Option<Vec<String>>,
}

/// One connector as stored and as the API shows it (without its metadata).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConnectorDocument {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub auth: Auth,
    /// The shared credential; a user's own credential takes precedence (ADR-0031 decision 3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolFilter>,
    #[serde(default = "default_timeout")]
    pub call_timeout_seconds: u32,
    #[serde(default = "enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub enabled_by_default: bool,
    /// File-only: plain `http` for tests and on-premises servers.
    #[serde(default, skip_serializing_if = "is_false")]
    pub insecure_http: bool,
    /// File-only: a private certificate authority for this server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca_file: Option<PathBuf>,
}

const fn default_timeout() -> u32 {
    DEFAULT_CALL_TIMEOUT_SECONDS
}

const fn enabled() -> bool {
    true
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes a reference"
)]
const fn is_false(value: &bool) -> bool {
    !*value
}

/// Why a record was refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Refusal {
    /// A field breaks its rule.
    Invalid(&'static str, InvalidReason),
    /// The name belongs to a stdio MCP server; both share the `mcp__<name>__` namespace.
    NameTaken,
}

/// Checks every rule of ADR-0031 decision 7. `stdio_names` are the `[[mcp.servers]]` names.
///
/// # Errors
/// The first field that breaks its rule, or a name another MCP server holds.
pub(crate) fn validate(
    document: &ConnectorDocument,
    origin: Origin,
    stdio_names: &[String],
) -> Result<(), Refusal> {
    let invalid = |field, reason| Err(Refusal::Invalid(field, reason));
    if !crate::config::valid_mcp_name(&document.name) {
        return invalid("name", InvalidReason::NameInvalid);
    }
    if stdio_names.contains(&document.name) {
        return Err(Refusal::NameTaken);
    }
    if origin == Origin::Admin && document.insecure_http {
        return invalid("insecure_http", InvalidReason::FileOnly);
    }
    if let Some(path) = &document.ca_file {
        if origin == Origin::Admin {
            return invalid("ca_file", InvalidReason::FileOnly);
        }
        if !path.is_absolute() {
            return invalid("ca_file", InvalidReason::ParameterInvalid);
        }
    }
    url(&document.url, document.insecure_http)?;
    auth(&document.auth)?;
    if let Some(reference) = &document.secret_ref {
        if reference.parse::<crate::secrets::SecretRef>().is_err() {
            return invalid("secret_ref", InvalidReason::SecretRefInvalid);
        }
        // A shared credential is the install's, never one user's own (FR-19, FR-24).
        if tqa_types::is_per_user_secret(reference) {
            return invalid("secret_ref", InvalidReason::SecretRefReserved);
        }
        if document.auth.scheme == Scheme::None {
            return invalid("secret_ref", InvalidReason::AuthInvalid);
        }
    }
    if let Some(tools) = &document.tools
        && !tool_filter_ok(tools)
    {
        return invalid("tools", InvalidReason::ToolFilterInvalid);
    }
    if !(1..=MAX_CALL_TIMEOUT_SECONDS).contains(&document.call_timeout_seconds) {
        return invalid("call_timeout_seconds", InvalidReason::LimitOutOfRange);
    }
    Ok(())
}

/// The URL rules of [`url`] for another record kind (a decision engine), as the field and its reason.
pub(crate) fn check_url(
    text: &str,
    insecure_http: bool,
) -> Result<(), (&'static str, InvalidReason)> {
    url(text, insecure_http).map_err(parts)
}

/// The credential-header rules of [`auth`] for another record kind (a decision engine), as the field and its reason.
pub(crate) fn check_auth(auth_config: &Auth) -> Result<(), (&'static str, InvalidReason)> {
    auth(auth_config).map_err(parts)
}

fn parts(refusal: Refusal) -> (&'static str, InvalidReason) {
    match refusal {
        Refusal::Invalid(field, reason) => (field, reason),
        Refusal::NameTaken => ("name", InvalidReason::NameInvalid),
    }
}

/// `https` (or `http` when the file allows it), a host, no user information, no fragment, no credential in the query.
fn url(text: &str, insecure_http: bool) -> Result<(), Refusal> {
    let refused = Err(Refusal::Invalid("url", InvalidReason::BaseUrlInvalid));
    if text.len() > MAX_URL_BYTES {
        return refused;
    }
    let Ok(parsed) = url::Url::parse(text) else {
        return refused;
    };
    let scheme_ok = parsed.scheme() == "https" || (insecure_http && parsed.scheme() == "http");
    if !scheme_ok
        || parsed.host_str().is_none_or(str::is_empty)
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
    {
        return refused;
    }
    let credential = parsed.query_pairs().any(|(key, _)| {
        let key = key.to_ascii_lowercase();
        CREDENTIAL_QUERY_KEYS.iter().any(|word| key.contains(word))
    });
    if credential {
        return Err(Refusal::Invalid("url", InvalidReason::AuthQueryInvalid));
    }
    Ok(())
}

/// A header only for the header scheme, and a required credential only with a scheme to send it.
fn auth(auth: &Auth) -> Result<(), Refusal> {
    match (auth.scheme, auth.header.as_deref()) {
        (Scheme::Header, Some(header)) if header_ok(header) => {}
        (Scheme::Header, _) => {
            return Err(Refusal::Invalid("auth", InvalidReason::AuthHeaderInvalid));
        }
        (_, Some(_)) => return Err(Refusal::Invalid("auth", InvalidReason::AuthInvalid)),
        (Scheme::None, None) if auth.required => {
            return Err(Refusal::Invalid("auth", InvalidReason::AuthInvalid));
        }
        _ => {}
    }
    Ok(())
}

/// `Authorization`, `X-Api-Key`, or an `X-` name of letters, digits and `-`, at most 64 characters.
fn header_ok(header: &str) -> bool {
    if header.eq_ignore_ascii_case("authorization") || header.eq_ignore_ascii_case("x-api-key") {
        return true;
    }
    header.len() > 2
        && header.len() <= MAX_HEADER_CHARS
        && header[..2].eq_ignore_ascii_case("x-")
        && header
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn tool_filter_ok(filter: &ToolFilter) -> bool {
    let ((Some(names), None) | (None, Some(names))) = (&filter.allow, &filter.deny) else {
        return false;
    };
    let distinct = names.iter().collect::<BTreeSet<_>>().len() == names.len();
    (1..=MAX_TOOL_FILTER).contains(&names.len())
        && distinct
        && names.iter().all(|name| {
            !name.is_empty()
                && name.len() <= MAX_TOOL_NAME_BYTES
                && !name.chars().any(char::is_control)
        })
}

#[cfg(test)]
#[path = "document_tests.rs"]
mod tests;
