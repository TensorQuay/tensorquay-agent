//! The kernel and rootfs artefacts named by their manifests (images/rootfs), verified by SHA-256 before any VM uses
//! them. Manifests are parsed strictly and bounded, every recorded digest must be well formed, and a mismatch refuses
//! the artefact with a static reason.

use std::{
    collections::HashMap,
    fmt::Write,
    fs::File,
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{ImageRef, SandboxError};

const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
/// Builds searched for a registry-digest pin: a template's versions over time, with a wide margin (32 templates).
pub(crate) const MAX_ROOTFS_BUILDS: usize = 256;
/// The slowest SHA-256 rate a rootfs check may run at: an unoptimised test build on the reference host. A release
/// gateway hashes several times faster; the bound only has to hold for the slowest build we run.
const ROOTFS_VERIFY_BYTES_PER_SECOND: u64 = 20 << 20;
const KERNEL_MALFORMED: &str = "the guest kernel manifest is missing or malformed";
const ROOTFS_MALFORMED: &str = "the rootfs manifest is malformed";
const ROOTFS_OUTDATED: &str =
    "the rootfs was built by an older builder; rebuild it (images/rootfs)";
/// Bounds on the recorded image policy, far above any real image.
const MAX_POLICY_ENTRIES: usize = 256;
const MAX_POLICY_ENTRY_BYTES: usize = 1024;

/// A verified guest kernel.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Kernel {
    /// The uncompressed kernel.
    pub(crate) path: PathBuf,
    /// Its kernel.org release.
    pub(crate) release: String,
}

/// A verified root filesystem for one template image.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Rootfs {
    /// The read-only ext4 image.
    pub(crate) path: PathBuf,
    /// The runtime protocol revision the image was labelled with.
    pub(crate) protocol_revision: u16,
    /// The image's environment key names and declared volumes, for the image policy (SBX-C-014).
    pub(crate) policy: ImagePolicy,
}

/// What the builder recorded of the image for Docker's image policy: key names only, never values.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ImagePolicy {
    env_keys: Vec<String>,
    volumes: Vec<String>,
}

impl ImagePolicy {
    fn valid(&self) -> bool {
        self.env_keys.len() <= MAX_POLICY_ENTRIES
            && self.volumes.len() <= MAX_POLICY_ENTRIES
            && self
                .env_keys
                .iter()
                .chain(&self.volumes)
                .all(|entry| !entry.is_empty() && entry.len() <= MAX_POLICY_ENTRY_BYTES)
    }

