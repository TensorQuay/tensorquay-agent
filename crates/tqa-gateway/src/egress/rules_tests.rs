use super::{AllowList, Decision, RuleInput, guarded};
use std::net::IpAddr;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn list(rules: &[(&str, &[u16])]) -> Result<AllowList, crate::GatewayError> {
    AllowList::new(
        rules
            .iter()
            .map(|(host, ports)| RuleInput {
                host: (*host).into(),
                ports: ports.to_vec(),
                internal: false,
            })
            .collect(),
    )
}

#[test]
fn gw_i_012_hosts_wildcards_and_ports_decide_exactly_and_default_deny() -> TestResult {
    let allow = list(&[
        ("pypi.org", &[443]),
        ("*.github.com", &[443, 80]),
        ("10.0.0.5", &[8080]),
    ])?;
    let cases = [
        ("pypi.org", 443, true),
        ("PYPI.ORG.", 443, true),
        ("pypi.org", 80, false),
        ("files.pypi.org", 443, false),
        ("api.github.com", 443, true),
        ("a.b.github.com", 80, true),
        ("github.com", 443, false),
        ("evilgithub.com", 443, false),
        ("github.com.evil.example", 443, false),
        ("10.0.0.5", 8080, true),
        ("10.0.0.5", 443, false),
        ("example.org", 443, false),
    ];
    for (host, port, allowed) in cases {
        let decision = allow.decide(host, port);
        assert_eq!(
            matches!(decision, Decision::Allowed { .. }),
            allowed,
            "{host}:{port}"
        );
    }
    assert_eq!(list(&[])?.decide("pypi.org", 443), Decision::Denied);
    Ok(())
}

#[test]
fn gw_i_012_invalid_rules_are_refused() {
    let invalid: &[(&str, &[u16])] = &[
        ("", &[443]),
        ("*", &[443]),
        ("*.", &[443]),
        ("a.*.example", &[443]),
        ("*example.org", &[443]),
        ("user@example.org", &[443]),
        ("example.org/path", &[443]),
        ("example.org", &[]),
        ("example.org", &[0]),
        ("example.org", &[443, 443]),
        (
            "example.org",
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17],
        ),
    ];
    for (host, ports) in invalid {
        assert!(list(&[(host, ports)]).is_err(), "{host} {ports:?}");
    }
    let many: Vec<(&str, &[u16])> = vec![("example.org", &[443]); 257];
    assert!(list(&many).is_err(), "the rule list is bounded");
}

#[test]
fn gw_i_012_reserved_destinations_need_an_ip_literal_rule() -> TestResult {
    let reserved: [IpAddr; 14] = [
        "127.0.0.1".parse()?,
        "10.1.2.3".parse()?,
        "172.16.0.1".parse()?,
        "192.168.1.1".parse()?,
        "169.254.169.254".parse()?,
        "100.64.0.1".parse()?,
        "0.0.0.0".parse()?,
        "::1".parse()?,
        "fd00::1".parse()?,
        "::ffff:10.0.0.1".parse()?,
        // IPv6 forms that embed IPv4: NAT64, 6to4 and IPv4-compatible.
        "64:ff9b::a00:5".parse()?,
        "2002:a00:5::1".parse()?,
        "::10.0.0.5".parse()?,
        "2002:a9fe:a9fe::1".parse()?,
    ];
    for address in reserved {
        assert!(
            guarded(address, false),
            "{address} blocked unless an IP-literal rule names it"
        );
        assert!(
            !guarded(address, true),
            "{address} allowed for its IP-literal rule"
        );
    }
    for public in [
        "93.184.215.14",
        "2606:2800:21f:cb07:6820:80da:af6b:8b2c",
        "64:ff9b::5db8:d70e",
        "2002:5db8:d70e::1",
    ] {
        assert!(!guarded(public.parse()?, false), "{public}");
    }
    // An exact host name never exempts a reserved answer: its DNS may be poisoned.
    let allow = list(&[
        ("*.internal.example", &[443]),
        ("db.internal.example", &[5432]),
        ("10.0.0.5", &[5432]),
    ])?;
    assert_eq!(
        allow.decide("x.internal.example", 443),
        Decision::Allowed { ip_literal: false }
    );
    assert_eq!(
        allow.decide("db.internal.example", 5432),
        Decision::Allowed { ip_literal: false }
    );
    assert_eq!(
        allow.decide("10.0.0.5", 5432),
        Decision::Allowed { ip_literal: true }
    );
    Ok(())
}

