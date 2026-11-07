//! A run's host keys as the runtime receives them (ADR-0037 §3, revision 11): for each key, the environment variable
//! its commands see, where it may go, how the service expects it, and the run's placeholder for it. Never the value.
use std::fmt;

use schemars::{JsonSchema, Schema, generate::SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

use crate::{ErrorCode, FrameError};

/// The most keys one run holds: a member's 64 own keys and 256 shared ones.
pub const MAX_RUN_HOST_KEYS: usize = 320;
/// The longest install-CA bundle in `session.config`: two P-256 certificates are far shorter.
pub const MAX_HOST_CA_BYTES: usize = 8 * 1024;
const CERTIFICATE_BEGIN: &str = "-----BEGIN CERTIFICATE-----\n";
const CERTIFICATE_END: &str = "-----END CERTIFICATE-----\n";
const HOST_CA_RULE: &str =
    "^(-----BEGIN CERTIFICATE-----\\n[A-Za-z0-9+/=\\n]+-----END CERTIFICATE-----\\n){1,2}$";
/// Every placeholder starts with this, followed by 32 random bytes in unpadded base64url (43 characters).
pub const PLACEHOLDER_PREFIX: &str = "tqa_ph_";
const PLACEHOLDER_BYTES: usize = 43;

const ENV_RULE: &str = "^[A-Z][A-Z0-9_]{0,63}$";
/// A host name, or an IP address (an IPv6 address has a colon and no brackets). The parser also insists on an address's
/// canonical spelling (zeros compressed as `Ipv6Addr` writes them), which a pattern can't say.
pub(crate) const HOST_RULE: &str = r"^([a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?(\.[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?)*|[0-9a-f.]*:[0-9a-f:.]*)$";
const PATH_RULE: &str = "^/[^?#]*[^/?#]$";
const PLACEHOLDER_RULE: &str = "^tqa_ph_[A-Za-z0-9_-]{43}$";

/// How the service expects the key; the header scheme comes in a later revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HostKeyScheme {
    /// `Authorization: Bearer <placeholder>`.
    Bearer,
    /// `Authorization: Basic base64(<user>:<placeholder>)`.
    Basic,
}

/// One key of a run: never its value, only the placeholder the proxy swaps for it.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HostKeyGrant {
    /// The environment variable every command of the run gets, holding the placeholder.
    #[serde(deserialize_with = "env")]
    #[schemars(regex(pattern = ENV_RULE), length(max = 64))]
    env: String,
    /// The one host it goes to, lower case.
    #[serde(deserialize_with = "host")]
    #[schemars(regex(pattern = HOST_RULE), length(min = 1, max = 253))]
    host: String,
    #[schemars(range(min = 1))]
    #[serde(deserialize_with = "port")]
    port: u16,
    /// Only below this path, when set.
    #[serde(default, deserialize_with = "path_prefix")]
    #[schemars(regex(pattern = PATH_RULE), length(max = 512))]
    path_prefix: Option<String>,
    scheme: HostKeyScheme,
    /// The user name a `basic` key goes with.
    #[serde(default, deserialize_with = "user_name")]
    #[schemars(length(min = 1, max = 256))]
    user_name: Option<String>,
    /// What the proxy replaces with the value; valid only for this run, sandbox and address.
    #[serde(deserialize_with = "placeholder")]
    #[schemars(regex(pattern = PLACEHOLDER_RULE))]
    placeholder: String,
}

impl HostKeyGrant {
    /// A checked grant: `(host, port, path_prefix)` where it goes, `(scheme, user_name)` how.
    ///
    /// # Errors
    /// Any field outside its rule.
    pub fn new(
        env: String,
        (host, port, path_prefix): (String, u16, Option<String>),
        (scheme, user_name): (String, Option<String>),
        placeholder: String,
    ) -> Result<Self, FrameError> {
        let scheme = match scheme.as_str() {
            "bearer" => HostKeyScheme::Bearer,
            "basic" => HostKeyScheme::Basic,
            _ => return Err(invalid("host_keys.scheme", "expected bearer or basic")),
        };
        let grant = Self {
            env,
            host,
            port,
            path_prefix,
            scheme,
            user_name,
            placeholder,
        };
        grant.check()?;
        Ok(grant)
    }

