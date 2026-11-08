//! The egress allow-list: exact hosts or `*.domain` subdomains, explicit ports, default deny (ARCHITECTURE §4.2).
use crate::{GatewayError, InvalidReason};
use serde::Deserialize;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

const MAX_RULES: usize = 256;
const MAX_PORTS: usize = 16;
const MAX_HOST_BYTES: usize = 253;

/// One configured rule before validation.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuleInput {
    pub host: String,
    pub ports: Vec<u16>,
    /// The administrator lets this exact host name reach a private address (ADR-0035 amendment): off unless set.
    #[serde(default)]
    pub internal: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Pattern {
    /// Any host (the `open_web` preset): never an exact-address match, so a reserved destination stays refused.
    Any,
    /// Exactly this host name or address.
    Exact(String),
    /// Any subdomain of this domain, never the domain itself.
    Subdomains(String),
}

#[derive(Clone, Debug)]
struct Rule {
    pattern: Pattern,
    ports: Vec<u16>,
    /// An exact host name an administrator lets reach a private address (ADR-0035 amendment).
    internal: bool,
}

/// Why a rule is refused; the file loader and the admin API report the same kind (ADR-0028: one validation path).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuleError {
    /// Not an exact host name or address.
    Host,
    /// A wildcard other than `*.` followed by a domain.
    Wildcard,
    /// Missing, zero, duplicate or more than 16 ports.
    Ports,
    /// `internal` on a rule that isn't one exact host name.
    Internal,
}

impl RuleError {
    /// The configuration message, as the file loader has always reported it.
    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::Host => "an egress host must be an exact name or address",
            Self::Wildcard => "an egress wildcard must be `*.` followed by a domain",
            Self::Ports => "egress ports must be 1 through 16 distinct non-zero ports",
            Self::Internal => {
                "egress internal = true is only for one exact host name (not a wildcard or an address)"
            }
        }
    }

    /// The API's refusal: the field and the closed reason for the same kind the file loader reports.
    pub(crate) const fn refusal(self) -> GatewayError {
        let (field, reason) = match self {
            Self::Host => ("host", InvalidReason::HostInvalid),
            Self::Wildcard => ("host", InvalidReason::WildcardInvalid),
            Self::Ports => ("ports", InvalidReason::PortsInvalid),
            Self::Internal => ("internal", InvalidReason::InternalNameOnly),
        };
        GatewayError::Invalid { field, reason }
    }
}

/// The outcome for one destination.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Decision {
    /// A rule admits the host and port. `ip_literal` is true when the matching rule is that exact IP address: the
    /// only way a reserved destination can be reached (a host name's DNS answers may be poisoned).
    Allowed { ip_literal: bool },
    /// No rule admits it.
    Denied,
}

/// A validated, bounded allow-list. An empty list denies everything.
#[derive(Clone, Debug, Default)]
pub(crate) struct AllowList(Vec<Rule>);

impl AllowList {
    /// Validates every rule.
    ///
    /// # Errors
    /// Rejects malformed hosts, bare or misplaced wildcards, missing, zero or duplicate ports, and unbounded lists.
    #[cfg(test)]
    pub(crate) fn new(inputs: Vec<RuleInput>) -> Result<Self, GatewayError> {
        Self::build(inputs, canonical)
    }

    /// The stored list, as the database holds it: the same rules, and the any-host rule only a preset can write
    /// ([`ANY_HOST`]); a typed rule and the file refuse it.
    ///
    /// # Errors
    /// As [`AllowList::new`].
    pub(crate) fn stored(inputs: Vec<RuleInput>) -> Result<Self, GatewayError> {
        Self::build(inputs, canonical_stored)
    }

    fn build(
        inputs: Vec<RuleInput>,
        check: fn(RuleInput) -> Result<RuleInput, RuleError>,
    ) -> Result<Self, GatewayError> {
        if inputs.len() > MAX_RULES {
            return Err(invalid("at most 256 egress rules may be configured"));
        }
        inputs
            .into_iter()
            .map(|input| check(input).map(|input| rule(&input)))
            .collect::<Result<_, _>>()
            .map(Self)
            .map_err(|error| invalid(error.message()))
    }