// TEST: GW-I-015
/// GW-I-061 (ADR-0028): one validator gives the file and the API the same typed refusal, and one canonical form.
#[test]
fn gw_i_061_rules_have_a_canonical_form_and_typed_refusals() -> TestResult {
    use super::{RuleError, canonical};
    let rule = |host: &str, ports: &[u16]| RuleInput {
        host: host.into(),
        ports: ports.to_vec(),
        internal: false,
    };
    let normal = canonical(rule("API.Example.ORG.", &[8443, 443])).map_err(RuleError::message)?;
    assert_eq!(
        (normal.host.as_str(), normal.ports.as_slice()),
        ("api.example.org", &[443, 8443][..])
    );
    assert_eq!(
        canonical(rule("*.Example.org", &[443]))
            .map_err(RuleError::message)?
            .host,
        "*.example.org"
    );
    for (host, ports, error) in [
        ("", &[443][..], RuleError::Host),
        ("user@example.org", &[443], RuleError::Host),
        ("*", &[443], RuleError::Wildcard),
        ("*.", &[443], RuleError::Wildcard),
        ("*.10.0.0.1", &[443], RuleError::Wildcard),
        ("*.-bad.example.org", &[443], RuleError::Wildcard),
        ("example.org", &[], RuleError::Ports),
        ("example.org", &[0], RuleError::Ports),
        ("example.org", &[443, 443], RuleError::Ports),
    ] {
        assert_eq!(
            canonical(rule(host, ports)).err(),
            Some(error),
            "{host} {ports:?}"
        );
    }
    Ok(())
}

// TEST: GW-I-015
#[test]
fn gw_i_061_each_rule_error_is_the_api_refusal_of_the_same_kind() {
    use super::RuleError;
    use crate::{GatewayError, InvalidReason};
    for (error, field, reason) in [
        (RuleError::Host, "host", InvalidReason::HostInvalid),
        (RuleError::Wildcard, "host", InvalidReason::WildcardInvalid),
        (RuleError::Ports, "ports", InvalidReason::PortsInvalid),
    ] {
        assert!(
            matches!(error.refusal(), GatewayError::Invalid { field: got, reason: got_reason } if got == field && got_reason == reason),
            "{error:?}"
        );
    }
}

#[test]
fn gw_i_012_a_wildcard_needs_a_whole_label_before_its_domain() -> TestResult {
    let list = AllowList::new(vec![RuleInput {
        host: "*.example.org".into(),
        ports: vec![443],
        internal: false,
    }])?;
    let allowed = Decision::Allowed { ip_literal: false };
    assert_eq!(list.decide("a.example.org", 443), allowed);
    assert_eq!(list.decide("a.b.example.org", 443), allowed);
    for host in [
        "example.org",
        ".example.org",
        "aexample.org",
        "xample.org",
        "org",
    ] {
        assert_eq!(list.decide(host, 443), Decision::Denied, "{host}");
    }
    Ok(())
}

// TEST: GW-I-015
#[test]
fn gw_i_061_each_rule_error_keeps_the_file_loaders_message() {
    use super::RuleError;
    assert_eq!(
        RuleError::Host.message(),
        "an egress host must be an exact name or address"
    );
    assert_eq!(
        RuleError::Wildcard.message(),
        "an egress wildcard must be `*.` followed by a domain"
    );
    assert_eq!(
        RuleError::Ports.message(),
        "egress ports must be 1 through 16 distinct non-zero ports"
    );
}

#[test]
fn gw_i_012_every_reserved_range_is_guarded_to_its_boundary() -> TestResult {
    // Each reserved range at its edges, and for IPv4 the public address just outside it (ARCHITECTURE §4.2). IPv6
    // neighbours of fc00::/7 and fe80::/10 are left out on purpose: whether deprecated site-local
    // fec0::/10 should be guarded is an open question, and this test must not settle it.
    let reserved = [
        "224.0.0.1",
        "239.255.255.255",
        "255.255.255.255",
        "0.1.2.3",
        "0.255.255.255",
        "100.64.0.0",
        "100.127.255.255",
        "::",
        "ff02::1",
        "fe80::1",
        "febf::1",
        "fc00::1",
        "fdff::1",
    ];
    for address in reserved {
        assert!(guarded(address.parse()?, false), "{address} is reserved");
    }
    let public = [
        "1.0.0.1",
        "100.63.255.255",
        "100.128.0.0",
        "223.255.255.255",
    ];
    for address in public {
        assert!(!guarded(address.parse()?, false), "{address} is public");
    }
    Ok(())
}