    /// Refuses what the Docker backend refuses at create (`docker/image_policy.rs`): an image-declared volume, or an
    /// image environment key the spec leaves unset, which would otherwise reach the guest with the image's value.
    ///
    /// # Errors
    /// `InvalidSpec` on `image`, naming the unset keys (never their values).
    pub(crate) fn check(&self, spec: &crate::SandboxSpec) -> Result<(), SandboxError> {
        if !self.volumes.is_empty() {
            return Err(SandboxError::invalid(
                "image",
                "image-declared volumes would add unapproved mounts",
            ));
        }
        let unset: Vec<Option<&str>> = self
            .env_keys
            .iter()
            .filter(|key| !spec.env.contains_key(*key))
            .map(|key| Some(key.as_str()))
            .collect();
        if unset.is_empty() {
            Ok(())
        } else {
            Err(SandboxError::InvalidSpec {
                field: "image",
                reason: crate::docker::image_policy::unset_environment(&unset).into(),
            })
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KernelManifest {
    schema: u32,
    release: String,
    licence: String,
    source_sha256: String,
    base_config: String,
    base_config_sha256: String,
    fragment_sha256: String,
    config_sha256: String,
    vmlinux_sha256: String,
}

impl KernelManifest {
    fn valid(&self) -> bool {
        self.schema == 1
            && self.licence == "GPL-2.0-only"
            && !self.release.is_empty()
            && !self.base_config.is_empty()
            && [
                &self.source_sha256,
                &self.base_config_sha256,
                &self.fragment_sha256,
                &self.config_sha256,
                &self.vmlinux_sha256,
            ]
            .iter()
            .all(|digest| hex64(digest))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RootfsManifest {
    schema: u32,
    source_image: SourceImage,
    rootfs: RootfsEntry,
    init: Binary,
    runtime: Binary,
    builder: Builder,
    /// Absent before schema 2.
    image: Option<ImagePolicy>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceImage {
    id: String,
    repo_digests: Vec<String>,
    protocol_revision: u16,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RootfsEntry {
    file: String,
    sha256: String,
    bytes: u64,
    tree_sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binary {
    path: String,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Builder {
    image_id: String,
    e2fsprogs: String,
}

impl RootfsManifest {
    fn valid(&self) -> bool {
        self.schema == 2
            && self.image.as_ref().is_some_and(ImagePolicy::valid)
            && self.rootfs.file == "rootfs.ext4"
            && self.source_image.repo_digests.len() <= 64
            && [
                &self.rootfs.sha256,
                &self.rootfs.tree_sha256,
                &self.init.sha256,
                &self.runtime.sha256,
            ]
            .iter()
            .all(|digest| hex64(digest))
            && self.init.path == "/usr/sbin/tqa-vm-init"
            && self.runtime.path.starts_with('/')
            && self
                .builder
                .image_id
                .strip_prefix("sha256:")
                .is_some_and(hex64)
            && !self.builder.e2fsprogs.is_empty()
    }
}

/// A file's identity: device, inode, size and modification time.
type FileKey = (u64, u64, u64, i64, i64);

/// Remembers verified files by identity, so a large rootfs is hashed once per change, not per sandbox.
#[derive(Debug, Default)]
pub(crate) struct Verified(Mutex<HashMap<FileKey, String>>);

impl Verified {
    /// Checks `path` against `expected` (64 lowercase hex digits).
    ///
    /// # Errors
    /// Returns `Preflight(reason)` for a missing, unreadable or different file.
    pub(crate) fn check(
        &self,
        path: &Path,
        expected: &str,
        reason: &'static str,
    ) -> Result<(), SandboxError> {
        let refuse = |_| SandboxError::Preflight(reason);
        let metadata = std::fs::metadata(path).map_err(refuse)?;
        let key = (
            metadata.dev(),
            metadata.ino(),
            metadata.size(),
            metadata.mtime(),
            metadata.mtime_nsec(),
        );
        let known = self
            .0
            .lock()
            .map_err(|_| SandboxError::Preflight(reason))?
            .get(&key)
            .cloned();
        let actual = if let Some(digest) = known {
            digest
        } else {
            let digest = sha256(path).map_err(refuse)?;
            self.0
                .lock()
                .map_err(|_| SandboxError::Preflight(reason))?
                .insert(key, digest.clone());
            digest
        };
        if actual == expected {
            Ok(())
        } else {
            Err(SandboxError::Preflight(reason))
        }
    }
}

/// Loads and verifies `directory/vmlinux` against `directory/manifest.json`.
///
/// # Errors
/// Returns `Preflight` for a missing or malformed manifest or a kernel that differs from it.
pub(crate) fn kernel(directory: &Path, verified: &Verified) -> Result<Kernel, SandboxError> {
    let manifest: KernelManifest = parse(&directory.join("manifest.json"), KERNEL_MALFORMED)?;
    if !manifest.valid() {
        return Err(SandboxError::Preflight(KERNEL_MALFORMED));
    }
    let path = directory.join("vmlinux");
    verified.check(
        &path,
        &manifest.vmlinux_sha256,
        "the guest kernel does not match its manifest",
    )?;
    Ok(Kernel {
        path,
        release: manifest.release,
    })
}

/// Loads and verifies the rootfs built for `image` under `directory/<image id hex>/`.
///
/// # Errors
/// Returns `Preflight` when no rootfs was built for the image, its manifest names another image or is malformed,
/// or the file differs from the manifest; `RuntimeTooOld` below the protocol floor.
/// How long checking the template's rootfs may take: `base` plus the time to hash the file at the slowest supported
/// rate, so a large template (the Workstation rootfs is over 2 GB) is not refused by a bound sized for small operations.
pub(crate) fn verify_bound(
    directory: &Path,
    image: &ImageRef,
    base: std::time::Duration,
) -> std::time::Duration {
    let bytes = image
        .digest()
        .strip_prefix("sha256:")
        .and_then(|hex| std::fs::metadata(directory.join(hex).join("rootfs.ext4")).ok())
        .map_or(0, |metadata| metadata.len());
    base.saturating_add(std::time::Duration::from_secs(
        bytes.div_ceil(ROOTFS_VERIFY_BYTES_PER_SECOND),
    ))
}

const ROOTFS_ABSENT: &str = "no rootfs was built for this image";
const ROOTFS_AMBIGUOUS: &str = "several rootfs builds claim this image";
const ROOTFS_TOO_MANY: &str = "too many rootfs builds to search for this image";
const ROOTFS_FOREIGN: &str = "the rootfs manifest names a different image";
/// The guest init comes from the builder, so a rootfs from another VM host image may silently lack its guarantees.
const ROOTFS_OTHER_BUILDER: &str =
    "the rootfs was built by another VM host image; rebuild it with images/rootfs/build_rootfs.py";

/// What a registry-digest search reads of a manifest; everything else is checked when the rootfs is verified.
#[derive(Deserialize)]
struct Listed {
    source_image: ListedSource,
}

#[derive(Deserialize)]
struct ListedSource {
    repo_digests: Vec<String>,
}

/// The local image ID whose build holds `image`'s rootfs. Builds are keyed by image ID. A registry-digest pin names a
/// registry manifest instead, so its build is the one whose manifest lists that digest; the digest identifies the
/// content, whatever repository name it was reached by. It reads the storage, so async callers run it off their
/// threads with a bound.
///
/// # Errors
/// `Preflight` when no build or more than one lists the digest, or when there are too many builds to search.
pub(crate) fn locate(directory: &Path, image: &ImageRef) -> Result<ImageRef, SandboxError> {
    if image.is_local() {
        return Ok(image.clone());
    }
    let entries =
        std::fs::read_dir(directory).map_err(|_| SandboxError::Preflight(ROOTFS_ABSENT))?;
    let (mut builds, mut found) = (0, None);
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(hex) = name.to_str().filter(|name| hex64(name)) else {
            continue;
        };
        builds += 1;
        if builds > MAX_ROOTFS_BUILDS {
            return Err(SandboxError::Preflight(ROOTFS_TOO_MANY));
        }
        let Ok(listed) = parse::<Listed>(&entry.path().join("manifest.json"), ROOTFS_ABSENT) else {
            continue;
        };
        let lists = listed.source_image.repo_digests.iter().any(|reference| {
            reference
                .rsplit_once('@')
                .is_some_and(|(_, digest)| digest == image.digest())
        });
        if lists && found.replace(format!("sha256:{hex}")).is_some() {
            return Err(SandboxError::Preflight(ROOTFS_AMBIGUOUS));
        }
    }
    ImageRef::local_id(found.ok_or(SandboxError::Preflight(ROOTFS_ABSENT))?)
}

pub(crate) fn rootfs(
    directory: &Path,
    image: &ImageRef,
    builder: Option<&str>,
    verified: &Verified,
) -> Result<Rootfs, SandboxError> {
    let hex = image
        .digest()
        .strip_prefix("sha256:")
        .ok_or(SandboxError::Preflight(
            "the image is not pinned by SHA-256",
        ))?;
    let base = directory.join(hex);
    let manifest: RootfsManifest = parse(&base.join("manifest.json"), ROOTFS_ABSENT)?;
    if manifest.schema == 1 {
        return Err(SandboxError::Preflight(ROOTFS_OUTDATED));
    }
    if !manifest.valid() {
        return Err(SandboxError::Preflight(ROOTFS_MALFORMED));
    }
    if manifest.source_image.id != image.digest() {
        return Err(SandboxError::Preflight(ROOTFS_FOREIGN));
    }
    let revision = manifest.source_image.protocol_revision;
    tqa_types::check_runtime_revision(revision).map_err(|too_old| SandboxError::RuntimeTooOld {
        needs: too_old.needs,
        has: revision,
    })?;
    if builder.is_some_and(|builder| builder != manifest.builder.image_id) {
        return Err(SandboxError::Preflight(ROOTFS_OTHER_BUILDER));
    }
    let path = base.join("rootfs.ext4");
    if std::fs::metadata(&path).map(|metadata| metadata.len()).ok() != Some(manifest.rootfs.bytes) {
        return Err(SandboxError::Preflight(
            "the rootfs does not match its manifest",
        ));
    }
    verified.check(
        &path,
        &manifest.rootfs.sha256,
        "the rootfs does not match its manifest",
    )?;
    Ok(Rootfs {
        path,
        protocol_revision: revision,
        policy: manifest.image.unwrap_or_default(),
    })
}

/// A template's rootfs as the admin API reports it (ADR-0032).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RootfsStatus {
    /// Verified and usable.
    Ready,
    /// No build exists for the image.
    Missing,
    /// Built by an older builder schema or another VM host image, or its runtime is below the protocol floor: rebuild it.
    Outdated,
    /// The file differs from its manifest, or the manifest breaks its rules.
    Mismatch,
    /// The manifest in the image's directory names another image.
    Foreign,
    /// Several builds claim the registry digest, or there are too many builds to search.
    Ambiguous,
}

/// The status of `image`'s rootfs: create's own checks, plus whether `builder` (the configured VM host image's ID,
/// when it is known) built it. It reads the storage and has no side effects; async callers run it off their threads.
pub(crate) fn status(
    directory: &Path,
    image: &ImageRef,
    builder: Option<&str>,
    verified: &Verified,
) -> RootfsStatus {
    let located = match locate(directory, image) {
        Ok(located) => located,
        Err(SandboxError::Preflight(ROOTFS_AMBIGUOUS | ROOTFS_TOO_MANY)) => {
            return RootfsStatus::Ambiguous;
        }
        Err(_) => return RootfsStatus::Missing,
    };
    // Create's own verification, so the status and create always agree.
    match rootfs(directory, &located, builder, verified) {
        Ok(_) => RootfsStatus::Ready,
        Err(SandboxError::Preflight(ROOTFS_ABSENT)) => RootfsStatus::Missing,
        Err(
            SandboxError::Preflight(ROOTFS_OUTDATED | ROOTFS_OTHER_BUILDER)
            | SandboxError::RuntimeTooOld { .. },
        ) => RootfsStatus::Outdated,
        Err(SandboxError::Preflight(ROOTFS_FOREIGN)) => RootfsStatus::Foreign,
        Err(_) => RootfsStatus::Mismatch,
    }
}

fn parse<T: serde::de::DeserializeOwned>(
    path: &Path,
    reason: &'static str,
) -> Result<T, SandboxError> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|_| SandboxError::Preflight(reason))?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(SandboxError::Preflight(reason));
    }
    serde_json::from_slice(&bytes).map_err(|_| SandboxError::Preflight(reason))
}

fn sha256(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 20];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

/// Lowercase hexadecimal, the form every manifest records.
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}

fn hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
#[path = "manifest_tests.rs"]
mod tests;
