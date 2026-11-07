//! SBX-U-003 (the pool's configuration and sizing) and SBX-U-004 (the allocator), ADR-0021 amendment.
use std::collections::BTreeSet;

use super::pool::{
    ADDRESSES_PER_SANDBOX, Allocator, Cidr, DEFAULT_POOL, DEFAULT_SUBNET_PREFIX, MAX_SUBNET_PREFIX,
    POOL_HEADROOM, SubnetPool, routes,
};
use crate::SandboxError;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn reason(result: Result<SubnetPool, SandboxError>) -> String {
    match result {
        Err(SandboxError::InvalidSpec {
            field: "network_pool",
            reason,
        }) => reason.into_owned(),
        other => format!("unexpected: {other:?}"),
    }
}

#[test]
fn sbx_u_003_the_default_pool_is_a_private_slash_16_of_slash_29s_for_the_whole_max_live_range()
-> TestResult {
    let pool = SubnetPool::default();
    assert_eq!(pool, SubnetPool::new(DEFAULT_POOL, 29)?);
    assert_eq!(pool.cidr().to_string(), "10.213.0.0/16");
    assert_eq!(pool.capacity(), 8192);
    pool.check_capacity(4096)?; // the largest max_live the gateway accepts
    Ok(())
}

#[test]
fn sbx_u_003_a_slash_29_is_the_smallest_subnet_with_room_for_one_sandbox() {
    // Network, bridge, gateway container, sandbox and broadcast.
    assert_eq!(ADDRESSES_PER_SANDBOX, 5);
    let addresses = |prefix: u8| 1_u32 << (32 - prefix);
    assert_eq!(addresses(MAX_SUBNET_PREFIX), 8);
    assert!(addresses(MAX_SUBNET_PREFIX) >= ADDRESSES_PER_SANDBOX);
    assert!(
        addresses(MAX_SUBNET_PREFIX + 1) < ADDRESSES_PER_SANDBOX,
        "a /30 is too small"
    );
    assert_eq!(DEFAULT_SUBNET_PREFIX, MAX_SUBNET_PREFIX);
}

#[test]
fn sbx_u_003_a_pool_must_be_a_private_ipv4_network_with_a_24_to_29_subnet_prefix() -> TestResult {
    for (cidr, prefix, expected) in [
        ("192.168.4.1/24", 28, "host bits zero"),
        ("10.213.0.0", 28, "host bits zero"),
        ("10.213.0.0/33", 28, "host bits zero"),
        ("10.213.0.0/016", 28, "host bits zero"),
        ("gateway/16", 28, "host bits zero"),
        ("fd00::/64", 28, "host bits zero"),
        ("10.213.0.0/16", 23, "24 through 29"),
        ("10.213.0.0/16", 30, "24 through 29"),
        ("0.0.0.0/7", 28, "at most a /8"),
        ("10.213.0.0/29", 28, "at least one"),
        ("203.0.113.0/24", 28, "private"),
        ("100.128.0.0/16", 28, "private"),
    ] {
        let reason = reason(SubnetPool::new(cidr, prefix));
        assert!(reason.contains(expected), "{cidr}/{prefix}: {reason}");
    }
    for (cidr, prefix, capacity) in [
        ("10.0.0.0/8", 24, 65_536),
        ("172.16.0.0/12", 29, 131_072),
        ("192.168.4.0/24", 24, 1),
        ("192.168.4.0/24", 29, 32),
        ("100.64.0.0/10", 28, 262_144),
    ] {
        assert_eq!(
            SubnetPool::new(cidr, prefix)?.capacity(),
            capacity,
            "{cidr}"
        );
    }
    Ok(())
}

#[test]
fn sbx_u_003_the_pool_must_hold_twice_max_live_and_says_by_how_much() -> TestResult {
    assert_eq!(POOL_HEADROOM, 2);
    let pool = SubnetPool::new("192.168.4.0/24", 29)?; // 32 subnets
    pool.check_capacity(16)?;
    let Err(SandboxError::InvalidSpec { field, reason }) = pool.check_capacity(17) else {
        return Err("17 live sandboxes fit 32 subnets".into());
    };
    assert_eq!(field, "network_pool");
    assert_eq!(
        reason,
        "the pool holds 32 subnets, but max_live 17 needs 34"
    );
    SubnetPool::default().check_capacity(4096)?;
    assert!(SubnetPool::default().check_capacity(u32::MAX).is_err());
    Ok(())
}

