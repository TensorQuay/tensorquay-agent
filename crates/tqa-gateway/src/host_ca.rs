//! The install CA for host keys (ADR-0037 §6): an ECDSA P-256 CA whose private key is a 0600 file beside the install
//! key, made at the first start. Only sandboxes trust it. The proxy completes TLS with a sandbox using a leaf for exactly
//! the keyed host, issued on demand, valid for 24 hours and cached. Rotation is on demand only ([`rotate`], taking
//! effect at the next start): the new CA issues from then on, and the previous one keeps issuing to sandboxes made before
//! the rotation, and stays in their bundle, for one sandbox lifetime.
use crate::GatewayError;
use base64::Engine as _;
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    Issuer, KeyPair, KeyUsagePurpose, PKCS_ECDSA_P256_SHA256,
};
use secrecy::{ExposeSecret as _, SecretSlice};
use sha2::Digest as _;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
};
use time::OffsetDateTime;
use tokio::io::AsyncWriteExt as _;

const KEY: &str = "host-ca-key.pem";
const CERTIFICATE: &str = "host-ca.pem";
const PREVIOUS_KEY: &str = "host-ca-previous-key.pem";
const PREVIOUS_CERTIFICATE: &str = "host-ca-previous.pem";
const ROTATED: &str = "host-ca-rotated";
/// The longest file the CA reads: a PEM key or certificate is far shorter.
const MAX_FILE_BYTES: usize = 16 * 1024;
/// A leaf lasts a day, and is issued anew in its last hour.
const LEAF_LIFETIME: time::Duration = time::Duration::hours(24);
const RENEW_BEFORE: time::Duration = time::Duration::hours(1);
/// The CA lasts until an administrator rotates it; ten years bounds a forgotten install.
const CA_LIFETIME: time::Duration = time::Duration::days(3650);

/// A leaf for one host: its certificate and private key, both DER, for the proxy's TLS with a sandbox.
#[derive(Clone)]
pub(crate) struct Leaf {
    certificate: Vec<u8>,
    key: SecretSlice<u8>,
    not_after: OffsetDateTime,
}

impl Leaf {
    /// The certificate, DER.
    pub(crate) fn certificate(&self) -> &[u8] {
        &self.certificate
    }

    /// The private key, PKCS #8 DER.
    pub(crate) fn key(&self) -> &[u8] {
        self.key.expose_secret()
    }
}

struct Authority {
    key: KeyPair,
    params: CertificateParams,
    certificate: Vec<u8>,
}

impl Authority {
    /// The CA's own parameters, rebuilt from its key alone: its name carries a digest of the public key, so two CAs
    /// never share a name and the stored certificate needs no parsing.
    fn params(key: &KeyPair) -> CertificateParams {
        let digest = sha2::Sha256::digest(key.public_key_raw());
        let mut params = CertificateParams::default();
        let mut name = DistinguishedName::new();
        name.push(DnType::OrganizationName, "TensorQuay");
        name.push(
            DnType::CommonName,
            format!(
                "TensorQuay host keys CA {}",
                &crate::hex::hex(&digest)[..16]
            ),
        );
        params.distinguished_name = name;
        params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        params
    }

    fn create(now: OffsetDateTime) -> Result<(Self, String), GatewayError> {
        let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).map_err(unavailable)?;
        let mut params = Self::params(&key);
        params.not_before = now - time::Duration::hours(1);
        params.not_after = now + CA_LIFETIME;
        let certificate = params.self_signed(&key).map_err(unavailable)?;
        let pem = certificate.pem();
        Ok((
            Self {
                params: Self::params(&key),
                certificate: certificate.der().to_vec(),
                key,
            },
            pem,
        ))
    }

    async fn read(key: &Path, certificate: &Path) -> Result<Self, GatewayError> {
        let key = crate::secrets::read_private(key, MAX_FILE_BYTES).await?;
        let key = std::str::from_utf8(key.expose_secret())
            .ok()
            .and_then(|pem| KeyPair::from_pem(pem).ok())
            .filter(|key| key.algorithm() == &PKCS_ECDSA_P256_SHA256)
            .ok_or(GatewayError::Configuration(
                "the host keys CA's key is not a P-256 key",
            ))?;
        let certificate = crate::secrets::read_private(certificate, MAX_FILE_BYTES).await?;
        let certificate = std::str::from_utf8(certificate.expose_secret())
            .ok()
            .and_then(der)
            .ok_or(GatewayError::Configuration(
                "the host keys CA's certificate is unreadable",
            ))?;
        Ok(Self {
            params: Self::params(&key),
            key,
            certificate,
        })
    }

    fn issue(&self, host: &str, now: OffsetDateTime) -> Result<Leaf, GatewayError> {
        let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).map_err(unavailable)?;
        let mut params = CertificateParams::new(vec![host.to_owned()]).map_err(unavailable)?;
        let mut name = DistinguishedName::new();
        name.push(DnType::CommonName, host);
        params.distinguished_name = name;
        params.is_ca = IsCa::ExplicitNoCa;
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.use_authority_key_identifier_extension = true;
        params.not_before = now - time::Duration::minutes(5);
        params.not_after = now + LEAF_LIFETIME;
        let issuer = Issuer::from_params(&self.params, &self.key);
        let certificate = params.signed_by(&key, &issuer).map_err(unavailable)?;
        Ok(Leaf {
            certificate: certificate.der().to_vec(),
            key: key.serialize_der().into(),
            not_after: params.not_after,
        })
    }
}

