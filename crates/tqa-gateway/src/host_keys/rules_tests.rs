//! GW-U-102 and GW-U-103 (ADR-0037 §1, Amendment 1): what a host key's address and name may be.
use super::{Address, RESERVED_NAMES, address, name};
use crate::InvalidReason;

// TEST: GW-U-102
#[test]
fn gw_u_102_an_address_is_one_https_host_with_an_optional_port_and_path() {
    let parsed = |text: &str| address(text).map_err(|(_, reason)| reason);
    assert_eq!(
        parsed("https://api.github.com"),
        Ok(Address {
            host: "api.github.com".into(),
            port: 443,
            path_prefix: None
        })
    );
    assert_eq!(
        parsed("https://GitHub.com:8443/acme/"),
        Ok(Address {
            host: "github.com".into(),
            port: 8443,
            path_prefix: Some("/acme".into())
        }),
        "the host lowercased, a trailing slash dropped"
    );
    assert_eq!(
        parsed("https://github.com/acme/tools.git"),
        Ok(Address {
            host: "github.com".into(),
            port: 443,
            path_prefix: Some("/acme/tools.git".into())
        })
    );
    assert_eq!(
        parsed("https://github.com/").map(|parsed| parsed.path_prefix),
        Ok(None)
    );
    // A single-label name, as an internal service may have, is a host name too.
    assert_eq!(
        parsed("https://gitea").map(|parsed| parsed.host),
        Ok("gitea".to_owned())
    );
}

// TEST: GW-U-102
#[test]
fn gw_u_102_anything_else_is_refused_with_its_reason() {
    let parsed = |text: &str| address(text).map_err(|(_, reason)| reason);
    for (text, reason) in [
        ("http://api.github.com", InvalidReason::AddressNotHttps),
        ("ftp://files.example", InvalidReason::AddressNotHttps),
        ("api.github.com", InvalidReason::AddressNotHttps),
        (
            "https://user:pass@api.github.com",
            InvalidReason::AddressNotPlain,
        ),
        (
            "https://user@api.github.com",
            InvalidReason::AddressNotPlain,
        ),
        (
            "https://api.github.com/?page=1",
            InvalidReason::AddressNotPlain,
        ),
        (
            "https://api.github.com/#top",
            InvalidReason::AddressNotPlain,
        ),
        ("https://*.github.com", InvalidReason::HostInvalid),
        ("https://", InvalidReason::AddressNotHttps),
        ("", InvalidReason::AddressNotHttps),
    ] {
        assert_eq!(parsed(text), Err(reason), "{text:?}");
    }
    let long = format!("https://example.com/{}", "a".repeat(600));
    assert_eq!(parsed(&long), Err(InvalidReason::AddressNotPlain));
}

// TEST: GW-U-103
#[test]
fn gw_u_103_a_name_is_an_environment_name_the_task_may_use() {
    for good in ["GITHUB_WORK", "A", "API2", &format!("K{}", "X".repeat(63))] {
        assert_eq!(name(good), Ok(()), "{good}");
    }
    for bad in [
        "",
        "github",
        "2FA",
        "GITHUB-WORK",
        "GITHUB WORK",
        "_X",
        &"X".repeat(65),
    ] {
        assert_eq!(name(bad), Err(InvalidReason::NameInvalid), "{bad:?}");
    }
    for reserved in [
        "PATH",
        "HOME",
        "HTTPS_PROXY",
        "SSL_CERT_FILE",
        "NODE_OPTIONS",
        "PYTHONPATH",
        "TQA_ANYTHING",
        "GIT_ASKPASS",
        "LD_PRELOAD",
        "LC_ALL",
    ] {
        assert_eq!(
            name(reserved),
            Err(InvalidReason::NameReserved),
            "{reserved}"
        );
    }
    // The list the API documents is the one the gateway checks.
    for listed in RESERVED_NAMES {
        assert_eq!(name(listed), Err(InvalidReason::NameReserved), "{listed}");
    }
}

// TEST: GW-U-102
#[test]
fn gw_u_102_an_ip_address_is_one_exact_address_in_canonical_form() {
    let parsed = |text: &str| {
        address(text)
            .map(|parsed| (parsed.host, parsed.port, parsed.path_prefix))
            .map_err(|(_, reason)| reason)
    };
    assert_eq!(
        parsed("https://192.0.2.10"),
        Ok(("192.0.2.10".to_owned(), 443, None))
    );
    assert_eq!(
        parsed("https://[2001:DB8:0::1]:8443/team/repo.git/"),
        Ok((
            "2001:db8::1".to_owned(),
            8443,
            Some("/team/repo.git".to_owned())
        ))
    );
    // Written another way, the same address (the URL standard's own reading of a numeric host).
    assert_eq!(
        parsed("https://3221225994"),
        Ok(("192.0.2.10".to_owned(), 443, None))
    );
    // An address is held to the path bound too.
    let long = format!("https://192.0.2.10/{}", "a".repeat(600));
    assert_eq!(parsed(&long), Err(InvalidReason::AddressNotPlain));
}