    fn check(&self) -> Result<(), FrameError> {
        let checks = [
            (
                valid_env(&self.env),
                "host_keys.env",
                "expected an environment name",
            ),
            (
                valid_host(&self.host),
                "host_keys.host",
                "expected a lower-case host name or a canonical IP address",
            ),
            (
                self.port != 0,
                "host_keys.port",
                "expected a port from 1 through 65535",
            ),
            (
                self.path_prefix.as_deref().is_none_or(valid_path),
                "host_keys.path_prefix",
                "expected a path starting with /",
            ),
            (
                self.user_name.as_deref().is_none_or(valid_user),
                "host_keys.user_name",
                "expected 1 through 256 bytes",
            ),
            (
                valid_placeholder(&self.placeholder),
                "host_keys.placeholder",
                "expected tqa_ph_ and 43 base64url characters",
            ),
        ];
        match checks.into_iter().find(|(valid, ..)| !valid) {
            Some((_, path, detail)) => Err(invalid(path, detail)),
            None => Ok(()),
        }
    }

    /// The environment variable.
    #[must_use]
    pub fn env(&self) -> &str {
        &self.env
    }

    /// The host.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The port.
    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// The path prefix, when set.
    #[must_use]
    pub fn path_prefix(&self) -> Option<&str> {
        self.path_prefix.as_deref()
    }

    /// How the service expects it.
    #[must_use]
    pub const fn scheme(&self) -> HostKeyScheme {
        self.scheme
    }

    /// A `basic` key's user name, when set.
    #[must_use]
    pub fn user_name(&self) -> Option<&str> {
        self.user_name.as_deref()
    }

    /// The run's placeholder: for the variable's value only, never for a log or the model.
    #[must_use]
    pub fn placeholder(&self) -> &str {
        &self.placeholder
    }
}

/// A debug print names the key and where it goes, never its placeholder.
impl fmt::Debug for HostKeyGrant {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HostKeyGrant")
            .field("env", &self.env)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("path_prefix", &self.path_prefix)
            .field("scheme", &self.scheme)
            .field("user_name", &self.user_name)
            .field("placeholder", &"<redacted>")
            .finish()
    }
}

fn invalid(path: &str, detail: &str) -> FrameError {
    FrameError {
        code: ErrorCode::InvalidFrame,
        path: path.into(),
        detail: detail.into(),
    }
}

fn valid_env(env: &str) -> bool {
    let bytes = env.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes[0].is_ascii_uppercase()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'_')
}

/// A lower-case host name, or an IP address in its canonical form (an IPv6 address without brackets): one spelling for
/// one address, as the gateway's proxy compares it.
fn valid_host(host: &str) -> bool {
    if let Ok(address) = host.parse::<std::net::IpAddr>() {
        return address.to_string() == host;
    }
    (1..=253).contains(&host.len())
        && host.split('.').all(|label| {
            let bytes = label.as_bytes();
            (1..=63).contains(&bytes.len())
                && bytes[0] != b'-'
                && bytes[bytes.len() - 1] != b'-'
                && bytes
                    .iter()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
        })
}

fn valid_path(path: &str) -> bool {
    path.len() <= 512
        && path.len() >= 2
        && path.starts_with('/')
        && !path.ends_with('/')
        && !path.contains(['?', '#'])
}

const fn valid_user(user: &str) -> bool {
    !user.is_empty() && user.len() <= 256
}

fn valid_placeholder(placeholder: &str) -> bool {
    placeholder
        .strip_prefix(PLACEHOLDER_PREFIX)
        .is_some_and(|rest| {
            rest.len() == PLACEHOLDER_BYTES
                && rest
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        })
}

fn checked<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
    valid: impl Fn(&T) -> bool,
    detail: &str,
) -> Result<T, D::Error> {
    let value = T::deserialize(deserializer)?;
    if valid(&value) {
        Ok(value)
    } else {
        Err(D::Error::custom(detail))
    }
}

fn env<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    checked(
        deserializer,
        |env: &String| valid_env(env),
        "expected an environment name",
    )
}

fn host<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    checked(
        deserializer,
        |host: &String| valid_host(host),
        "expected a lower-case host name",
    )
}