#[test]
fn gw_i_012_a_list_over_256_rules_is_refused_with_its_message() {
    let rules = (0..257)
        .map(|index| RuleInput {
            host: format!("h{index}.example.org"),
            ports: vec![443],
            internal: false,
        })
        .collect();
    assert!(matches!(
        AllowList::new(rules),
        Err(crate::GatewayError::Configuration(
            "at most 256 egress rules may be configured"
        ))
    ));
}

#[test]
fn gw_i_012_special_purpose_ranges_are_guarded_also_when_embedded_in_ipv6() -> TestResult {
    // IANA special-purpose ranges added to the guard (default deny): benchmarking 198.18.0.0/15, reserved 240.0.0.0/4,
    // IETF protocol assignments 192.0.0.0/24, 6to4 relay anycast 192.88.99.0/24 and site-local fec0::/10.
    let reserved = [
        "198.18.0.0",
        "198.19.255.255",
        "240.0.0.0",
        "254.255.255.255",
        "192.0.0.0",
        "192.0.0.255",
        "192.88.99.0",
        "192.88.99.255",
        "fec0::1",
        "feff::1",
        // The same IPv4 ranges inside IPv6: mapped, NAT64 and 6to4 are judged by the IPv4 rules.
        "::ffff:198.18.0.1",
        "64:ff9b::c612:1",
        "2002:c612:1::1",
        "::ffff:240.0.0.1",
        "64:ff9b::c000:1",
        "2002:c058:6301::1",
    ];
    for address in reserved {
        assert!(guarded(address.parse()?, false), "{address} is reserved");
        assert!(
            !guarded(address.parse()?, true),
            "{address} is allowed for its own IP-literal rule"
        );
    }
    let public = [
        "198.17.255.255",
        "198.20.0.0",
        "191.255.255.255",
        "192.0.1.0",
        "192.88.98.255",
        "192.88.100.0",
    ];
    for address in public {
        assert!(!guarded(address.parse()?, false), "{address} is public");
    }
    Ok(())
}

// TEST: GW-I-015
#[test]
fn gw_i_061_a_rule_takes_up_to_16_ports() -> TestResult {
    use super::canonical;
    let sixteen: Vec<u16> = (1..=16).collect();
    let rule = canonical(RuleInput {
        host: "example.org".into(),
        ports: sixteen.clone(),
        internal: false,
    })
    .map_err(super::RuleError::message)?;
    assert_eq!(rule.ports, sixteen, "exactly the bound is accepted");
    let seventeen: Vec<u16> = (1..=17).collect();
    assert_eq!(
        canonical(RuleInput {
            host: "example.org".into(),
            ports: seventeen,
            internal: false,
        })
        .err(),
        Some(super::RuleError::Ports)
    );
    Ok(())
}

/// GW-I-133 (the list's part): the prompt's allow-list is the list's host names in its order, at most 32, with
/// address rules and the rest counted in `more`; egress turned off sends an empty list (ADR-0035 §3).
#[test]
fn gw_i_133_the_snapshot_lists_host_names_in_order_and_counts_the_rest() -> TestResult {
    let allow = list(&[
        ("api.example.com", &[443]),
        ("10.0.0.5", &[8080]),
        ("*.pypi.org", &[443, 80]),
        ("fd00::1", &[443]),
    ])?;
    let snapshot = allow.snapshot(false);
    let hosts: Vec<_> = snapshot
        .allowed
        .iter()
        .map(|entry| (entry.host.as_str(), entry.ports.clone()))
        .collect();
    assert_eq!(
        hosts,
        [
            ("api.example.com", vec![443]),
            ("*.pypi.org", vec![80, 443])
        ]
    );
    assert_eq!(
        snapshot.more, 2,
        "the two address rules are counted, not shown"
    );
    assert_eq!(allow.snapshot(true), tqa_types::NetworkSnapshot::default());
    let names: Vec<String> = (0..40)
        .map(|index| format!("h{index:02}.example.com"))
        .collect();
    let many = list(
        &names
            .iter()
            .map(|name| (name.as_str(), &[443][..]))
            .collect::<Vec<_>>(),
    )?;
    let snapshot = many.snapshot(false);
    assert_eq!(snapshot.allowed.len(), tqa_types::MAX_NETWORK_ENTRIES);
    assert_eq!(snapshot.allowed[0].host, "h00.example.com");
    assert_eq!(snapshot.more, 8);
    // What the gateway builds is what the protocol accepts.
    let value = serde_json::to_value(&snapshot)?;
    assert_eq!(
        serde_json::from_value::<tqa_types::NetworkSnapshot>(value)?,
        snapshot
    );
    Ok(())
}

