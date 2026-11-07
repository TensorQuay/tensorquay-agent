//! Credential files in a template's own layers (ADR-0032, SBX-U-010). The layers beyond the base's are read in order,
//! uncompressed, as tar archives; a regular file is a hit when it has one of a fixed set of high-confidence credential
//! shapes. Only paths are reported, never content. The scan is bounded and cannot prove absence: a secret in any other
//! shape passes.
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    io::Read,
    path::{Component, Path},
};

#[cfg(test)]
mod export_tests;
#[cfg(test)]
mod tests;

/// The bytes of a regular file read for a private-key header.
pub const HEADER_BYTES: usize = 4096;
/// The bytes of a well-known credential file read for its rule.
pub const NAMED_FILE_BYTES: usize = 65_536;
/// The most paths a result lists; the total counts the rest.
pub const MAX_LISTED: usize = 16;
/// The longest listed path, in bytes; a longer one is cut at a character boundary and ends with `…`.
pub const MAX_LISTED_PATH_BYTES: usize = 512;
/// The default entry bound: 4.9 times the Workstation template's own layers (32,787 entries on 27 Sep 2026).
pub const DEFAULT_MAX_ENTRIES: u64 = 160_000;
/// The default content bound: 6 GiB, 4.6 times the Workstation template's own layers (1.40 GB).
pub const DEFAULT_MAX_BYTES: u64 = 6 << 30;

const WHITEOUT: &str = ".wh.";
const OPAQUE: &str = ".wh..wh..opq";
const KEY_FILES: [&str; 4] = ["id_rsa", "id_dsa", "id_ecdsa", "id_ed25519"];

/// How far a scan may read before it stops as [`ScanError::Limit`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Bounds {
    /// Tar entries across the scanned layers.
    pub max_entries: u64,
    /// Regular-file content across the scanned layers, counted by the size each entry declares.
    pub max_bytes: u64,
}

impl Default for Bounds {
    fn default() -> Self {
        Self {
            max_entries: DEFAULT_MAX_ENTRIES,
            max_bytes: DEFAULT_MAX_BYTES,
        }
    }
}

/// Which of the scan's bounds a template's layers exceed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Bound {
    /// [`Bounds::max_entries`].
    Entries,
    /// [`Bounds::max_bytes`].
    Bytes,
}

impl Bound {
    /// The stable name (`entries`, `bytes`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Entries => "entries",
            Self::Bytes => "bytes",
        }
    }

    /// This bound's value in `bounds`.
    #[must_use]
    pub const fn limit(self, bounds: Bounds) -> u64 {
        match self {
            Self::Entries => bounds.max_entries,
            Self::Bytes => bounds.max_bytes,
        }
    }
}

/// Which credential shape a hit has: a closed set, so a listing can say why in words (never content).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Rule {
    /// A PEM private-key header in the file's first bytes.
    PrivateKey,
    /// `.git-credentials` with content.
    GitCredentials,
    /// `.netrc` with a password.
    Netrc,
    /// `.aws/credentials` with a secret access key.
    AwsCredentials,
    /// `.docker/config.json` with a stored registry credential.
    DockerAuth,
    /// `.npmrc` with a token or password.
    NpmrcToken,
    /// `.pypirc` with a password.
    PypircPassword,
    /// An `id_*` SSH private key file.
    SshKey,
}

impl Rule {
    /// The stable name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PrivateKey => "private_key",
            Self::GitCredentials => "git_credentials",
            Self::Netrc => "netrc",
            Self::AwsCredentials => "aws_credentials",
            Self::DockerAuth => "docker_auth",
            Self::NpmrcToken => "npmrc_token",
            Self::PypircPassword => "pypirc_password",
            Self::SshKey => "ssh_key",
        }
    }
}

/// Why a scan has no result. Neither is ever a clean result.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ScanError {
    /// The layers exceed this bound (`image_scan_limit`).
    #[error("the template's layers exceed the credential-file scan's {} bound", .0.as_str())]
    Limit(Bound),
    /// A layer is not a readable tar archive, or names a path outside the root.
    #[error("a template layer could not be read")]
    Malformed,
}

