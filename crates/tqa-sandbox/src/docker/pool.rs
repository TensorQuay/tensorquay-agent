//! The dedicated subnet pool for per-sandbox networks (ADR-0021 amendment): every Docker sandbox gets its own internal
//! network with an explicit subnet from this pool, so sandboxes never share a network and Docker's default address
//! pools are never used.

use std::{collections::BTreeSet, fmt, net::Ipv4Addr};

use serde::{Deserialize, Serialize};

use crate::SandboxError;

/// The addresses one sandbox network uses: the network address, the bridge (Docker's gateway), the gateway container,
/// the sandbox, and the broadcast address.
pub const ADDRESSES_PER_SANDBOX: u32 = 5;
/// Each sandbox network's prefix length when none is configured: a /29 has 8 addresses, room for the 5 above, so the
/// default /16 pool holds 8192 subnets and the whole `max_live` range (at most 4096) fits with the headroom.
pub const DEFAULT_SUBNET_PREFIX: u8 = 29;
/// The pool when none is configured.
pub const DEFAULT_POOL: &str = "10.213.0.0/16";
/// The pool must hold this many subnets per live sandbox: a retiring sandbox's network may still exist while its
/// replacement starts, and orphans wait for the sweep.
pub const POOL_HEADROOM: u32 = 2;
/// The smallest subnet that holds [`ADDRESSES_PER_SANDBOX`].
pub const MAX_SUBNET_PREFIX: u8 = 29;
/// The largest subnet a sandbox is given.
const MIN_SUBNET_PREFIX: u8 = 24;
/// The largest pool accepted.
const MIN_POOL_PREFIX: u8 = 8;
/// Private and shared address space a pool may come from (RFC 1918 and RFC 6598).
const PRIVATE: [(Ipv4Addr, u8); 4] = [
    (Ipv4Addr::new(10, 0, 0, 0), 8),
    (Ipv4Addr::new(172, 16, 0, 0), 12),
    (Ipv4Addr::new(192, 168, 0, 0), 16),
    (Ipv4Addr::new(100, 64, 0, 0), 10),
];

/// An IPv4 network in CIDR form, with its host bits zero.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Cidr {
    network: Ipv4Addr,
    prefix: u8,
}

impl Cidr {
    /// Parses `a.b.c.d/n`; the address must be the network's first address.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let (address, prefix) = text.split_once('/')?;
        let prefix: u8 = prefix.parse().ok().filter(|prefix| *prefix <= 32)?;
        if prefix.to_string() != text[address.len() + 1..] {
            return None;
        }
        Self::new(address.parse().ok()?, prefix)
    }

    /// The network `address/prefix`, or `None` if host bits are set or the prefix is over 32.
    #[must_use]
    pub fn new(address: Ipv4Addr, prefix: u8) -> Option<Self> {
        let cidr = Self {
            network: address,
            prefix,
        };
        (prefix <= 32 && u32::from(address) & !cidr.mask() == 0).then_some(cidr)
    }

    const fn mask(self) -> u32 {
        match self.prefix {
            0 => 0,
            prefix => u32::MAX << (32 - prefix),
        }
    }

    fn first(self) -> u32 {
        u32::from(self.network)
    }

    fn last(self) -> u32 {
        // The host bits of `first` are zero, so this cannot overflow.
        self.first() + !self.mask()
    }

    /// Whether the two networks share any address.
    #[must_use]
    pub fn overlaps(self, other: Self) -> bool {
        self.first() <= other.last() && other.first() <= self.last()
    }

    fn contains(self, other: Self) -> bool {
        self.prefix <= other.prefix && self.first() <= other.first() && other.last() <= self.last()
    }

    /// The network's first address.
    #[must_use]
    pub const fn network(self) -> Ipv4Addr {
        self.network
    }

    /// The prefix length.
    #[must_use]
    pub const fn prefix(self) -> u8 {
        self.prefix
    }

    /// The bridge's address: the first usable one.
    #[must_use]
    pub fn bridge(self) -> Ipv4Addr {
        Ipv4Addr::from(self.first() + 1)
    }
}

