//! `[egress]`: the policy proxy's listener, its seed rules, and the one switch that turns egress off (ADR-0028,
//! ARCHITECTURE §4.2, ADR-0021). The proxy always runs unless `disabled = true`; its allow-list lives in the database,
//! seeded from these rules, and an empty list denies everything.
use crate::{
    GatewayError,
    egress::{RuleInput, canonical},
};
use serde::Deserialize;
use std::net::{Ipv4Addr, SocketAddr};

/// Rules a seed may list, as many as the proxy accepts.
const MAX_SEED_RULES: usize = 256;
/// Connections the proxy serves at once; more wait in the listen backlog.
const DEFAULT_CONNECTIONS: usize = 256;
/// A tunnel or keyed connection without traffic, or a new request, for this long is closed.
const DEFAULT_IDLE_SECONDS: u64 = 300;
/// Keyed connections (ADR-0037 §4) at once, in all and per sandbox; past either a new one is closed.
const DEFAULT_KEYED_CONNECTIONS: usize = 256;
const DEFAULT_KEYED_PER_SANDBOX: usize = 16;
/// The longest wait for a keyed host's answer to begin.
const DEFAULT_KEYED_ANSWER_SECONDS: u64 = 120;
/// The ranges a file may set them in.
const CONNECTIONS: std::ops::RangeInclusive<usize> = 1..=4096;
const PER_SANDBOX: std::ops::RangeInclusive<usize> = 1..=256;
const IDLE_SECONDS: std::ops::RangeInclusive<u64> = 1..=86_400;
const ANSWER_SECONDS: std::ops::RangeInclusive<u64> = 1..=3600;
/// The proxy's listener when the file does not name one.
const DEFAULT_LISTEN: SocketAddr =
    SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::UNSPECIFIED), 3128);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EgressInput {
    #[serde(default = "default_listen")]
    listen: SocketAddr,
    #[serde(default)]
    allow: Vec<RuleInput>,
    #[serde(default)]
    disabled: bool,
    #[serde(default = "default_connections")]
    connections: usize,
    #[serde(default = "default_idle_seconds")]
    idle_seconds: u64,
    #[serde(default = "default_keyed_connections")]
    keyed_connections: usize,
    #[serde(default = "default_keyed_per_sandbox")]
    keyed_per_sandbox: usize,
    #[serde(default = "default_keyed_answer_seconds")]
    keyed_answer_seconds: u64,
    /// A PEM bundle trusted beside the public roots for keyed hosts behind an internal certificate authority.
    #[serde(default)]
    ca_file: Option<std::path::PathBuf>,
    /// Addresses and networks an internal rule never reaches, beside what the gateway finds itself: the host's own
    /// addresses, which it can't see from a container (ADR-0035 amendment).
    #[serde(default)]
    internal_never: Vec<String>,
}

const fn default_connections() -> usize {
    DEFAULT_CONNECTIONS
}
const fn default_idle_seconds() -> u64 {
    DEFAULT_IDLE_SECONDS
}
const fn default_keyed_connections() -> usize {
    DEFAULT_KEYED_CONNECTIONS
}
const fn default_keyed_per_sandbox() -> usize {
    DEFAULT_KEYED_PER_SANDBOX
}
const fn default_keyed_answer_seconds() -> u64 {
    DEFAULT_KEYED_ANSWER_SECONDS
}

const fn default_listen() -> SocketAddr {
    DEFAULT_LISTEN
}

/// The validated proxy settings. The seed rules are canonical; the live list is the settings snapshot's.
#[derive(Clone, Debug)]
pub(crate) struct EgressConfig {
    pub listen: SocketAddr,
    /// Egress is off: sandboxes are internal-only and no rule applies (file-only, for locked-down installs).
    pub disabled: bool,
    /// The file's rules, imported into the database under the seed-change rule.
    pub seed: Vec<RuleInput>,
    /// Connections the proxy serves at once.
    pub connections: usize,
    /// How long a tunnel, or a keyed connection between requests, may be idle.
    pub idle: std::time::Duration,
    /// Keyed connections at once, in all and per sandbox (ADR-0037 §4).
    pub keyed_connections: usize,
    pub keyed_per_sandbox: usize,
    /// How long a keyed host may take to begin its answer.
    pub keyed_answer: std::time::Duration,
    /// Certificates trusted beside the public roots for keyed hosts (PEM, checked at start), from `ca_file`.
    /// Verification is never off.
    pub ca_pem: Option<std::sync::Arc<Vec<u8>>>,
    /// `internal_never`, parsed: what internal rules never reach beyond the gateway's own findings.
    pub internal_never: Vec<crate::egress::internal::Network>,
}

