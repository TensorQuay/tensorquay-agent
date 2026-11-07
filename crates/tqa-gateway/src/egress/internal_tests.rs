//! SEC-050, SEC-051, SEC-054 (ADR-0035 amendment): what an internal rule may reach, the inventory, and the gateway's own
//! addresses read from `/proc`.
use super::{
    Guard, Inventory, MAX_PROC_BYTES, Network, STALE, local_v4, local_v6, own_addresses, private,
    read_bounded,
};
use crate::egress::refusal;
use std::{
    net::IpAddr,
    time::{Duration, Instant},
};

fn ip(text: &str) -> IpAddr {
    text.parse().unwrap_or(IpAddr::from([0, 0, 0, 0]))
}

fn net(text: &str) -> Network {
    Network::parse(text).unwrap_or(Network {
        address: ip("0.0.0.0"),
        prefix: 32,
    })
}

/// The engine: a bridge whose gateway is the host's address on it, a compose network, and an IPv6 network.
fn engine() -> tqa_sandbox::EngineNetworks {
    tqa_sandbox::EngineNetworks {
        subnets: vec![
            "192.168.4.0/27".into(),
            "172.16.0.0/16".into(),
            "fd00:18::/64".into(),
        ],
        gateways: vec![ip("192.168.4.1"), ip("172.16.0.1")],
    }
}

/// A guard with `internal_never` as given, the sandbox pool 10.213.0.0/16, and a fresh inventory from `own`.
fn guard(never: &[&str], own: &[&str], now: Instant) -> Guard {
    let guard = Guard::new(
        never.iter().map(|text| net(text)).collect(),
        vec![net("10.213.0.0/16")],
    );
    let own: Vec<IpAddr> = own.iter().map(|text| ip(text)).collect();
    guard.update(Inventory::new(&engine(), &own, &guard.fixed(), now));
    guard
}

// TEST: SEC-050
#[test]
fn sec_050_networks_parse_and_contain() {
    let contains =
        |network: &str, address: &str| Network::parse(network).map(|n| n.contains(ip(address)));
    assert_eq!(contains("10.0.0.0/8", "10.1.2.3"), Some(true));
    assert_eq!(contains("10.0.0.0/8", "100.64.0.1"), Some(false));
    assert_eq!(contains("192.168.1.1", "192.168.1.1"), Some(true));
    assert_eq!(contains("192.168.1.1", "192.168.4.1"), Some(false));
    assert_eq!(contains("fd00::/8", "fd12::1"), Some(true));
    assert_eq!(contains("0.0.0.0/0", "93.184.215.14"), Some(true));
    // An IPv4 address written as IPv6 is that address.
    assert_eq!(contains("172.16.0.0/16", "::ffff:172.16.0.1"), Some(true));
    for bad in [
        "10.0.0.0/33",
        "fd00::/129",
        "example.org",
        "10.0.0.0/x",
        "",
        "/8",
    ] {
        assert_eq!(Network::parse(bad), None, "{bad}");
    }
}

// TEST: SEC-050
#[test]
fn sec_050_an_internal_rule_reaches_private_ranges_and_nothing_the_gateway_stands_on() {
    let now = Instant::now();
    let guard = guard(&["192.168.1.1/32"], &["192.168.4.16"], now);
    for allowed in [
        "10.1.2.3",
        "10.0.0.5",
        "100.64.0.1",
        "fd12:3456::1",
        "93.184.215.14",
    ] {
        assert_eq!(guard.verdict(&[ip(allowed)], now), Ok(()), "{allowed}");
    }
    for (address, reason) in [
        ("192.168.4.0", refusal::INTERNAL_FORBIDDEN), // the engine's bridge
        ("192.168.4.1", refusal::INTERNAL_FORBIDDEN), // the host on that bridge
        ("192.168.4.16", refusal::INTERNAL_FORBIDDEN), // the gateway itself
        ("172.16.0.1", refusal::INTERNAL_FORBIDDEN),  // another engine network
        ("fd00:18::7", refusal::INTERNAL_FORBIDDEN),  // an engine network's IPv6 subnet
        ("10.213.0.0", refusal::INTERNAL_FORBIDDEN),  // the sandbox pool
        ("192.168.1.1", refusal::INTERNAL_FORBIDDEN), // the host's LAN address, from internal_never
        ("127.0.0.1", refusal::PRIVATE),
        ("169.254.169.254", refusal::PRIVATE), // cloud metadata
        ("0.0.0.0", refusal::PRIVATE),
        ("224.0.0.1", refusal::PRIVATE),
        ("255.255.255.255", refusal::PRIVATE),
        ("::1", refusal::PRIVATE),
        ("fe80::1", refusal::PRIVATE),
        ("::ffff:127.0.0.1", refusal::PRIVATE),
    ] {
        assert_eq!(guard.verdict(&[ip(address)], now), Err(reason), "{address}");
    }
}

// TEST: SEC-051
#[test]
fn sec_051_one_forbidden_address_refuses_the_whole_answer() {
    let now = Instant::now();
    let guard = guard(&["192.168.1.1"], &[], now);
    assert_eq!(
        guard.verdict(&[ip("10.1.2.3"), ip("10.0.0.5")], now),
        Ok(())
    );
    assert_eq!(
        guard.verdict(&[ip("10.1.2.3"), ip("192.168.4.1")], now),
        Err(refusal::INTERNAL_FORBIDDEN)
    );
    assert_eq!(
        guard.verdict(&[ip("10.1.2.3"), ip("127.0.0.1")], now),
        Err(refusal::PRIVATE)
    );
}