/// The install CA and, for one sandbox lifetime after a rotation, the previous one.
pub(crate) struct HostCa {
    current: Authority,
    /// The previous CA and when it was rotated out.
    previous: Option<(Authority, OffsetDateTime)>,
    leaves: Mutex<HashMap<(bool, String), Leaf>>,
}

impl HostCa {
    /// Reads the CA from `directory` (the install key's), making it at the first start; drops a previous CA whose
    /// sandbox `lifetime` since the rotation has passed.
    ///
    /// # Errors
    /// Unreadable or unsafe files, or a key that isn't P-256.
    pub(crate) async fn load(
        directory: &Path,
        lifetime: std::time::Duration,
        now: OffsetDateTime,
    ) -> Result<Self, GatewayError> {
        private_directory(directory).await?;
        let path = |name: &str| directory.join(name);
        if !exists(&path(KEY)).await? {
            create(directory, now).await?;
        }
        let current = Authority::read(&path(KEY), &path(CERTIFICATE)).await?;
        let previous = if exists(&path(PREVIOUS_KEY)).await? {
            let rotated = tokio::fs::read_to_string(path(ROTATED))
                .await
                .ok()
                .and_then(|text| {
                    OffsetDateTime::parse(
                        text.trim(),
                        &time::format_description::well_known::Rfc3339,
                    )
                    .ok()
                })
                .ok_or(GatewayError::Configuration(
                    "the host keys CA's rotation time is unreadable",
                ))?;
            let lifetime = time::Duration::try_from(lifetime)
                .map_err(|_| GatewayError::Configuration("the sandbox lifetime is too long"))?;
            if now >= rotated + lifetime {
                forget_previous(directory).await?;
                None
            } else {
                Some((
                    Authority::read(&path(PREVIOUS_KEY), &path(PREVIOUS_CERTIFICATE)).await?,
                    rotated,
                ))
            }
        } else {
            None
        };
        Ok(Self {
            current,
            previous,
            leaves: Mutex::new(HashMap::new()),
        })
    }

    /// Whether the CA's key is P-256.
    #[cfg(test)]
    pub(crate) fn is_p256(&self) -> bool {
        self.current.key.algorithm() == &PKCS_ECDSA_P256_SHA256
    }

    /// The CA certificates a sandbox trusts, DER, the current one first.
    pub(crate) fn certificates(&self) -> Vec<Vec<u8>> {
        std::iter::once(&self.current)
            .chain(self.previous.as_ref().map(|(authority, _)| authority))
            .map(|authority| authority.certificate.clone())
            .collect()
    }

    /// Whether the PEM `bundle` holds one of this install's CA certificates.
    pub(crate) fn among(&self, bundle: &[u8]) -> bool {
        use rustls::pki_types::{CertificateDer, pem::PemObject};
        let mine = self.certificates();
        CertificateDer::pem_slice_iter(bundle)
            .filter_map(Result::ok)
            .any(|certificate| {
                mine.iter()
                    .any(|own| own.as_slice() == certificate.as_ref())
            })
    }

    /// The same, as one PEM bundle for `session.config`.
    pub(crate) fn bundle_pem(&self) -> String {
        self.certificates()
            .iter()
            .map(|certificate| pem(certificate))
            .collect()
    }

    /// A leaf for `host` for a sandbox made at `sandbox_made`: from the previous CA when the sandbox predates the
    /// rotation, otherwise from the current one; cached until its last hour.
    ///
    /// # Errors
    /// The certificate can't be made.
    pub(crate) fn leaf(
        &self,
        host: &str,
        sandbox_made: OffsetDateTime,
        now: OffsetDateTime,
    ) -> Result<Leaf, GatewayError> {
        let (older, authority) = match &self.previous {
            Some((previous, rotated)) if sandbox_made < *rotated => (true, previous),
            _ => (false, &self.current),
        };
        let mut leaves = self
            .leaves
            .lock()
            .map_err(|_| GatewayError::StoreUnavailable)?;
        let slot = (older, host.to_owned());
        if let Some(leaf) = leaves.get(&slot)
            && leaf.not_after - now > RENEW_BEFORE
        {
            return Ok(leaf.clone());
        }
        let leaf = authority.issue(host, now)?;
        leaves.insert(slot, leaf.clone());
        Ok(leaf)
    }
}