/// Validates the section; an absent section means the proxy runs on its default listener with no seed rules.
///
/// # Errors
/// Rejects invalid or too many rules, rules alongside `disabled`, a listener that collides with the public one, and a
/// `ca_file` that isn't a readable PEM file of authority certificates up to 1 MiB.
pub(super) fn egress(
    input: Option<EgressInput>,
    public: SocketAddr,
) -> Result<Box<EgressConfig>, GatewayError> {
    let input = input.unwrap_or(EgressInput {
        listen: DEFAULT_LISTEN,
        allow: Vec::new(),
        disabled: false,
        connections: DEFAULT_CONNECTIONS,
        idle_seconds: DEFAULT_IDLE_SECONDS,
        keyed_connections: DEFAULT_KEYED_CONNECTIONS,
        keyed_per_sandbox: DEFAULT_KEYED_PER_SANDBOX,
        keyed_answer_seconds: DEFAULT_KEYED_ANSWER_SECONDS,
        ca_file: None,
        internal_never: Vec::new(),
    });
    let bounds = [
        (
            CONNECTIONS.contains(&input.connections),
            "egress.connections must be 1 through 4096",
        ),
        (
            IDLE_SECONDS.contains(&input.idle_seconds),
            "egress.idle_seconds must be 1 through 86400",
        ),
        (
            CONNECTIONS.contains(&input.keyed_connections),
            "egress.keyed_connections must be 1 through 4096",
        ),
        (
            PER_SANDBOX.contains(&input.keyed_per_sandbox)
                && input.keyed_per_sandbox <= input.keyed_connections,
            "egress.keyed_per_sandbox must be 1 through 256, and at most keyed_connections",
        ),
        (
            ANSWER_SECONDS.contains(&input.keyed_answer_seconds),
            "egress.keyed_answer_seconds must be 1 through 3600",
        ),
    ];
    if let Some((_, message)) = bounds.into_iter().find(|(within, _)| !within) {
        return Err(GatewayError::Configuration(message));
    }
    if input.listen.port() == public.port() {
        return Err(GatewayError::Configuration(
            "egress.listen must differ from the public listener",
        ));
    }
    if input.disabled && !input.allow.is_empty() {
        return Err(GatewayError::Configuration(
            "egress.disabled turns egress off; remove its allow rules",
        ));
    }
    let internal_never = internal_never(&input.internal_never)?;
    if input.allow.len() > MAX_SEED_RULES {
        return Err(GatewayError::Configuration(
            "at most 256 egress rules may be configured",
        ));
    }
    let seed = input
        .allow
        .into_iter()
        .map(|rule| canonical(rule).map_err(|error| GatewayError::Configuration(error.message())))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Box::new(EgressConfig {
        listen: input.listen,
        disabled: input.disabled,
        seed,
        connections: input.connections,
        idle: std::time::Duration::from_secs(input.idle_seconds),
        keyed_connections: input.keyed_connections,
        keyed_per_sandbox: input.keyed_per_sandbox,
        keyed_answer: std::time::Duration::from_secs(input.keyed_answer_seconds),
        ca_pem: input.ca_file.as_deref().map(ca_file).transpose()?,
        internal_never,
    }))
}

/// `ca_file`: authority certificates in PEM (ADR-0037 §4).
///
/// # Errors
/// An unreadable, oversized or non-authority file.
fn ca_file(path: &std::path::Path) -> Result<std::sync::Arc<Vec<u8>>, GatewayError> {
    super::ca_bundle::read(
        path,
        "egress.ca_file must be a readable PEM file of authority certificates up to 1 MiB",
    )
}

/// `internal_never`: at most 64 addresses or networks.
///
/// # Errors
/// Anything else, or more of them.
fn internal_never(input: &[String]) -> Result<Vec<crate::egress::internal::Network>, GatewayError> {
    input
        .iter()
        .map(|text| crate::egress::internal::Network::parse(text.trim()))
        .collect::<Option<Vec<_>>>()
        .filter(|networks| networks.len() <= crate::egress::internal::MAX_NEVER)
        .ok_or(GatewayError::Configuration(
            "egress.internal_never must be at most 64 addresses or networks (a.b.c.d, a.b.c.d/n, IPv6 likewise)",
        ))
}

#[cfg(test)]
#[path = "egress_tests.rs"]
mod tests;