/// One listed credential file: its path (sanitised), its rule, and whether the install file acknowledges it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecretFile {
    /// The absolute path inside the image, as a listing shows it.
    pub path: String,
    /// Its credential shape.
    pub rule: Rule,
    /// Whether the install file acknowledges the path.
    pub acknowledged: bool,
}

/// The credential files found: at most [`MAX_LISTED`] items, the unacknowledged first, each group by path; and counts.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SecretFiles {
    /// The listed files.
    pub items: Vec<SecretFile>,
    /// All hits, listed or not.
    pub total: usize,
    /// The hits the install file does not acknowledge, listed or not: more than none refuses the template.
    pub unacknowledged: usize,
}

/// One layer's effect on a scan: the paths it removes or replaces, and its credential files. It is read before it is
/// known to be one of the template's own layers (an engine export lists layers by content), then applied in order.
#[derive(Debug, Default)]
pub struct Layer {
    entries: u64,
    bytes: u64,
    removals: Vec<(String, bool)>,
    hits: Vec<(String, Rule)>,
}

impl Layer {
    /// Reads one uncompressed layer tar, bounded by `bounds` on its own. `reader` is borrowed, so a caller hashing the
    /// layer can read the rest of it afterwards.
    ///
    /// # Errors
    /// [`ScanError::Limit`] past a bound; [`ScanError::Malformed`] for an unreadable archive or an escaping path.
    pub fn read<R: Read>(reader: &mut R, bounds: Bounds) -> Result<Self, ScanError> {
        let mut layer = Self::default();
        let mut archive = tar::Archive::new(reader);
        for entry in archive.entries().map_err(|_| ScanError::Malformed)? {
            let mut entry = entry.map_err(|_| ScanError::Malformed)?;
            layer.entries += 1;
            if layer.entries > bounds.max_entries {
                return Err(ScanError::Limit(Bound::Entries));
            }
            let path = absolute(&entry.path().map_err(|_| ScanError::Malformed)?)?;
            let (directory, name) = split(&path);
            if name == OPAQUE {
                // The lower layers' content of this directory is gone; this layer's own stays.
                layer.removals.push((directory.to_owned(), false));
                continue;
            }
            if let Some(removed) = name.strip_prefix(WHITEOUT) {
                layer.removals.push((join(directory, removed), true));
                continue;
            }
            // Anything at this path replaces what a lower layer had there.
            layer.removals.push((path.clone(), true));
            if !entry.header().entry_type().is_file() {
                continue;
            }
            let size = entry.size();
            layer.bytes = layer.bytes.saturating_add(size);
            if layer.bytes > bounds.max_bytes {
                return Err(ScanError::Limit(Bound::Bytes));
            }
            let rule = named_rule(&path);
            let limit = if rule.is_some() {
                NAMED_FILE_BYTES
            } else {
                HEADER_BYTES
            };
            let mut head = Vec::with_capacity(limit.min(usize::try_from(size).unwrap_or(limit)));
            (&mut entry)
                .take(limit as u64)
                .read_to_end(&mut head)
                .map_err(|_| ScanError::Malformed)?;
            // A named file's own rule names the hit; otherwise a private-key header does.
            let hit = match rule {
                Some((named, rule)) if rule.matches(&head) => Some(named),
                _ => private_key_header(&head[..head.len().min(HEADER_BYTES)])
                    .then_some(Rule::PrivateKey),
            };
            if let Some(rule) = hit {
                layer.hits.push((path, rule));
            }
        }
        Ok(layer)
    }
}