// GW-U-097: the any-host pattern ('*', the open_web preset). A typed rule and the file refuse it; only the stored list
// (where only a preset can have put it) holds it. It admits any name on its ports, and never makes a reserved address
// reachable: a destination it admits is never an exact-address match, so the reserved check always applies.
#[test]
fn gw_u_097_any_host_admits_names_on_its_ports_and_never_a_reserved_address() -> TestResult {
    let any = RuleInput {
        host: super::ANY_HOST.into(),
        ports: vec![80, 443],
        internal: false,
    };
    assert!(
        super::canonical_host("*").is_err(),
        "typed rules never hold '*'"
    );
    assert!(
        AllowList::new(vec![any.clone()]).is_err(),
        "the file never holds '*'"
    );
    let allow = AllowList::stored(vec![
        any,
        RuleInput {
            host: "10.0.0.5".into(),
            ports: vec![8080],
            internal: false,
        },
    ])?;
    for host in [
        "example.org",
        "news.example.com",
        "93.184.215.14",
        "10.0.0.5",
    ] {
        assert_eq!(
            allow.decide(host, 443),
            Decision::Allowed { ip_literal: false },
            "{host}"
        );
    }
    assert_eq!(allow.decide("example.org", 22), Decision::Denied);
    // An admitted name or address that is private is still refused; only the exact address rule reaches one.
    let private: IpAddr = "10.0.0.5".parse()?;
    assert!(guarded(private, false));
    assert_eq!(
        allow.decide("10.0.0.5", 8080),
        Decision::Allowed { ip_literal: true }
    );
    // The model is told the web is open on those ports, never an address.
    let snapshot = allow.snapshot(false);
    assert_eq!(snapshot.allowed.len(), 1);
    assert_eq!(snapshot.allowed[0].host, "*");
    assert_eq!(snapshot.allowed[0].ports, [80, 443]);
    assert_eq!(snapshot.more, 1);
    Ok(())
}

// TEST: GW-U-115
#[test]
fn gw_u_115_an_ip_rule_admits_its_one_address_however_either_side_writes_it() -> TestResult {
    // Stored as typed (the stored list keeps old rows), compared in canonical form.
    let allow = AllowList::stored(vec![
        RuleInput {
            host: "fd00:0::5".into(),
            ports: vec![8443],
            internal: false,
        },
        RuleInput {
            host: "192.0.2.10".into(),
            ports: vec![443],
            internal: false,
        },
    ])?;
    let exact = Decision::Allowed { ip_literal: true };
    for host in ["fd00::5", "FD00::5", "fd00:0:0::5", "fd00:0000::0005"] {
        assert_eq!(allow.decide(host, 8443), exact, "{host}");
    }
    assert_eq!(allow.decide("192.0.2.10", 443), exact);
    // Nothing else: another address, another port, or a name.
    for (host, port) in [
        ("fd00::6", 8443),
        ("fd00::5", 443),
        ("192.0.2.11", 443),
        ("fd00.example", 8443),
    ] {
        assert_eq!(allow.decide(host, port), Decision::Denied, "{host}:{port}");
    }
    assert_eq!(super::normalized("FD00:0::5"), "fd00::5");
    assert_eq!(super::normalized("Example.ORG."), "example.org");
    Ok(())
}

#[test]
fn gw_i_012_a_list_of_exactly_256_rules_is_accepted_and_an_empty_one_admits_nothing() -> TestResult
{
    let rules = (0..256)
        .map(|index| RuleInput {
            host: format!("h{index}.example.org"),
            ports: vec![443],
            internal: false,
        })
        .collect();
    let full = AllowList::new(rules)?;
    assert!(!full.is_empty());
    assert_eq!(
        full.decide("h255.example.org", 443),
        Decision::Allowed { ip_literal: false }
    );
    assert!(AllowList::new(Vec::new())?.is_empty());
    Ok(())
}

#[test]
fn sec_053_only_an_internal_rule_for_that_name_and_port_reaches_a_private_address() -> TestResult {
    let allow = AllowList::new(vec![
        RuleInput {
            host: "db.corp.example".into(),
            ports: vec![5432],
            internal: true,
        },
        RuleInput {
            host: "web.corp.example".into(),
            ports: vec![443],
            internal: false,
        },
    ])?;
    assert!(allow.internal("db.corp.example", 5432));
    assert!(
        allow.internal("DB.Corp.Example.", 5432),
        "compared as decide compares"
    );
    // Not another port of the internal rule, not a rule without the switch, not another name.
    assert!(!allow.internal("db.corp.example", 5433));
    assert!(!allow.internal("web.corp.example", 443));
    assert!(!allow.internal("other.corp.example", 5432));
    Ok(())
}
