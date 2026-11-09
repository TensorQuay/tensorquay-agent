//! GW-U-114 (ADR-0037 §4): `[egress] ca_file` names authorities trusted beside the public roots for keyed hosts.
//! It must be a readable PEM file of authority certificates only, up to 1 MiB, or the start stops. Its old alias,
//! `[git_push] ca_file`, was removed with the Git relay (ADR-0037 §7): a file that still has the section stops the start,
//! naming the setting to use instead.
use crate::{GatewayConfig, GatewayError};
use std::path::Path;

const MESSAGE: &str =
    "egress.ca_file must be a readable PEM file of authority certificates up to 1 MiB";

fn load(extra: &str) -> Result<GatewayConfig, GatewayError> {
    let image = format!("sha256:{}", "a".repeat(64));
    GatewayConfig::from_toml(&format!(
        "listen='127.0.0.1:8080'\npublic_origin='http://localhost:8080'\nmodels_path='/etc/tqa/models.toml'\n\
         [storage]\nroot='/srv/tqa'\n[runtime]\nbackend='docker'\nimage='{image}'\n\
         [sandbox]\ngateway_url='http://tqa-gateway:8080'\n{extra}"
    ))
}

/// A certificate as PEM (an authority, or a server's own) and its key's PEM.
fn issued(ca: bool) -> Result<(String, String), Box<dyn std::error::Error>> {
    let key = rcgen::KeyPair::generate()?;
    let mut params = rcgen::CertificateParams::new(vec!["internal.example".to_owned()])?;
    if ca {
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    }
    Ok((params.self_signed(&key)?.pem(), key.serialize_pem()))
}

/// `text` written as `name` in `directory`, as a TOML string.
fn file(directory: &Path, name: &str, text: &str) -> Result<String, Box<dyn std::error::Error>> {
    let path = directory.join(name);
    std::fs::write(&path, text)?;
    Ok(serde_json::to_string(&path)?)
}

fn refused(result: &Result<GatewayConfig, GatewayError>) -> Option<&'static str> {
    match result {
        Err(GatewayError::Configuration(message)) => Some(*message),
        _ => None,
    }
}

// TEST: GW-U-114
#[test]
fn gw_u_114_egress_ca_file_is_a_bundle_of_roots_for_the_proxy()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let (first, _) = issued(true)?;
    let (second, _) = issued(true)?;
    let ca = file(
        directory.path(),
        "ca.pem",
        &format!("{first}\nsome words between\n{second}"),
    )?;
    let config = load(&format!("[egress]\nca_file={ca}\n"))?;
    let pem = config.egress.ca_pem.clone().ok_or("the bundle")?;
    assert!(config.warnings().is_empty());
    let mut store = rustls::RootCertStore::empty();
    assert!(super::ca_bundle::add_roots(&mut store, &pem).is_some());
    assert_eq!(store.len(), 2);
    // Unset: only the public roots.
    let plain = load("")?;
    assert!(plain.egress.ca_pem.is_none());
    Ok(())
}

// TEST: GW-U-114
#[test]
fn gw_u_114_the_removed_git_push_section_stops_the_start_naming_egress_ca_file()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let (authority, _) = issued(true)?;
    let ca = file(directory.path(), "ca.pem", &authority)?;
    for extra in [
        format!("[git_push]\nca_file={ca}\n"),
        format!("[egress]\nca_file={ca}\n[git_push]\nca_file={ca}\n"),
        "[git_push]\n".to_owned(),
    ] {
        assert_eq!(
            refused(&load(&extra)),
            Some(
                "gateway.toml: [git_push] was removed in this release (ADR-0037 §7): name its ca_file as [egress] \
                 ca_file instead, and delete [git_push]"
            ),
            "{extra}"
        );
    }
    Ok(())
}

// TEST: GW-U-114
#[test]
fn gw_u_114_a_bundle_that_is_missing_oversized_or_not_certificates_stops_the_start()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let missing = serde_json::to_string(&directory.path().join("absent.pem"))?;
    let empty = file(directory.path(), "empty.pem", "")?;
    let text = file(directory.path(), "text.pem", "not a certificate\n")?;
    let large = file(directory.path(), "large.pem", &"a".repeat(1024 * 1024 + 1))?;
    for path in [missing, empty, text, large] {
        assert_eq!(
            refused(&load(&format!("[egress]\nca_file={path}\n"))),
            Some(MESSAGE),
            "{path}"
        );
    }
    Ok(())
}