    /// The list as the prompt shows it (ADR-0035 §3): host names in the list's order, at most
    /// [`tqa_types::MAX_NETWORK_ENTRIES`]; address rules, which would tell the model provider the internal network's
    /// layout, and the rest past the bound are counted in `more`. Egress turned off is an empty list.
    pub(crate) fn snapshot(&self, disabled: bool) -> tqa_types::NetworkSnapshot {
        if disabled {
            return tqa_types::NetworkSnapshot::default();
        }
        let mut snapshot = tqa_types::NetworkSnapshot::default();
        for rule in &self.0 {
            let host = match &rule.pattern {
                Pattern::Any => ANY_HOST.to_owned(),
                Pattern::Exact(host) if host.parse::<IpAddr>().is_err() => host.clone(),
                Pattern::Subdomains(domain) => format!("*.{domain}"),
                Pattern::Exact(_) => {
                    snapshot.more += 1;
                    continue;
                }
            };
            if snapshot.allowed.len() == tqa_types::MAX_NETWORK_ENTRIES {
                snapshot.more += 1;
                continue;
            }
            snapshot.allowed.push(tqa_types::NetworkEntry {
                host,
                ports: rule.ports.clone(),
            });
        }
        snapshot
    }

    /// Whether no rule admits anything: a task reaches nothing outside.
    pub(crate) const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether the exact rule for `host` on `port` lets it reach a private address (ADR-0035 amendment): only a rule
    /// for that one host name with `internal` set; a wildcard, the any-host rule and an address never are.
    pub(crate) fn internal(&self, host: &str, port: u16) -> bool {
        let host = normalized(host);
        self.0.iter().any(|rule| {
            rule.internal
                && rule.ports.contains(&port)
                && matches!(&rule.pattern, Pattern::Exact(exact) if *exact == host)
        })
    }

    /// Decides a destination; the host is compared case-insensitively without a trailing dot.
    pub(crate) fn decide(&self, host: &str, port: u16) -> Decision {
        let host = normalized(host);
        let literal = host.parse::<IpAddr>().is_ok();
        let mut ip_literal = None;
        for rule in self.0.iter().filter(|rule| rule.ports.contains(&port)) {
            match &rule.pattern {
                Pattern::Any => {
                    ip_literal.get_or_insert(false);
                }
                Pattern::Exact(exact) if *exact == host => ip_literal = Some(literal),
                Pattern::Subdomains(domain)
                    if host.len() > domain.len() + 1
                        && host.ends_with(domain.as_str())
                        && host.as_bytes()[host.len() - domain.len() - 1] == b'.' =>
                {
                    ip_literal.get_or_insert(false);
                }
                _ => {}
            }
        }
        ip_literal.map_or(Decision::Denied, |ip_literal| Decision::Allowed {
            ip_literal,
        })
    }
}

/// Validates one rule and returns its canonical form: a lower-case host without a trailing dot, and sorted ports. The
/// database stores and compares rules in this form.
///
/// # Errors
/// The typed reason the rule is refused.
pub(crate) fn canonical(input: RuleInput) -> Result<RuleInput, RuleError> {
    let host = canonical_host(&input.host)?;
    let mut ports = input.ports;
    ports.sort_unstable();
    let unique = ports.windows(2).all(|pair| pair[0] != pair[1]);
    if ports.is_empty() || ports.len() > MAX_PORTS || ports.contains(&0) || !unique {
        return Err(RuleError::Ports);
    }
    if input.internal && (host.starts_with('*') || host.parse::<IpAddr>().is_ok()) {
        return Err(RuleError::Internal);
    }
    Ok(RuleInput {
        host,
        ports,
        internal: input.internal,
    })
}

/// The host of the any-host rule: stored only by the `open_web` preset, never accepted as a typed rule or in the file.
pub(crate) const ANY_HOST: &str = "*";

/// A stored rule's canonical form: as [`canonical`], and the any-host rule with its ports checked the same way.
fn canonical_stored(input: RuleInput) -> Result<RuleInput, RuleError> {
    if input.host == ANY_HOST {
        let ports = canonical(RuleInput {
            host: "any.invalid".to_owned(),
            ports: input.ports,
            internal: false,
        })?
        .ports;
        if input.internal {
            return Err(RuleError::Internal);
        }
        return Ok(RuleInput {
            host: ANY_HOST.to_owned(),
            ports,
            internal: false,
        });
    }
    canonical(input)
}

/// Validates a rule's host pattern and returns its canonical form: lower case, without a trailing dot.
///
/// # Errors
/// `Host` or `Wildcard`.
pub(crate) fn canonical_host(host: &str) -> Result<String, RuleError> {
    let host = normalized(host);
    match host.strip_prefix("*.") {
        Some(domain) if valid_name(domain) && domain.parse::<IpAddr>().is_err() => Ok(host),
        Some(_) => Err(RuleError::Wildcard),
        None if host == "*" => Err(RuleError::Wildcard),
        None if valid_name(&host) || host.parse::<IpAddr>().is_ok() => Ok(host),
        None => Err(RuleError::Host),
    }
}