impl fmt::Display for Cidr {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.network, self.prefix)
    }
}

/// The validated pool: where per-sandbox subnets come from and how large each is.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "PoolInput", into = "PoolInput")]
pub struct SubnetPool {
    cidr: Cidr,
    subnet_prefix: u8,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PoolInput {
    cidr: String,
    prefix: u8,
}

impl TryFrom<PoolInput> for SubnetPool {
    type Error = SandboxError;
    fn try_from(input: PoolInput) -> Result<Self, Self::Error> {
        Self::new(&input.cidr, input.prefix)
    }
}

impl From<SubnetPool> for PoolInput {
    fn from(pool: SubnetPool) -> Self {
        Self {
            cidr: pool.cidr.to_string(),
            prefix: pool.subnet_prefix,
        }
    }
}

impl Default for SubnetPool {
    fn default() -> Self {
        Self {
            cidr: Cidr {
                network: Ipv4Addr::new(10, 213, 0, 0),
                prefix: 16,
            },
            subnet_prefix: DEFAULT_SUBNET_PREFIX,
        }
    }
}

impl SubnetPool {
    /// A pool of `/subnet_prefix` networks carved from `cidr`.
    ///
    /// # Errors
    /// `InvalidSpec` unless `cidr` is an IPv4 network (host bits zero) inside private or shared address space, at most
    /// a /8 and no smaller than one subnet, and `subnet_prefix` is 24 through [`MAX_SUBNET_PREFIX`].
    pub fn new(cidr: &str, subnet_prefix: u8) -> Result<Self, SandboxError> {
        let invalid = |reason| Err(SandboxError::invalid("network_pool", reason));
        let Some(parsed) = Cidr::parse(cidr) else {
            return invalid(
                "the pool must be an IPv4 network such as 10.213.0.0/16, with host bits zero",
            );
        };
        if !(MIN_SUBNET_PREFIX..=MAX_SUBNET_PREFIX).contains(&subnet_prefix) {
            return invalid("the per-sandbox prefix must be 24 through 29");
        }
        if !(MIN_POOL_PREFIX..=subnet_prefix).contains(&parsed.prefix) {
            return invalid(
                "the pool must be at most a /8 and hold at least one per-sandbox subnet",
            );
        }
        if !PRIVATE
            .iter()
            .filter_map(|(address, prefix)| Cidr::new(*address, *prefix))
            .any(|private| private.contains(parsed))
        {
            return invalid(
                "the pool must be private (RFC 1918) or shared (100.64.0.0/10) address space",
            );
        }
        Ok(Self {
            cidr: parsed,
            subnet_prefix,
        })
    }

    /// The pool's network.
    #[must_use]
    pub const fn cidr(&self) -> Cidr {
        self.cidr
    }

    /// How many per-sandbox subnets the pool holds.
    #[must_use]
    pub const fn capacity(&self) -> u32 {
        1 << (self.subnet_prefix - self.cidr.prefix)
    }

    /// Checks the sizing rule against the most sandboxes that can be live at once.
    ///
    /// # Errors
    /// `InvalidSpec` naming both numbers when the pool holds fewer than `POOL_HEADROOM` subnets per live sandbox.
    pub fn check_capacity(&self, max_live: u32) -> Result<(), SandboxError> {
        let needed = u64::from(max_live) * u64::from(POOL_HEADROOM);
        if u64::from(self.capacity()) < needed {
            return Err(SandboxError::InvalidSpec {
                field: "network_pool",
                reason: format!(
                    "the pool holds {} subnets, but max_live {max_live} needs {needed}",
                    self.capacity()
                )
                .into(),
            });
        }
        Ok(())
    }

    fn subnet(self, index: u32) -> Cidr {
        Cidr {
            network: Ipv4Addr::from(self.cidr.first() + (index << (32 - self.subnet_prefix))),
            prefix: self.subnet_prefix,
        }
    }

    /// The subnet's index when it is exactly one of this pool's subnets.
    fn index(self, subnet: Cidr) -> Option<u32> {
        (subnet.prefix == self.subnet_prefix && self.cidr.contains(subnet))
            .then(|| (subnet.first() - self.cidr.first()) >> (32 - self.subnet_prefix))
    }