// TEST: GW-U-114
#[test]
fn gw_u_114_a_key_another_section_or_a_servers_own_certificate_stops_the_start()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let (authority, key) = issued(true)?;
    let (leaf, _) = issued(false)?;
    // Armour built here, so no such text sits in the source.
    let section = |label: &str| format!("-----BEGIN {label}-----\nAAAA\n-----END {label}-----\n");
    let cases = [
        ("key.pem", format!("{authority}{key}")),
        (
            "crl.pem",
            format!("{authority}{}", section(&["X509", "CRL"].join(" "))),
        ),
        (
            "unknown.pem",
            format!("{authority}{}", section("SOMETHING ELSE")),
        ),
        ("leaf.pem", leaf.clone()),
        ("mixed.pem", format!("{authority}{leaf}")),
    ];
    for (name, text) in cases {
        let path = file(directory.path(), name, &text)?;
        assert_eq!(
            refused(&load(&format!("[egress]\nca_file={path}\n"))),
            Some(MESSAGE),
            "{name}"
        );
    }
    Ok(())
}

// TEST: SEC-053
#[test]
fn sec_053_the_file_allows_internal_only_on_one_exact_name()
-> Result<(), Box<dyn std::error::Error>> {
    let rule = |host: &str, internal: &str| {
        format!("[[egress.allow]]\nhost='{host}'\nports=[443]\n{internal}")
    };
    let config = load(&format!(
        "{}{}",
        rule("git.corp.test", "internal=true\n"),
        rule("docs.corp.test", "")
    ))?;
    let internal: Vec<(String, bool)> = config
        .egress
        .seed
        .iter()
        .map(|rule| (rule.host.clone(), rule.internal))
        .collect();
    assert_eq!(
        internal,
        [
            ("git.corp.test".to_owned(), true),
            ("docs.corp.test".to_owned(), false)
        ],
        "off unless set"
    );
    for host in ["*.corp.test", "10.0.0.5", "fd00::5"] {
        assert_eq!(
            refused(&load(&rule(host, "internal=true\n"))),
            Some(
                "egress internal = true is only for one exact host name (not a wildcard or an address)"
            ),
            "{host}"
        );
    }
    Ok(())
}

// TEST: SEC-054
#[test]
fn sec_054_internal_never_is_addresses_or_networks_and_its_absence_is_warned()
-> Result<(), Box<dyn std::error::Error>> {
    let internal = "[[egress.allow]]\nhost='git.corp.test'\nports=[443]\ninternal=true\n";
    let warning = "egress rules marked internal refuse until [egress] internal_never names this machine's own \
                   addresses, unless the gateway runs in the host's network namespace (it then finds them itself)";
    // An internal rule without internal_never: the file is valid, and said to need it.
    let without = load(&format!("[egress]\n{internal}"))?;
    assert_eq!(without.warnings(), [warning]);
    let with = load(&format!(
        "[egress]\ninternal_never=['192.168.1.1', '192.168.4.0/27', 'fd00:1::/64']\n{internal}"
    ))?;
    assert!(with.warnings().is_empty());
    assert_eq!(with.egress.internal_never.len(), 3);
    // No internal rule: nothing to warn about.
    assert!(load("").map(|config| config.warnings().is_empty())?);
    let message = "egress.internal_never must be at most 64 addresses or networks (a.b.c.d, a.b.c.d/n, IPv6 likewise)";
    let many: Vec<String> = (0..65).map(|n| format!("'10.0.{n}.0/24'")).collect();
    for bad in [
        "['example.org']".to_owned(),
        "['10.0.0.0/33']".to_owned(),
        format!("[{}]", many.join(",")),
    ] {
        assert_eq!(
            refused(&load(&format!("[egress]\ninternal_never={bad}\n"))),
            Some(message),
            "{bad}"
        );
    }
    Ok(())
}