impl SecretFiles {
    /// The listing of these hits, marked by the paths the install file acknowledges (compared before sanitising).
    #[must_use]
    pub fn listing(hits: &BTreeMap<String, Rule>, acknowledged: &BTreeSet<String>) -> Self {
        let mut ordered: Vec<(bool, &String, Rule)> = hits
            .iter()
            .map(|(path, rule)| (acknowledged.contains(path), path, *rule))
            .collect();
        // Unacknowledged (false) first, then acknowledged; each group by path.
        ordered.sort();
        Self {
            total: hits.len(),
            unacknowledged: ordered.iter().filter(|(known, ..)| !known).count(),
            items: ordered
                .into_iter()
                .take(MAX_LISTED)
                .map(|(acknowledged, path, rule)| SecretFile {
                    path: listed(path),
                    rule,
                    acknowledged,
                })
                .collect(),
        }
    }

    /// The listed paths, in order.
    #[must_use]
    pub fn paths(&self) -> Vec<&str> {
        self.items.iter().map(|item| item.path.as_str()).collect()
    }
}

/// A path as a listing shows it: control characters as U+FFFD, and at most [`MAX_LISTED_PATH_BYTES`].
fn listed(path: &str) -> String {
    let clean: String = path
        .chars()
        .map(|character| {
            if character.is_control() {
                '\u{fffd}'
            } else {
                character
            }
        })
        .collect();
    if clean.len() <= MAX_LISTED_PATH_BYTES {
        return clean;
    }
    let mut end = MAX_LISTED_PATH_BYTES - '…'.len_utf8();
    while !clean.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &clean[..end])
}

/// A scan over a template's own layers, applied one layer at a time in the image's order.
#[derive(Debug)]
pub struct LayerScan {
    bounds: Bounds,
    hits: BTreeMap<String, Rule>,
    entries: u64,
    bytes: u64,
}

impl LayerScan {
    /// An empty scan with these bounds.
    #[must_use]
    pub const fn new(bounds: Bounds) -> Self {
        Self {
            bounds,
            hits: BTreeMap::new(),
            entries: 0,
            bytes: 0,
        }
    }

    /// Reads and applies the next layer, an uncompressed tar archive.
    ///
    /// # Errors
    /// As [`Layer::read`], with the bounds this scan has left.
    pub fn layer(&mut self, mut reader: impl Read) -> Result<(), ScanError> {
        let left = Bounds {
            max_entries: self.bounds.max_entries.saturating_sub(self.entries),
            max_bytes: self.bounds.max_bytes.saturating_sub(self.bytes),
        };
        let layer = Layer::read(&mut reader, left)?;
        self.apply(layer)
    }

    /// Applies the next layer: its removals act on the earlier layers' hits, then its own hits are added.
    ///
    /// # Errors
    /// [`ScanError::Limit`] when the layers so far exceed a bound.
    pub fn apply(&mut self, layer: Layer) -> Result<(), ScanError> {
        self.entries = self.entries.saturating_add(layer.entries);
        self.bytes = self.bytes.saturating_add(layer.bytes);
        if self.entries > self.bounds.max_entries {
            return Err(ScanError::Limit(Bound::Entries));
        }
        if self.bytes > self.bounds.max_bytes {
            return Err(ScanError::Limit(Bound::Bytes));
        }
        for (path, inclusive) in &layer.removals {
            self.remove_under(path, *inclusive);
        }
        self.hits.extend(layer.hits);
        Ok(())
    }

    /// The hits, none acknowledged: at most [`MAX_LISTED`] items, and the counts.
    #[must_use]
    pub fn finish(self) -> SecretFiles {
        SecretFiles::listing(&self.hits, &BTreeSet::new())
    }

    /// Removes the earlier layers' hits under `path` (and `path` itself when `inclusive`).
    fn remove_under(&mut self, path: &str, inclusive: bool) {
        let prefix = if path == "/" {
            "/".to_owned()
        } else {
            format!("{path}/")
        };
        self.hits
            .retain(|hit, _| !(hit.starts_with(&prefix) || (inclusive && hit == path)));
    }
}

/// The entry's path as an absolute, normalised image path; a `..` or an absolute component is malformed.
fn absolute(path: &Path) -> Result<String, ScanError> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) | Component::ParentDir => {
                return Err(ScanError::Malformed);
            }
        }
    }
    Ok(format!("/{}", parts.join("/")))
}