    /// The first of `networks` (Docker IPAM subnets or routes, as CIDR text) that overlaps the pool, other than the
    /// pool's own per-sandbox subnets listed in `own`. Unparseable entries (IPv6, malformed) never conflict.
    #[must_use]
    pub fn conflict<'a>(
        &self,
        networks: impl IntoIterator<Item = &'a str>,
        own: &BTreeSet<Cidr>,
    ) -> Option<Cidr> {
        networks
            .into_iter()
            .filter_map(Cidr::parse)
            .find(|network| !own.contains(network) && network.overlaps(self.cidr))
    }
}

/// Which of the pool's subnets are in use. One per backend instance, rebuilt at start from the labelled networks.
#[derive(Debug)]
pub struct Allocator {
    pool: SubnetPool,
    used: BTreeSet<u32>,
    /// Subnets Docker refused as overlapping something else; skipped until the next start.
    unusable: BTreeSet<u32>,
}

impl Allocator {
    /// An allocator with `in_use` already taken; entries that are not exactly one of the pool's subnets are ignored
    /// (a foreign network, or a pool change since they were made), and returned so the caller can report them.
    #[must_use]
    pub fn rebuild<'a>(
        pool: SubnetPool,
        in_use: impl IntoIterator<Item = &'a str>,
    ) -> (Self, Vec<String>) {
        let mut allocator = Self {
            pool,
            used: BTreeSet::new(),
            unusable: BTreeSet::new(),
        };
        let mut ignored = Vec::new();
        for text in in_use {
            match Cidr::parse(text).and_then(|cidr| pool.index(cidr)) {
                Some(index) => {
                    allocator.used.insert(index);
                }
                None => ignored.push(text.to_owned()),
            }
        }
        (allocator, ignored)
    }

    /// The lowest free subnet, now marked in use; `None` when the pool is exhausted.
    pub fn allocate(&mut self) -> Option<Cidr> {
        let index = (0..self.pool.capacity())
            .find(|index| !self.used.contains(index) && !self.unusable.contains(index))?;
        self.used.insert(index);
        Some(self.pool.subnet(index))
    }

    /// Returns a subnet whose network is confirmed gone. Anything not allocated from this pool is ignored.
    pub fn release(&mut self, subnet: Cidr) {
        if let Some(index) = self.pool.index(subnet) {
            self.used.remove(&index);
        }
    }

    /// Marks an allocated subnet that Docker refused as overlapping; it is not offered again until a restart.
    pub fn mark_unusable(&mut self, subnet: Cidr) {
        if let Some(index) = self.pool.index(subnet) {
            self.used.remove(&index);
            self.unusable.insert(index);
        }
    }

    /// Subnets still available.
    #[must_use]
    pub fn free(&self) -> u32 {
        let taken = self.used.union(&self.unusable).count();
        self.pool.capacity()
            - u32::try_from(taken)
                .unwrap_or(u32::MAX)
                .min(self.pool.capacity())
    }
}

/// The IPv4 destinations in `/proc/net/route` text as CIDRs. The kernel prints `Destination` and `Mask` as the raw
/// 32-bit words in host byte order, so their in-memory bytes are the address in network order. The default route and
/// malformed lines are skipped.
#[must_use]
pub fn routes(text: &str) -> Vec<String> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let word = |index: usize| {
                fields
                    .get(index)
                    .and_then(|field| u32::from_str_radix(field, 16).ok())
                    .map(u32::to_ne_bytes)
            };
            let address = Ipv4Addr::from(word(1)?);
            let mask = u32::from_be_bytes(word(7)?);
            // At most 32, so the conversion never fails; 0 (the default route) is skipped below.
            let prefix = u8::try_from(mask.leading_ones()).unwrap_or(0);
            (prefix > 0 && mask.count_ones() == mask.leading_ones())
                .then(|| Cidr::new(address, prefix).map(|cidr| cidr.to_string()))
                .flatten()
        })
        .collect()
}