/// A rule from its canonical form.
fn rule(input: &RuleInput) -> Rule {
    let pattern = match input.host.strip_prefix("*.") {
        _ if input.host == ANY_HOST => Pattern::Any,
        Some(domain) => Pattern::Subdomains(domain.to_owned()),
        // Stored as typed; compared in canonical form, so one address is one address however it was written.
        None => Pattern::Exact(normalized(&input.host)),
    };
    Rule {
        pattern,
        ports: input.ports.clone(),
        internal: input.internal,
    }
}

/// A host as rules compare it: lower case, without a trailing dot, and an IP address in its canonical form (an IPv6
/// address without brackets, zeros compressed), so `fd00:0::5` and `FD00::5` are the one address `fd00::5`.
pub(crate) fn normalized(host: &str) -> String {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    host.parse::<IpAddr>()
        .map_or(host, |address| address.to_string())
}

fn valid_name(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= MAX_HOST_BYTES
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

/// Whether a resolved destination must be refused: loopback, private, link-local (including cloud metadata),
/// shared-address, unique-local, unspecified, multicast and broadcast addresses, also when embedded in IPv6, unless
/// the matching rule is that exact IP literal. The gateway's own sandbox-network addresses are private, so covered.
pub(crate) fn guarded(address: IpAddr, ip_literal: bool) -> bool {
    !ip_literal && reserved(address)
}

pub(super) fn reserved(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(v4) => reserved_v4(v4),
        IpAddr::V6(v6) => embedded_v4(v6).map_or_else(|| reserved_v6(v6), reserved_v4),
    }
}

/// The IPv4 address an IPv6 address carries: mapped `::ffff:a.b.c.d`, IPv4-compatible `::a.b.c.d`,
/// NAT64 `64:ff9b::/96` and 6to4 `2002:AABB:CCDD::/48`.
fn embedded_v4(address: Ipv6Addr) -> Option<Ipv4Addr> {
    let segments = address.segments();
    let low = || Ipv4Addr::from((u32::from(segments[6]) << 16) | u32::from(segments[7]));
    if let Some(mapped) = address.to_ipv4_mapped() {
        return Some(mapped);
    }
    if segments[..6] == [0; 6] && !address.is_loopback() && !address.is_unspecified() {
        return Some(low());
    }
    if segments[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        return Some(low());
    }
    (segments[0] == 0x2002)
        .then(|| Ipv4Addr::from((u32::from(segments[1]) << 16) | u32::from(segments[2])))
}

/// IPv4 destinations no name-based rule may reach. Default deny for the IANA IPv4 Special-Purpose Address Registry
/// (RFC 6890) and the reserved blocks of the IPv4 Address Space Registry that no legitimate egress target uses.
fn reserved_v4(address: Ipv4Addr) -> bool {
    let [first, second, third, _] = address.octets();
    address.is_loopback()
        || address.is_private()
        || address.is_link_local()
        || address.is_unspecified()
        || address.is_multicast()
        // Shared address space 100.64.0.0/10 (RFC 6598) and "this network" 0.0.0.0/8 (RFC 791).
        || (first == 100 && (64..128).contains(&second))
        || first == 0
        // Benchmarking 198.18.0.0/15 (RFC 2544).
        || (first == 198 && (second == 18 || second == 19))
        // Reserved 240.0.0.0/4 (RFC 1112), which includes the limited broadcast 255.255.255.255.
        || first >= 240
        // IETF protocol assignments 192.0.0.0/24 (RFC 6890) and 6to4 relay anycast 192.88.99.0/24 (RFC 7526).
        || (first == 192 && second == 0 && third == 0)
        || (first == 192 && second == 88 && third == 99)
}

/// IPv6 destinations no name-based rule may reach, from the IANA IPv6 Special-Purpose Address Registry (RFC 6890);
/// IPv6 forms that carry an IPv4 address are judged by [`reserved_v4`] before this (see [`embedded_v4`]).
fn reserved_v6(address: Ipv6Addr) -> bool {
    let first = address.segments()[0];
    address.is_loopback()
        || address.is_unspecified()
        || address.is_multicast()
        // Unique local fc00::/7 (RFC 4193), link-local fe80::/10 (RFC 4291) and deprecated site-local fec0::/10
        // (RFC 3879), which older networks still route internally.
        || (first & 0xfe00) == 0xfc00
        || (first & 0xffc0) == 0xfe80
        || (first & 0xffc0) == 0xfec0
}

const fn invalid(message: &'static str) -> GatewayError {
    GatewayError::Configuration(message)
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod tests;