fn split(path: &str) -> (&str, &str) {
    match path.rsplit_once('/') {
        Some(("", name)) => ("/", name),
        Some((directory, name)) => (directory, name),
        None => ("/", path),
    }
}

fn join(directory: &str, name: &str) -> String {
    if directory == "/" {
        format!("/{name}")
    } else {
        format!("{directory}/{name}")
    }
}

/// A well-known credential file and what its content must hold to be one.
#[derive(Clone, Copy, Debug)]
enum NamedRule {
    /// Any content at all.
    NonEmpty,
    /// Any content: the name alone is the private key's.
    Always,
    /// One of these substrings.
    Contains(&'static [&'static str]),
    /// A Docker client configuration with a stored registry credential.
    DockerAuths,
}

impl NamedRule {
    fn matches(self, head: &[u8]) -> bool {
        match self {
            Self::Always => true,
            Self::NonEmpty => head.iter().any(|byte| !byte.is_ascii_whitespace()),
            Self::Contains(needles) => {
                let text = String::from_utf8_lossy(head);
                needles.iter().any(|needle| text.contains(needle))
            }
            Self::DockerAuths => serde_json::from_slice::<serde_json::Value>(head)
                .ok()
                .and_then(|config| config.get("auths").cloned())
                .and_then(|auths| auths.as_object().cloned())
                .is_some_and(|auths| {
                    auths.values().any(|entry| {
                        entry
                            .get("auth")
                            .and_then(serde_json::Value::as_str)
                            .is_some_and(|auth| !auth.is_empty())
                    })
                }),
        }
    }
}

/// The rule for a well-known credential file's path, if it is one: the hit's name and what its content must hold.
fn named_rule(path: &str) -> Option<(Rule, NamedRule)> {
    let (_, name) = split(path);
    if KEY_FILES.contains(&name) {
        return Some((Rule::SshKey, NamedRule::Always));
    }
    if path.ends_with("/.aws/credentials") {
        return Some((
            Rule::AwsCredentials,
            NamedRule::Contains(&["aws_secret_access_key"]),
        ));
    }
    if path.ends_with("/.docker/config.json") {
        return Some((Rule::DockerAuth, NamedRule::DockerAuths));
    }
    match name {
        ".git-credentials" => Some((Rule::GitCredentials, NamedRule::NonEmpty)),
        ".netrc" => Some((Rule::Netrc, NamedRule::Contains(&["password"]))),
        ".pypirc" => Some((Rule::PypircPassword, NamedRule::Contains(&["password"]))),
        ".npmrc" => Some((
            Rule::NpmrcToken,
            NamedRule::Contains(&["_authToken=", "_auth=", "_password="]),
        )),
        _ => None,
    }
}

/// Whether `head` holds a PEM private-key header: `-----BEGIN <label>PRIVATE KEY-----` (or `PRIVATE KEY BLOCK`),
/// the label being upper-case letters, digits and spaces.
fn private_key_header(head: &[u8]) -> bool {
    const BEGIN: &[u8] = b"-----BEGIN ";
    let mut rest = head;
    while let Some(start) = find(rest, BEGIN) {
        let after = &rest[start + BEGIN.len()..];
        let label_end = after
            .iter()
            .position(|byte| !(byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b' '))
            .unwrap_or(after.len());
        let label = &after[..label_end];
        let closed = after[label_end..].starts_with(b"-----");
        if closed && (label.ends_with(b"PRIVATE KEY") || label.ends_with(b"PRIVATE KEY BLOCK")) {
            return true;
        }
        rest = after;
    }
    false
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Why an engine export gave no result. Neither is ever a clean result.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ExportError {
    /// One of the template's own layers is past a bound or unreadable.
    #[error(transparent)]
    Scan(ScanError),
    /// The export could not be read, or did not hold every one of the template's layers in a form this scan reads
    /// (a zstd layer, for one): nothing is known.
    #[error("the image export could not be scanned")]
    Unavailable,
}

const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];
const ZSTD_MAGIC: [u8; 4] = [0x28, 0xb5, 0x2f, 0xfd];
const TAR_MAGIC_OFFSET: usize = 257;

