//! The task's network allow-list as the runtime receives it (ADR-0035 §3): the prompt shows it so the agent knows the
//! rule before it tries; the gateway's proxy enforces it, never the runtime.
use crate::{ErrorCode, FrameError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The most entries a `session.config` carries; the gateway counts the rest in `more`.
pub const MAX_NETWORK_ENTRIES: usize = 32;
/// The most entries `more` can count: the gateway's allow-list holds at most 256 rules.
pub const MAX_NETWORK_MORE: u32 = 256;
const MAX_PORTS: usize = 16;
const MAX_HOST_BYTES: usize = 253;

/// The allowed hosts and ports, in the proxy's order. An empty list means nothing outside the sandbox is reachable,
/// which also covers egress turned off.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, try_from = "NetworkFields")]
// The schema is `NetworkFields`'s, which carries the bounds.
pub struct NetworkSnapshot {
    /// At most [`MAX_NETWORK_ENTRIES`] host names. The gateway leaves address literals out, so the prompt never shows
    /// the internal network's addresses, and counts them in `more` (ADR-0035 §3).
    pub allowed: Vec<NetworkEntry>,
    /// How many allowed entries are not listed.
    pub more: u32,
}

/// One allowed destination.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NetworkEntry {
    /// An exact lower-case host name, `*.` and a domain for any of its subdomains, or a bare `*` for any website (the
    /// `open_web` preset, on its ports); never an address.
    #[schemars(
        length(min = 1, max = MAX_HOST_BYTES),
        regex(pattern = r"^(\*|(\*\.)?[a-z0-9-]+(\.[a-z0-9-]+)*)$")
    )]
    pub host: String,
    /// One to 16 distinct non-zero ports.
    #[schemars(length(min = 1, max = MAX_PORTS), inner(range(min = 1)))]
    pub ports: Vec<u16>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct NetworkFields {
    #[schemars(length(max = MAX_NETWORK_ENTRIES))]
    allowed: Vec<NetworkEntry>,
    #[schemars(range(max = MAX_NETWORK_MORE))]
    more: u32,
}

impl TryFrom<NetworkFields> for NetworkSnapshot {
    type Error = FrameError;
    fn try_from(value: NetworkFields) -> Result<Self, Self::Error> {
        if value.allowed.len() > MAX_NETWORK_ENTRIES {
            return Err(invalid("allowed", "expected at most 32 entries"));
        }
        if value.more > MAX_NETWORK_MORE {
            return Err(invalid("more", "expected at most 256"));
        }
        for (index, entry) in value.allowed.iter().enumerate() {
            if !host(&entry.host) {
                return Err(invalid(
                    &format!("allowed[{index}].host"),
                    "expected a lower-case host name, `*.` and a domain, or `*`",
                ));
            }
            if !ports(&entry.ports) {
                return Err(invalid(
                    &format!("allowed[{index}].ports"),
                    "expected 1 to 16 distinct non-zero ports",
                ));
            }
        }
        Ok(Self {
            allowed: value.allowed,
            more: value.more,
        })
    }
}

/// The allow-list's own host rule (a name of labels, or `*.` and one) in its canonical form, minus addresses.
fn host(host: &str) -> bool {
    // Any website: the open_web preset's rule, the only wildcard that isn't a domain's subdomains.
    if host == "*" {
        return true;
    }
    let name = host.strip_prefix("*.").unwrap_or(host);
    !name.is_empty()
        && host.len() <= MAX_HOST_BYTES
        && name.parse::<std::net::IpAddr>().is_err()
        // An all-digit dotted name (an IPv4 address under a wildcard) names an address, never a host.
        && !name.split('.').all(|label| label.bytes().all(|byte| byte.is_ascii_digit()))
        && name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
}

fn ports(ports: &[u16]) -> bool {
    !ports.is_empty()
        && ports.len() <= MAX_PORTS
        && !ports.contains(&0)
        && ports
            .iter()
            .enumerate()
            .all(|(index, port)| !ports[..index].contains(port))
}

fn invalid(path: &str, detail: &str) -> FrameError {
    FrameError {
        code: ErrorCode::InvalidFrame,
        path: format!("$.network.{path}"),
        detail: detail.into(),
    }
}
