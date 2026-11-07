//! GW-U-105 (ADR-0037 §6): the install CA is P-256 with its private key in a 0600 file beside the install key, made
//! once and read back unchanged; a leaf per keyed host names exactly that host, lasts 24 hours and is cached; a rotation
//! makes a new CA that issues from then on, while the previous one keeps issuing to sandboxes made before it for one
//! sandbox lifetime and stays in the bundle until then.
use super::{HostCa, rotate};
use rustls_pki_types::{CertificateDer, ServerName, UnixTime};
use std::{os::unix::fs::PermissionsExt, time::Duration};
use time::OffsetDateTime;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const LIFETIME: Duration = Duration::from_hours(8);

fn at(now: OffsetDateTime) -> Result<UnixTime, Box<dyn std::error::Error>> {
    Ok(UnixTime::since_unix_epoch(Duration::from_secs(
        u64::try_from(now.unix_timestamp())?,
    )))
}

/// Whether `leaf` chains to `ca` for `host` at `now`, as a TLS client checks it.
fn trusted(
    ca: &[u8],
    leaf: &[u8],
    host: &str,
    now: OffsetDateTime,
) -> Result<bool, Box<dyn std::error::Error>> {
    let ca = CertificateDer::from(ca.to_vec());
    let anchor = webpki::anchor_from_trusted_cert(&ca)?;
    let leaf = CertificateDer::from(leaf.to_vec());
    let end = webpki::EndEntityCert::try_from(&leaf)?;
    let chained = end
        .verify_for_usage(
            webpki::ALL_VERIFICATION_ALGS,
            &[anchor],
            &[],
            at(now)?,
            webpki::KeyUsage::server_auth(),
            None,
            None,
        )
        .is_ok();
    Ok(chained
        && end
            .verify_is_valid_for_subject_name(&ServerName::try_from(host.to_owned())?)
            .is_ok())
}

// TEST: GW-U-105
#[tokio::test]
async fn gw_u_105_the_install_ca_issues_a_leaf_per_host_and_rotates_on_demand() -> TestResult {
    let temporary = tempfile::tempdir()?;
    // The CA makes its own directory, private (0700).
    let directory = Private(temporary.path().join("ca"));
    let now = OffsetDateTime::now_utc();
    let first = HostCa::load(directory.path(), LIFETIME, now).await?;
    // P-256, the private key readable only by the gateway; loading again reads the same CA.
    assert!(first.is_p256());
    let key = std::fs::metadata(directory.path().join("host-ca-key.pem"))?;
    assert_eq!(key.permissions().mode() & 0o7777, 0o600);
    let again = HostCa::load(directory.path(), LIFETIME, now).await?;
    assert_eq!(again.bundle_pem(), first.bundle_pem());
    let certificates = first.certificates();
    let [ca] = certificates.as_slice() else {
        return Err("one CA before a rotation".into());
    };
    assert!(
        first
            .bundle_pem()
            .starts_with("-----BEGIN CERTIFICATE-----")
    );

    // A leaf names exactly its host, for 24 hours; the same host gets the cached one.
    let leaf = first.leaf("api.example.com", now, now)?;
    assert!(trusted(ca, leaf.certificate(), "api.example.com", now)?);
    assert!(!trusted(ca, leaf.certificate(), "other.example.com", now)?);
    assert!(!trusted(
        ca,
        leaf.certificate(),
        "sub.api.example.com",
        now
    )?);
    assert!(trusted(
        ca,
        leaf.certificate(),
        "api.example.com",
        now + time::Duration::hours(23)
    )?);
    assert!(!trusted(
        ca,
        leaf.certificate(),
        "api.example.com",
        now + time::Duration::hours(25)
    )?);
    assert_eq!(
        first.leaf("api.example.com", now, now)?.certificate(),
        leaf.certificate()
    );
    assert_ne!(
        first.leaf("git.example.com", now, now)?.certificate(),
        leaf.certificate()
    );
    // Near its end a leaf is issued anew.
    let later = now + time::Duration::hours(23) + time::Duration::minutes(30);
    assert_ne!(
        first.leaf("api.example.com", now, later)?.certificate(),
        leaf.certificate()
    );

    assert!(!leaf.key().is_empty());
    Ok(())
}