/// Scans an engine export (`docker save`, in the OCI or the classic layout) for credential files in the template's
/// own layers. `own_layers` are the layer IDs (`sha256:<hex>` of each uncompressed layer tar) beyond the base's, in
/// the image's order. Every blob is read once: each gzip or tar blob is scanned and hashed, and only those whose hash
/// is one of `own_layers` count, applied in the image's order whatever the export's order.
///
/// # Errors
/// [`ExportError::Scan`] when an own layer is past a bound or malformed; [`ExportError::Unavailable`] when the export
/// is unreadable or an own layer was not found.
pub fn scan_export(
    export: impl Read,
    own_layers: &[String],
    bounds: Bounds,
) -> Result<SecretFiles, ExportError> {
    export_hits(export, own_layers, bounds)
        .map(|hits| SecretFiles::listing(&hits, &BTreeSet::new()))
}

/// As [`scan_export`], with every hit and its rule rather than the listing.
///
/// # Errors
/// As [`scan_export`].
pub fn export_hits(
    export: impl Read,
    own_layers: &[String],
    bounds: Bounds,
) -> Result<BTreeMap<String, Rule>, ExportError> {
    if own_layers.is_empty() {
        return Ok(BTreeMap::new());
    }
    let mut found: Vec<Option<Result<Layer, ScanError>>> =
        own_layers.iter().map(|_| None).collect();
    let mut archive = tar::Archive::new(export);
    for entry in archive.entries().map_err(|_| ExportError::Unavailable)? {
        let entry = entry.map_err(|_| ExportError::Unavailable)?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let mut buffered = std::io::BufReader::with_capacity(1 << 16, entry);
        let head =
            std::io::BufRead::fill_buf(&mut buffered).map_err(|_| ExportError::Unavailable)?;
        let (layer, digest) = if head.starts_with(&GZIP_MAGIC) {
            read_blob(flate2::read::GzDecoder::new(buffered), bounds)
        } else if head.starts_with(&ZSTD_MAGIC) {
            // Not readable here: if it is one of the template's layers, that layer stays missing.
            continue;
        } else if head.get(TAR_MAGIC_OFFSET..TAR_MAGIC_OFFSET + 5) == Some(b"ustar".as_slice()) {
            read_blob(buffered, bounds)
        } else {
            continue;
        };
        let Some(digest) = digest else { continue };
        if let Some(index) = own_layers.iter().position(|id| *id == digest) {
            found[index] = Some(layer);
        }
    }
    let mut scan = LayerScan::new(bounds);
    for layer in found {
        let layer = layer
            .ok_or(ExportError::Unavailable)?
            .map_err(ExportError::Scan)?;
        scan.apply(layer).map_err(ExportError::Scan)?;
    }
    Ok(scan.hits)
}

/// Scans one blob as a layer tar, then reads it to the end for its layer ID; `None` when it could not be read whole.
fn read_blob(reader: impl Read, bounds: Bounds) -> (Result<Layer, ScanError>, Option<String>) {
    let mut hashing = Hashing {
        inner: reader,
        hasher: sha2::Sha256::default(),
    };
    let layer = Layer::read(&mut hashing, bounds);
    let digest = std::io::copy(&mut hashing, &mut std::io::sink())
        .ok()
        .map(|_| {
            let digest = sha2::Digest::finalize(hashing.hasher);
            digest
                .iter()
                .fold(String::from("sha256:"), |mut text, byte| {
                    let _ = write!(text, "{byte:02x}");
                    text
                })
        });
    (layer, digest)
}

/// A reader that hashes what passes through it.
struct Hashing<R> {
    inner: R,
    hasher: sha2::Sha256,
}

impl<R: Read> Read for Hashing<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        sha2::Digest::update(&mut self.hasher, &buffer[..read]);
        Ok(read)
    }
}