// TEST: SEC-054
#[test]
fn sec_054_internal_rules_refuse_until_the_host_is_known_and_the_inventory_fresh() {
    let now = Instant::now();
    // No inventory yet.
    let empty = Guard::new(vec![net("192.168.1.1")], Vec::new());
    assert_eq!(
        empty.verdict(&[ip("10.1.2.3")], now),
        Err(refusal::INTERNAL_UNVERIFIED)
    );
    // In a container (no bridge gateway of its own) and no internal_never: refused in words.
    let unset = guard(&[], &["192.168.4.16"], now);
    assert_eq!(unset.unready(now), Some(refusal::INTERNAL_UNCONFIGURED));
    assert_eq!(unset.state(now), Some("internal_never_missing"));
    assert_eq!(
        unset.verdict(&[ip("10.1.2.3")], now),
        Err(refusal::INTERNAL_UNCONFIGURED)
    );
    // In the host's namespace (it holds the bridge's gateway): its own addresses are the host's, internal_never optional.
    let host = guard(&[], &["192.168.4.1", "192.168.1.1"], now);
    assert_eq!(host.unready(now), None);
    assert_eq!(
        host.verdict(&[ip("192.168.1.1")], now),
        Err(refusal::INTERNAL_FORBIDDEN)
    );
    assert_eq!(host.verdict(&[ip("10.1.2.3")], now), Ok(()));
    // Stale after three minutes without a renewal.
    let later = now + Duration::from_secs(181);
    assert_eq!(
        host.verdict(&[ip("10.1.2.3")], later),
        Err(refusal::INTERNAL_UNVERIFIED)
    );
    assert_eq!(empty.state(now), Some("engine_unknown"));
}

// TEST: SEC-050
#[test]
fn sec_050_the_gateways_own_addresses_are_read_from_proc() {
    let fib = "Main:\n  +-- 0.0.0.0/0 3 0 5\n     |-- 0.0.0.0\n        /0 universe UNICAST\n     |-- 127.0.0.1\n\
               /32 host LOCAL\n     |-- 192.168.4.16\n        /32 link BROADCAST\n        /32 host LOCAL\n\
               |-- 192.168.4.48\n        /32 link BROADCAST\nLocal:\n     |-- 192.168.4.16\n        /32 host LOCAL\n";
    assert_eq!(
        local_v4(fib)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["127.0.0.1", "192.168.4.16"]
    );
    let inet6 = "00000000000000000000000000000001 01 80 10 80       lo\n\
                 fd000000000000180000000000000005 02 40 00 80     eth0\nshort 01\n";
    assert_eq!(
        local_v6(inet6)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["::1", "fd00:0:0:18::5"]
    );
    assert!(private(ip("10.0.0.1")) && private(ip("fd00::1")) && !private(ip("127.0.0.1")));
}

// TEST: SEC-050
#[test]
fn sec_050_shared_address_space_is_private_only_under_100() {
    // 100.64.0.0/10 is the only shared space: the same second octet under another first one is not private, and a
    // loopback address there stays refused to an internal rule.
    // Written as octets: a loopback and a public address in the 64-127 second-octet band.
    let loopback = IpAddr::from([127, 64, 0, 1]);
    for (address, expected) in [
        (ip("100.64.0.1"), true),
        (ip("100.127.255.255"), true),
        (ip("100.63.255.255"), false),
        (ip("100.128.0.0"), false),
        (loopback, false),
        (IpAddr::from([8, 64, 0, 1]), false),
    ] {
        assert_eq!(private(address), expected, "{address}");
    }
    let now = Instant::now();
    let host = guard(&[], &["192.168.4.1"], now);
    assert_eq!(host.verdict(&[loopback], now), Err(refusal::PRIVATE));
    assert_eq!(host.verdict(&[ip("100.64.0.1")], now), Ok(()));
}

// TEST: SEC-054
#[test]
fn sec_054_an_inventory_is_fresh_for_exactly_three_minutes() {
    let now = Instant::now();
    let host = guard(&[], &["192.168.4.1"], now);
    assert_eq!(host.unready(now + STALE), None);
    assert_eq!(
        host.unready(now + STALE + Duration::from_nanos(1)),
        Some(refusal::INTERNAL_UNVERIFIED)
    );
}

// TEST: SEC-050
#[test]
fn sec_050_the_own_addresses_a_test_sets_replace_procs() {
    let guard = Guard::new(Vec::new(), Vec::new());
    guard.set_own(vec![ip("192.0.2.7")]);
    assert_eq!(guard.own(), [ip("192.0.2.7")]);
}

// TEST: SEC-050
#[test]
fn sec_050_proc_tables_are_read_whole_up_to_their_bound() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = tempfile::tempdir()?;
    let small = dir.path().join("small");
    std::fs::write(&small, "x".repeat(4096))?;
    assert_eq!(read_bounded(&small).len(), 4096);
    let large = dir.path().join("large");
    let bound = usize::try_from(MAX_PROC_BYTES)?;
    std::fs::write(&large, "x".repeat(bound + 10))?;
    assert_eq!(read_bounded(&large).len(), bound);
    assert_eq!(read_bounded(&dir.path().join("missing")), "");
    // This host's own tables name at least its loopback address.
    if std::path::Path::new("/proc/net/fib_trie").exists() {
        assert!(own_addresses().contains(&ip("127.0.0.1")));
    }
    Ok(())
}