// TEST: GW-U-105
#[tokio::test]
async fn gw_u_105_a_rotation_keeps_the_previous_ca_for_one_sandbox_lifetime() -> TestResult {
    let temporary = tempfile::tempdir()?;
    // The CA makes its own directory, private (0700).
    let directory = Private(temporary.path().join("ca"));
    let now = OffsetDateTime::now_utc();
    let first = HostCa::load(directory.path(), LIFETIME, now).await?;
    let certificates = first.certificates();
    let [ca] = certificates.as_slice() else {
        return Err("one CA before a rotation".into());
    };
    // A rotation: the new CA issues for sandboxes made after it, the previous one for those made before, and the
    // bundle carries both.
    let rotated = now + time::Duration::hours(1);
    rotate(directory.path(), rotated).await?;
    let after = HostCa::load(directory.path(), LIFETIME, rotated).await?;
    let certificates = after.certificates();
    let [new, previous] = certificates.as_slice() else {
        return Err("two CAs after a rotation".into());
    };
    assert_eq!(previous, ca);
    assert_ne!(new, ca);
    let fresh = after.leaf("api.example.com", rotated, rotated)?;
    assert!(trusted(
        new,
        fresh.certificate(),
        "api.example.com",
        rotated
    )?);
    assert!(!trusted(
        ca,
        fresh.certificate(),
        "api.example.com",
        rotated
    )?);
    let old = after.leaf("api.example.com", now, rotated)?;
    assert!(trusted(ca, old.certificate(), "api.example.com", rotated)?);
    for pem in [&new, &previous] {
        assert!(after.bundle_pem().contains(&pem_of(pem)));
    }

    // One sandbox lifetime after the rotation the previous CA is gone, and every sandbox gets the new one's leaves.
    let gone = rotated + time::Duration::hours(8) + time::Duration::minutes(1);
    let last = HostCa::load(directory.path(), LIFETIME, gone).await?;
    assert_eq!(last.certificates().as_slice(), std::slice::from_ref(new));
    let leaf = last.leaf("api.example.com", now, gone)?;
    assert!(trusted(new, leaf.certificate(), "api.example.com", gone)?);
    assert!(!directory.path().join("host-ca-previous-key.pem").exists());
    Ok(())
}

fn pem_of(der: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(der)[..64].to_owned()
}

/// A path whose directory the CA makes itself (so it is private), with `path()` like a temporary directory.
struct Private(std::path::PathBuf);

impl Private {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

// TEST: GW-U-118
#[tokio::test]
async fn gw_u_118_an_existing_directory_others_can_read_is_refused()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt as _;
    let temporary = tempfile::tempdir()?;
    let open = temporary.path().join("open");
    std::fs::create_dir(&open)?;
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755))?;
    let refused = HostCa::load(&open, LIFETIME, OffsetDateTime::now_utc()).await;
    assert!(
        matches!(refused, Err(crate::GatewayError::Configuration(message)) if message.contains("must be private")),
        "a world-readable directory is refused"
    );
    assert!(
        !open.join("host-ca-key.pem").exists(),
        "and no key is written into it"
    );
    // One the CA makes is private, and so is reused.
    let made = temporary.path().join("made");
    HostCa::load(&made, LIFETIME, OffsetDateTime::now_utc()).await?;
    assert_eq!(
        std::fs::metadata(&made)?.permissions().mode() & 0o777,
        0o700
    );
    HostCa::load(&made, LIFETIME, OffsetDateTime::now_utc()).await?;
    Ok(())
}

// TEST: GW-U-105
/// The install CA is found in a bundle that holds it, alone or among others, so `egress.ca_file` can be refused when it
/// does: the install CA must never become an upstream root (the second review's L6).
#[tokio::test]
async fn gw_u_105_the_install_ca_is_found_in_a_bundle_that_holds_it() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let now = OffsetDateTime::now_utc();
    let install = HostCa::load(&temporary.path().join("install"), LIFETIME, now).await?;
    let other = HostCa::load(&temporary.path().join("other"), LIFETIME, now).await?;
    assert!(install.among(install.bundle_pem().as_bytes()));
    assert!(install.among(format!("{}{}", other.bundle_pem(), install.bundle_pem()).as_bytes()));
    assert!(!install.among(other.bundle_pem().as_bytes()));
    assert!(!install.among(b""));
    Ok(())
}
