//! Where a model route runs, from its address as written: no name is looked up, so what an administrator typed
//! decides. The one classification for every rule that depends on it: the model picker's second line
//! (`GET /api/v1/models`, `runs_on`) and the relay's local-model allowances (`relay::local_route`).
use serde::Serialize;
use std::net::{Ipv4Addr, Ipv6Addr};

/// Where a route's model runs.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Place {
    /// On the computer the gateway runs on: loopback, `localhost`, or the container host.
    ThisComputer,
    /// On another machine of the owner's network: a single-label host (a compose service), `*.local`, `*.internal`,
    /// or a private, link-local, shared (100.64.0.0/10, Tailscale) or unique-local address.
    Network,
    /// Anywhere else, an address that doesn't parse included.
    Hosted,
}

/// Well-known public model APIs, by exact host; anything else names no provider (extend by PR).
const PROVIDERS: [(&str, &str); 7] = [
    ("api.openai.com", "OpenAI"),
    ("api.anthropic.com", "Anthropic"),
    ("api.deepseek.com", "DeepSeek"),
    ("generativelanguage.googleapis.com", "Google"),
    ("openrouter.ai", "OpenRouter"),
    ("api.mistral.ai", "Mistral"),
    ("api.groq.com", "Groq"),
];

/// The address's host, a name without its trailing dot (the same name). The URL parser lowercases names.
fn host(base_url: &str) -> Option<url::Host> {
    match url::Url::parse(base_url).ok()?.host()?.to_owned() {
        url::Host::Domain(name) => Some(url::Host::Domain(name.trim_end_matches('.').to_owned())),
        other => Some(other),
    }
}

/// Where the route at `base_url` runs.
pub(crate) fn place(base_url: &str) -> Place {
    match host(base_url) {
        Some(url::Host::Domain(name)) => domain(&name),
        Some(url::Host::Ipv4(address)) => ipv4(address),
        Some(url::Host::Ipv6(address)) => ipv6(address),
        None => Place::Hosted,
    }
}

/// Whether the route's host is loopback as written: `localhost`, `*.localhost`, the 127/8 block or `::1` (an IPv4-mapped
/// loopback included). The container host's own names are not: from the gateway's container, loopback is the container
/// itself, which the route check says (`container_host`).
pub(crate) fn loopback(base_url: &str) -> bool {
    match host(base_url) {
        Some(url::Host::Domain(name)) => name == "localhost" || name.ends_with(".localhost"),
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address
            .to_ipv4_mapped()
            .map_or(address.is_loopback(), |mapped| mapped.is_loopback()),
        None => false,
    }
}

fn domain(name: &str) -> Place {
    // The container host's names end in .internal, but they are this computer: decided first.
    if name == "localhost"
        || name.ends_with(".localhost")
        || name == "host.docker.internal"
        || name == "host.containers.internal"
    {
        Place::ThisComputer
    } else if matches!(
        name.rsplit_once('.'),
        None | Some((_, "local" | "internal"))
    ) {
        Place::Network
    } else {
        Place::Hosted
    }
}

fn ipv4(address: Ipv4Addr) -> Place {
    if address.is_loopback() {
        Place::ThisComputer
    } else if address.is_private() || address.is_link_local() || shared(address) {
        Place::Network
    } else {
        Place::Hosted
    }
}

/// The shared address space 100.64.0.0/10 (RFC 6598), which Tailscale gives the machines of a private network.
const fn shared(address: Ipv4Addr) -> bool {
    let [first, second, ..] = address.octets();
    first == 100 && second & 0b1100_0000 == 0b0100_0000
}

fn ipv6(address: Ipv6Addr) -> Place {
    // An IPv4 address written as IPv6 (::ffff:a.b.c.d) is that IPv4 address.
    if let Some(mapped) = address.to_ipv4_mapped() {
        return ipv4(mapped);
    }
    if address.is_loopback() {
        Place::ThisComputer
    } else if address.is_unique_local() || address.is_unicast_link_local() {
        Place::Network
    } else {
        Place::Hosted
    }
}

/// The provider's name for a well-known public API, by its exact host; `None` for any other address, so no internal
/// name is ever shown.
pub(crate) fn provider(base_url: &str) -> Option<&'static str> {
    let Some(url::Host::Domain(name)) = host(base_url) else {
        return None;
    };
    PROVIDERS
        .iter()
        .find(|(known, _)| name.eq_ignore_ascii_case(known))
        .map(|(_, provider)| *provider)
}

#[cfg(test)]
#[path = "route_place_tests.rs"]
mod tests;
