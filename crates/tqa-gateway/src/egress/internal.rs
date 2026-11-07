//! Internal rules (ADR-0035 amendment, the owner's decision of 1 Oct 2026): an administrator may let a rule for one exact
//! host name reach a private address, for a named server on the internal network. Such a rule still never reaches
//! loopback, link-local (cloud metadata), unspecified, multicast or broadcast addresses, nor anything the gateway itself
//! stands on: the container engine's networks and their gateways (the host's address on each), the sandbox pool, the
//! gateway's own addresses and the host's own interfaces. The host's LAN addresses can't be seen from inside a
//! container, so an installation that uses internal rules must name them in `[egress] internal_never`, unless the
//! gateway sees the host's interfaces itself (it holds an engine bridge's gateway address: the host's network
//! namespace). While either is missing, or the engine's networks are unknown or stale, internal rules refuse: fail
//! closed. Every address a connection resolves is checked, and the proxy connects only to that checked set, so a name
//! re-pointed between the check and the connection can't move it (DNS rebinding).
use super::refusal;
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::RwLock,
    time::{Duration, Instant},
};

/// How often the inventory is renewed, and how old it may be before internal rules refuse.
pub(crate) const REFRESH: Duration = Duration::from_mins(1);
const STALE: Duration = Duration::from_mins(3);
/// The most `internal_never` ranges a file may hold, and the most addresses one inventory keeps.
pub(crate) const MAX_NEVER: usize = 64;
const MAX_INVENTORY: usize = 4096;
/// The largest `/proc` table read, bounded.
const MAX_PROC_BYTES: u64 = 1024 * 1024;

/// An IPv4 or IPv6 network: an address and a prefix length.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Network {
    address: IpAddr,
    prefix: u8,
}

impl Network {
    /// `address/prefix`, or one address; `None` for anything else. Host bits may be set (an engine writes subnets as
    /// it likes); only the prefix counts.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let (address, prefix) = if let Some((address, prefix)) = text.split_once('/') {
            (address.parse::<IpAddr>().ok()?, prefix.parse::<u8>().ok()?)
        } else {
            let address = text.parse::<IpAddr>().ok()?;
            (address, if address.is_ipv4() { 32 } else { 128 })
        };
        let most = if address.is_ipv4() { 32 } else { 128 };
        (prefix <= most).then_some(Self { address, prefix })
    }

    /// Whether `address` is in this network (an IPv4 address written as IPv6 counts as that IPv4 address).
    pub(crate) fn contains(self, address: IpAddr) -> bool {
        match (self.address, unmapped(address)) {
            (IpAddr::V4(network), IpAddr::V4(address)) => {
                let mask = u32::MAX
                    .checked_shl(32 - u32::from(self.prefix))
                    .unwrap_or(0);
                u32::from(network) & mask == u32::from(address) & mask
            }
            (IpAddr::V6(network), IpAddr::V6(address)) => {
                let mask = u128::MAX
                    .checked_shl(128 - u32::from(self.prefix))
                    .unwrap_or(0);
                u128::from(network) & mask == u128::from(address) & mask
            }
            _ => false,
        }
    }
}

fn unmapped(address: IpAddr) -> IpAddr {
    match address {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(address, IpAddr::V4),
        IpAddr::V4(_) => address,
    }
}

/// The private ranges an internal rule may reach: RFC 1918, shared address space (RFC 6598) and IPv6 unique-local.
/// Every other reserved address stays refused.
fn private(address: IpAddr) -> bool {
    match unmapped(address) {
        IpAddr::V4(v4) => {
            let [first, second, ..] = v4.octets();
            v4.is_private() || (first == 100 && (64..128).contains(&second))
        }
        IpAddr::V6(v6) => (v6.segments()[0] & 0xfe00) == 0xfc00,
    }
}

/// What internal rules may not reach, renewed on [`REFRESH`].
#[derive(Clone, Debug)]
pub(crate) struct Inventory {
    never: Vec<Network>,
    /// The gateway holds an engine bridge's gateway address: it is in the host's network namespace and sees the host's
    /// interfaces, so they are in `never` and `internal_never` is optional.
    host_visible: bool,
    taken_at: Instant,
}

impl Inventory {
    /// From the engine's networks, the gateway's own addresses, the sandbox pool and `internal_never`.
    pub(crate) fn new(
        engine: &tqa_sandbox::EngineNetworks,
        own: &[IpAddr],
        fixed: &[Network],
        taken_at: Instant,
    ) -> Self {
        let mut never: Vec<Network> = fixed.to_vec();
        never.extend(
            engine
                .subnets
                .iter()
                .filter_map(|subnet| Network::parse(subnet)),
        );
        never.extend(
            engine
                .gateways
                .iter()
                .chain(own)
                .filter_map(|address| Network::parse(&address.to_string())),
        );
        never.truncate(MAX_INVENTORY);
        let host_visible = engine.gateways.iter().any(|gateway| own.contains(gateway));
        Self {
            never,
            host_visible,
            taken_at,
        }
    }
}