/// Rotates the CA in `directory`: the current one becomes the previous (replacing an earlier previous) and a new one
/// is made. The running gateway reads it at its next start.
///
/// # Errors
/// No CA to rotate, or the files can't be written.
pub async fn rotate(directory: &Path, now: OffsetDateTime) -> Result<(), GatewayError> {
    private_directory(directory).await?;
    let path = |name: &str| directory.join(name);
    if !exists(&path(KEY)).await? {
        return Err(GatewayError::Configuration(
            "there is no host keys CA to rotate yet",
        ));
    }
    forget_previous(directory).await?;
    tokio::fs::rename(path(CERTIFICATE), path(PREVIOUS_CERTIFICATE))
        .await
        .map_err(|_| GatewayError::SecretUnavailable)?;
    tokio::fs::rename(path(KEY), path(PREVIOUS_KEY))
        .await
        .map_err(|_| GatewayError::SecretUnavailable)?;
    let rotated = now
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|_| GatewayError::SecretUnavailable)?;
    write_private(&path(ROTATED), rotated.as_bytes()).await?;
    create(directory, now).await
}

async fn create(directory: &Path, now: OffsetDateTime) -> Result<(), GatewayError> {
    let (authority, certificate) = Authority::create(now)?;
    // The certificate first: a key without its certificate would stop the next start.
    write_private(&directory.join(CERTIFICATE), certificate.as_bytes()).await?;
    let key: SecretSlice<u8> = authority.key.serialize_pem().into_bytes().into();
    write_private(&directory.join(KEY), key.expose_secret()).await?;
    sync_directory(directory).await
}

async fn forget_previous(directory: &Path) -> Result<(), GatewayError> {
    for name in [PREVIOUS_KEY, PREVIOUS_CERTIFICATE, ROTATED] {
        match tokio::fs::remove_file(directory.join(name)).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(GatewayError::SecretUnavailable),
        }
    }
    sync_directory(directory).await
}

/// The CA's directory, made 0700 when missing; an existing one others can read is refused (it holds a private key).
///
/// # Errors
/// A directory that can't be made, or one with group or other permissions.
pub(crate) async fn private_directory(directory: &Path) -> Result<(), GatewayError> {
    use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true).mode(0o700);
    let path = directory.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let fresh = !path.exists();
        builder.create(&path).map_err(|_| GatewayError::SecretUnavailable)?;
        let mode = std::fs::metadata(&path)
            .map_err(|_| GatewayError::SecretUnavailable)?
            .permissions()
            .mode();
        if !fresh && mode & 0o077 != 0 {
            return Err(GatewayError::Configuration(
                "the host keys CA directory must be private (mode 0700): it holds the CA's private key",
            ));
        }
        Ok(())
    })
    .await
    .map_err(|_| GatewayError::SecretUnavailable)?
}

async fn exists(path: &PathBuf) -> Result<bool, GatewayError> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(GatewayError::SecretUnavailable),
    }
}

/// Writes a new 0600 file, replacing an older one of the same name.
async fn write_private(path: &Path, bytes: &[u8]) -> Result<(), GatewayError> {
    let temporary = path.with_extension("new");
    let _ = tokio::fs::remove_file(&temporary).await;
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true).mode(0o600);
    let mut file = options
        .open(&temporary)
        .await
        .map_err(|_| GatewayError::SecretUnavailable)?;
    file.write_all(bytes)
        .await
        .map_err(|_| GatewayError::SecretUnavailable)?;
    file.sync_all()
        .await
        .map_err(|_| GatewayError::SecretUnavailable)?;
    tokio::fs::rename(&temporary, path)
        .await
        .map_err(|_| GatewayError::SecretUnavailable)
}

async fn sync_directory(directory: &Path) -> Result<(), GatewayError> {
    tokio::fs::File::open(directory)
        .await
        .map_err(|_| GatewayError::SecretUnavailable)?
        .sync_all()
        .await
        .map_err(|_| GatewayError::SecretUnavailable)
}

fn pem(der: &[u8]) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(der);
    let mut pem = String::from("-----BEGIN CERTIFICATE-----\n");
    for line in encoded.as_bytes().chunks(64) {
        pem.push_str(std::str::from_utf8(line).unwrap_or_default());
        pem.push('\n');
    }
    pem.push_str("-----END CERTIFICATE-----\n");
    pem
}

fn der(pem: &str) -> Option<Vec<u8>> {
    let body = pem
        .trim()
        .strip_prefix("-----BEGIN CERTIFICATE-----")?
        .strip_suffix("-----END CERTIFICATE-----")?;
    let text: String = body.split_whitespace().collect();
    base64::engine::general_purpose::STANDARD.decode(text).ok()
}

fn unavailable(_: rcgen::Error) -> GatewayError {
    GatewayError::SecretUnavailable
}

#[cfg(test)]
#[path = "host_ca_tests.rs"]
mod tests;