#[test]
fn sbx_u_003_the_pool_round_trips_through_its_configuration_form() -> TestResult {
    let pool: SubnetPool =
        serde_json::from_value(serde_json::json!({"cidr":"192.168.4.0/24","prefix":29}))?;
    assert_eq!(pool.capacity(), 32);
    assert_eq!(
        serde_json::to_value(pool)?,
        serde_json::json!({"cidr":"192.168.4.0/24","prefix":29})
    );
    assert!(
        serde_json::from_value::<SubnetPool>(
            serde_json::json!({"cidr":"203.0.113.0/24","prefix":28})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<SubnetPool>(
            serde_json::json!({"cidr":"192.168.4.0/24","prefix":29,"x":1})
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn sbx_u_003_conflicts_are_foreign_networks_and_routes_that_overlap_the_pool() -> TestResult {
    let pool = SubnetPool::default();
    let own = BTreeSet::from([Cidr::parse("10.213.0.0/29").ok_or("own")?]);
    let conflict = |networks: &[&str]| {
        pool.conflict(networks.iter().copied(), &own)
            .map(|cidr| cidr.to_string())
    };
    assert_eq!(
        conflict(&["203.0.113.0/24", "198.51.100.0/24", "fd00::/64", "junk"]),
        None
    );
    assert_eq!(
        conflict(&["10.213.0.0/29"]),
        None,
        "our own sandbox network"
    );
    assert_eq!(
        conflict(&["10.0.0.0/8"]),
        Some("10.0.0.0/8".into()),
        "a wider route"
    );
    assert_eq!(
        conflict(&["203.0.113.0/24", "10.213.0.0/16"]),
        Some("10.213.0.0/16".into()),
        "the whole pool, after an unrelated network"
    );
    assert_eq!(
        conflict(&["10.213.0.0/28"]),
        Some("10.213.0.0/28".into()),
        "inside, but not one of our subnets"
    );
    Ok(())
}

#[test]
fn sbx_u_003_proc_routes_become_cidrs_without_the_default_route() {
    // x86 `/proc/net/route`: host-order words. 192.168.4.0/24, 10.213.0.0/29, the default route, a garbled line and
    // a non-contiguous mask.
    let text = "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT\n\
        eth0\t0004A8C0\t00000000\t0001\t0\t0\t0\t00FFFFFF\t0\t0\t0\n\
        eth1\t0000D50A\t00000000\t0001\t0\t0\t0\tF8FFFFFF\t0\t0\t0\n\
        eth0\t00000000\t0104A8C0\t0003\t0\t0\t0\t00000000\t0\t0\t0\n\
        eth0\tzz\n\
        eth0\t0004A8C0\n\
        eth2\t0000000A\t00000000\t0001\t0\t0\t0\t00FF00FF\t0\t0\t0\n";
    if cfg!(target_endian = "little") {
        assert_eq!(routes(text), ["192.168.4.0/24", "10.213.0.0/29"]);
    }
}

#[test]
fn sbx_u_004_the_allocator_gives_the_lowest_free_subnet_and_reuses_released_ones() -> TestResult {
    let pool = SubnetPool::new("192.168.4.0/26", 28)?; // 4 subnets
    let (mut allocator, ignored) = Allocator::rebuild(pool, []);
    assert!(ignored.is_empty());
    assert_eq!(allocator.free(), 4);
    let first = allocator.allocate().ok_or("first")?;
    let second = allocator.allocate().ok_or("second")?;
    assert_eq!(
        (first.to_string(), second.to_string()),
        ("192.168.4.0/28".into(), "192.168.4.16/28".into())
    );
    assert_eq!(first.bridge().to_string(), "192.168.4.1");
    allocator.release(first);
    assert_eq!(allocator.allocate(), Some(first), "the lowest free again");
    assert_eq!(allocator.free(), 2);
    Ok(())
}

#[test]
fn sbx_u_004_an_exhausted_pool_allocates_nothing_until_a_release() -> TestResult {
    let pool = SubnetPool::new("192.168.4.0/27", 28)?; // 2 subnets
    let (mut allocator, _) = Allocator::rebuild(pool, []);
    let first = allocator.allocate().ok_or("first")?;
    allocator.allocate().ok_or("second")?;
    assert_eq!(allocator.allocate(), None);
    assert_eq!(allocator.free(), 0);
    allocator.release(first);
    assert_eq!(allocator.allocate(), Some(first));
    Ok(())
}

#[test]
fn sbx_u_004_a_subnet_docker_refused_is_skipped_until_restart() -> TestResult {
    let pool = SubnetPool::new("192.168.4.0/27", 28)?;
    let (mut allocator, _) = Allocator::rebuild(pool, []);
    let refused = allocator.allocate().ok_or("first")?;
    allocator.mark_unusable(refused);
    assert_eq!(allocator.free(), 1);
    let next = allocator.allocate().ok_or("next")?;
    assert_ne!(next, refused);
    allocator.release(next);
    assert_eq!(
        allocator.allocate(),
        Some(next),
        "the refused one is never offered"
    );
    assert_eq!(allocator.allocate(), None);
    // Releasing it (a network removed after all) does not bring it back before a restart.
    allocator.release(refused);
    assert_eq!(allocator.allocate(), None);
    assert_eq!(allocator.free(), 0);
    // A restart forgets it.
    let (mut restarted, _) = Allocator::rebuild(pool, []);
    assert_eq!(restarted.allocate(), Some(refused));
    Ok(())
}

#[test]
fn sbx_u_004_rebuilding_takes_the_pools_own_subnets_and_reports_the_rest() -> TestResult {
    let pool = SubnetPool::new("192.168.4.0/26", 28)?;
    let (mut allocator, ignored) = Allocator::rebuild(
        pool,
        [
            "192.168.4.0/28",
            "192.168.4.32/28",
            "192.168.4.16/29",
            "10.213.0.0/28",
            "192.168.4.1/28",
            "junk",
        ],
    );
    assert_eq!(
        ignored,
        ["192.168.4.16/29", "10.213.0.0/28", "192.168.4.1/28", "junk"]
    );
    assert_eq!(allocator.free(), 2);
    assert_eq!(
        allocator.allocate().map(|cidr| cidr.to_string()),
        Some("192.168.4.16/28".into())
    );
    assert_eq!(
        allocator.allocate().map(|cidr| cidr.to_string()),
        Some("192.168.4.48/28".into())
    );
    // Releasing or refusing something that is not the pool's changes nothing.
    allocator.release(Cidr::parse("10.213.0.0/28").ok_or("foreign")?);
    allocator.mark_unusable(Cidr::parse("10.213.0.0/28").ok_or("foreign")?);
    assert_eq!(allocator.free(), 0);
    Ok(())
}

#[test]
fn sbx_u_004_cidrs_overlap_by_address_range() -> TestResult {
    let parse = |text: &str| Cidr::parse(text).ok_or("cidr");
    assert!(parse("192.168.4.0/24")?.overlaps(parse("192.168.4.16/28")?));
    assert!(parse("192.168.4.16/28")?.overlaps(parse("192.168.4.0/24")?));
    assert!(
        !parse("192.168.4.0/28")?.overlaps(parse("192.168.4.16/28")?),
        "adjacent"
    );
    assert!(
        !parse("192.168.4.16/28")?.overlaps(parse("192.168.4.0/28")?),
        "adjacent"
    );
    assert!(parse("0.0.0.0/0")?.overlaps(parse("192.168.4.1/32")?));
    assert_eq!(
        parse("192.168.4.0/24")?.network().to_string(),
        "192.168.4.0"
    );
    assert_eq!(parse("192.168.4.0/24")?.prefix(), 24);
    Ok(())
}

#[test]
fn sbx_u_003_the_gateway_container_is_a_bounded_docker_name() -> TestResult {
    let mut config = super::DockerBackendConfig::new("tqa-unit", "/sandbox-storage")?;
    assert_eq!(config.gateway_container, None);
    assert_eq!(config.network_pool, SubnetPool::default());
    for valid in ["tqa-gateway", "g1", "a.b_c-d", &"x".repeat(128)] {
        config.gateway_container = Some(valid.into());
        config.validate()?;
    }
    for invalid in [
        "",
        "g",
        "-gateway",
        ".gateway",
        "gate way",
        "gate/way",
        &"x".repeat(129),
    ] {
        config.gateway_container = Some(invalid.into());
        assert!(
            matches!(
                config.validate(),
                Err(SandboxError::InvalidSpec {
                    field: "gateway_container",
                    ..
                })
            ),
            "{invalid:?}"
        );
    }
    let parsed: super::DockerBackendConfig = serde_json::from_value(serde_json::json!({
        "network_prefix": "tqa-unit", "storage_root": "/sandbox-storage",
        "operation_timeout": {"secs": 30, "nanos": 0}, "pull_timeout": {"secs": 120, "nanos": 0},
        "readiness_interval": {"secs": 0, "nanos": 100_000_000}, "max_containers": 4096,
        "test_command_override": null,
        "network_pool": {"cidr": "192.168.4.0/24", "prefix": 29}, "gateway_container": "tqa-gateway"
    }))?;
    assert_eq!(parsed.network_pool.capacity(), 32);
    assert_eq!(parsed.gateway_container.as_deref(), Some("tqa-gateway"));
    Ok(())
}