/// The guard's state: the configuration's fixed ranges and the latest inventory.
#[derive(Debug, Default)]
pub(crate) struct Guard {
    /// `internal_never` (as configured; empty means not set).
    configured: Vec<Network>,
    /// The sandbox pool and anything else the configuration says is the gateway's own.
    fixed: Vec<Network>,
    current: RwLock<Option<Inventory>>,
    /// Tests only: the gateway's own addresses, instead of `/proc` (which differs from host to host).
    own: RwLock<Option<Vec<IpAddr>>>,
}

impl Guard {
    pub(crate) fn new(configured: Vec<Network>, fixed: Vec<Network>) -> Self {
        Self {
            configured,
            fixed,
            current: RwLock::new(None),
            own: RwLock::new(None),
        }
    }

    /// The ranges that are never reached whatever the inventory: `internal_never` and the configuration's own.
    pub(crate) fn fixed(&self) -> Vec<Network> {
        self.configured.iter().chain(&self.fixed).copied().collect()
    }

    /// The gateway's own addresses: `/proc`'s, unless a test set them.
    pub(crate) fn own(&self) -> Vec<IpAddr> {
        self.own
            .read()
            .ok()
            .and_then(|own| own.clone())
            .unwrap_or_else(own_addresses)
    }

    /// Tests only (the `testing` feature): the gateway's own addresses to use instead of `/proc`'s.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn set_own(&self, own: Vec<IpAddr>) {
        if let Ok(mut current) = self.own.write() {
            *current = Some(own);
        }
    }

    /// Whether internal rules are usable, as the admin list says it: `None` when ready, else a closed code.
    pub(crate) fn state(&self, now: Instant) -> Option<&'static str> {
        match self.unready(now)? {
            refusal::INTERNAL_UNCONFIGURED => Some("internal_never_missing"),
            _ => Some("engine_unknown"),
        }
    }

    pub(crate) fn update(&self, inventory: Inventory) {
        if let Ok(mut current) = self.current.write() {
            *current = Some(inventory);
        }
    }

    /// Why internal rules refuse right now, or `None` when they may be used.
    pub(crate) fn unready(&self, now: Instant) -> Option<&'static str> {
        let current = self.current.read().ok()?.clone();
        let Some(inventory) = current else {
            return Some(refusal::INTERNAL_UNVERIFIED);
        };
        if now.saturating_duration_since(inventory.taken_at) > STALE {
            return Some(refusal::INTERNAL_UNVERIFIED);
        }
        (!inventory.host_visible && self.configured.is_empty())
            .then_some(refusal::INTERNAL_UNCONFIGURED)
    }

    /// Whether an internal rule's connection may go to every one of `addresses`; the refusal otherwise. One address
    /// outside what the rule may reach refuses the whole connection, never a filtered set.
    pub(crate) fn verdict(&self, addresses: &[IpAddr], now: Instant) -> Result<(), &'static str> {
        if let Some(reason) = self.unready(now) {
            return Err(reason);
        }
        let never = self
            .current
            .read()
            .ok()
            .and_then(|current| current.clone())
            .map(|inventory| inventory.never)
            .unwrap_or_default();
        for address in addresses {
            if super::rules::reserved(*address) && !private(*address) {
                return Err(refusal::PRIVATE);
            }
            if never.iter().any(|network| network.contains(*address)) {
                return Err(refusal::INTERNAL_FORBIDDEN);
            }
        }
        Ok(())
    }
}

/// The gateway's own addresses: the local IPv4 addresses in `/proc/net/fib_trie` (`/32 host LOCAL`) and every IPv6
/// address in `/proc/net/if_inet6`, read bounded. Unreadable tables give none (Linux only).
pub(crate) fn own_addresses() -> Vec<IpAddr> {
    let read = |path: &str| read_bounded(std::path::Path::new(path));
    let mut own: Vec<IpAddr> = local_v4(&read("/proc/net/fib_trie"))
        .into_iter()
        .map(IpAddr::V4)
        .collect();
    own.extend(
        local_v6(&read("/proc/net/if_inet6"))
            .into_iter()
            .map(IpAddr::V6),
    );
    own.sort_unstable();
    own.dedup();
    own.truncate(MAX_INVENTORY);
    own
}

/// A `/proc` table, at most [`MAX_PROC_BYTES`] of it; an unreadable one reads as empty.
fn read_bounded(path: &std::path::Path) -> String {
    use std::io::Read as _;
    let mut text = String::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(MAX_PROC_BYTES).read_to_string(&mut text))
        .map(|_| text)
        .unwrap_or_default()
}

/// The `|-- a.b.c.d` entries of a `fib_trie` table that a `/32 host LOCAL` line marks as this host's own.
fn local_v4(table: &str) -> Vec<Ipv4Addr> {
    let mut last = None;
    let mut found = Vec::new();
    for line in table.lines() {
        let line = line.trim();
        if let Some(address) = line.strip_prefix("|-- ") {
            last = address.parse::<Ipv4Addr>().ok();
        } else if line == "/32 host LOCAL" {
            found.extend(last);
        }
    }
    found.sort_unstable();
    found.dedup();
    found
}

/// The address at the start of each `if_inet6` line (32 hexadecimal digits).
fn local_v6(table: &str) -> Vec<Ipv6Addr> {
    table
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|hex| hex.len() == 32)
        .filter_map(|hex| u128::from_str_radix(hex, 16).ok())
        .map(Ipv6Addr::from)
        .collect()
}

#[cfg(test)]
#[path = "internal_tests.rs"]
mod tests;