fn port<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u16, D::Error> {
    checked(
        deserializer,
        |port: &u16| *port != 0,
        "expected a port from 1 through 65535",
    )
}

fn path_prefix<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    checked(
        deserializer,
        |path: &Option<String>| path.as_deref().is_none_or(valid_path),
        "expected a path starting with /",
    )
}

fn user_name<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    checked(
        deserializer,
        |user: &Option<String>| user.as_deref().is_none_or(valid_user),
        "expected 1 through 256 bytes",
    )
}

fn placeholder<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    checked(
        deserializer,
        |placeholder: &String| valid_placeholder(placeholder),
        "expected tqa_ph_ and 43 base64url characters",
    )
}

/// The `host_keys` list: at most [`MAX_RUN_HOST_KEYS`], each variable once.
pub(crate) fn grants<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<HostKeyGrant>, D::Error> {
    let grants = Vec::<HostKeyGrant>::deserialize(deserializer)
        .map_err(|error| D::Error::custom(format!("host_keys: {error}")))?;
    if grants.len() > MAX_RUN_HOST_KEYS {
        return Err(D::Error::custom("host_keys: expected at most 320 keys"));
    }
    for (index, grant) in grants.iter().enumerate() {
        if grants[..index]
            .iter()
            .any(|earlier| earlier.env == grant.env)
        {
            return Err(D::Error::custom("host_keys: each variable at most once"));
        }
    }
    Ok(grants)
}

pub(crate) fn grants_schema(generator: &mut SchemaGenerator) -> Schema {
    let item = generator.subschema_for::<HostKeyGrant>();
    json_schema!({"type": "array", "maxItems": MAX_RUN_HOST_KEYS, "uniqueItems": true, "items": item})
}

/// Whether `bundle` is one or two PEM certificates (the install CA and, after a rotation, the previous one), and
/// nothing else, within [`MAX_HOST_CA_BYTES`].
fn valid_host_ca(bundle: &str) -> bool {
    let mut rest = bundle;
    let mut count = 0;
    while !rest.is_empty() {
        let Some(body) = rest.strip_prefix(CERTIFICATE_BEGIN) else {
            return false;
        };
        let Some(end) = body.find(CERTIFICATE_END) else {
            return false;
        };
        let (content, after) = (&body[..end], &body[end + CERTIFICATE_END.len()..]);
        if content.is_empty()
            || !content.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=' | b'\n')
            })
        {
            return false;
        }
        count += 1;
        rest = after;
    }
    (1..=2).contains(&count) && bundle.len() <= MAX_HOST_CA_BYTES
}

/// The `host_ca` of `session.config` (revision 11, ADR-0037 §6).
pub(crate) fn host_ca<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    checked(
        deserializer,
        |bundle: &Option<String>| bundle.as_deref().is_none_or(valid_host_ca),
        "host_ca: expected one or two PEM certificates",
    )
}

pub(crate) fn host_ca_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": ["string", "null"], "maxLength": MAX_HOST_CA_BYTES, "pattern": HOST_CA_RULE})
}

/// Why the proxy refused a keyed request (ADR-0037 §8, Amendment 1), as `host_key.refused` says it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HostKeyOutcome {
    /// An earlier or another run's placeholder.
    ForeignRun,
    /// Another sandbox's or task's placeholder.
    ForeignSandbox,
    /// The placeholder's run had ended.
    Expired,
    /// Another host or port, or outside the key's path.
    WrongHost,
    /// Anywhere but the key's Authorization form.
    WrongPosition,
    /// The network list no longer allows the host.
    NotAllowed,
    /// A placeholder the gateway doesn't hold.
    Absent,
    /// The key was deleted, or its person disabled, during the run.
    Revoked,
    /// The use couldn't be recorded, so nothing was sent.
    Unrecorded,
    /// The key's value couldn't be read from the store.
    Unavailable,
}

/// Whether `name` could be a key's variable, for the frames.
pub(crate) fn valid_notice(name: Option<&str>, host: &str, port: u16, sender_id: &str) -> bool {
    name.is_none_or(valid_env)
        && valid_host(host)
        && port != 0
        && sender_id.len() == 36
        && sender_id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
}
