//! The two inputs of the guest init: the host's per-sandbox configuration device and the image configuration the
//! rootfs pipeline recorded. Both are parsed once here into validated types (parse, don't validate).

use std::{collections::BTreeSet, net::IpAddr, path::PathBuf};

use secrecy::SecretString;
use serde::Deserialize;

use crate::error::InitError;

/// Upper bound for either document; the configuration device is padded with zero bytes after the document.
pub const MAX_DOCUMENT_BYTES: usize = 64 * 1024;
/// Variables whose values the init owns because it owns the paths they name.
pub const INIT_OWNED: [&str; 2] = ["TQA_RUNTIME_TOKEN_FILE", "TQA_STATE_DIR"];
const MAX_ENV: usize = 96;
const MAX_HOSTS: usize = 4;

/// One static name-to-address entry; the guest has no resolver.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostEntry {
    /// A validated host name.
    pub name: String,
    /// Its address on the sandbox network.
    pub address: IpAddr,
}

/// The host's per-sandbox configuration; `Debug` redacts the token.
#[derive(Debug)]
pub struct GuestConfig {
    /// The per-run token, written only to the token file.
    pub token: SecretString,
    /// The runtime environment built by the host backend, without init-owned names.
    pub env: Vec<(String, String)>,
    /// Static host entries, at least the gateway.
    pub hosts: Vec<HostEntry>,
    /// Process limit for the sandbox cgroup.
    pub pids: u32,
}

/// The image's process configuration, recorded at rootfs build time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageConfig {
    /// Non-root numeric user.
    pub uid: u32,
    /// Non-root numeric group.
    pub gid: u32,
    /// The image's environment.
    pub env: Vec<(String, String)>,
    /// Absolute working directory.
    pub working_dir: PathBuf,
    /// Entrypoint followed by command; the first element is an absolute path.
    pub argv: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGuest {
    version: u32,
    token: String,
    env: Vec<String>,
    hosts: Vec<RawHost>,
    pids: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawHost {
    name: String,
    address: IpAddr,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawImage {
    user: String,
    env: Vec<String>,
    working_dir: String,
    entrypoint: Vec<String>,
    cmd: Vec<String>,
}

/// Parses the configuration device's contents.
///
/// # Errors
/// Returns `Config` for oversized, malformed, unknown-field or out-of-range input, without echoing it.
pub fn parse_guest(device: &[u8]) -> Result<GuestConfig, InitError> {
    let bad = InitError::Config;
    let raw: RawGuest = serde_json::from_slice(document(device).ok_or(bad("size"))?)
        .map_err(|_| bad("document"))?;
    if raw.version != 1 {
        return Err(bad("unsupported version"));
    }
    if raw.token.is_empty() || raw.token.len() > 8192 || raw.token.contains('\0') {
        return Err(bad("token"));
    }
    let env = environment(&raw.env).ok_or(bad("env"))?;
    if env
        .iter()
        .any(|(key, _)| INIT_OWNED.contains(&key.as_str()))
    {
        return Err(bad("env names a variable the init owns"));
    }
    if raw.hosts.is_empty()
        || raw.hosts.len() > MAX_HOSTS
        || !raw.hosts.iter().all(|host| host_name(&host.name))
    {
        return Err(bad("hosts"));
    }
    if !(1..=65_536).contains(&raw.pids) {
        return Err(bad("pids"));
    }
    let hosts = raw
        .hosts
        .into_iter()
        .map(|host| HostEntry {
            name: host.name,
            address: host.address,
        })
        .collect();
    Ok(GuestConfig {
        token: SecretString::from(raw.token),
        env,
        hosts,
        pids: raw.pids,
    })
}

/// Parses the image configuration file.
///
/// # Errors
/// Returns `Image` for malformed input, a root or non-numeric user, a relative directory or program, or an empty
/// command.
pub fn parse_image(bytes: &[u8]) -> Result<ImageConfig, InitError> {
    let bad = InitError::Image;
    let raw: RawImage =
        serde_json::from_slice(document(bytes).ok_or(bad("size"))?).map_err(|_| bad("document"))?;
    let (uid, gid) = raw
        .user
        .split_once(':')
        .ok_or(bad("user must be uid:gid"))?;
    let (uid, gid) = (
        uid.parse::<u32>().map_err(|_| bad("user"))?,
        gid.parse::<u32>().map_err(|_| bad("user"))?,
    );
    if uid == 0 || gid == 0 {
        return Err(bad("the sandbox user must not be root"));
    }
    let working_dir = PathBuf::from(raw.working_dir);
    if !working_dir.is_absolute() {
        return Err(bad("working directory"));
    }
    let argv: Vec<String> = raw.entrypoint.into_iter().chain(raw.cmd).collect();
    if !argv.first().is_some_and(|program| program.starts_with('/'))
        || argv.iter().any(|arg| arg.contains('\0'))
    {
        return Err(bad("command"));
    }
    let env = environment(&raw.env).ok_or(bad("env"))?;
    Ok(ImageConfig {
        uid,
        gid,
        env,
        working_dir,
        argv,
    })
}

fn document(bytes: &[u8]) -> Option<&[u8]> {
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    (end > 0 && end <= MAX_DOCUMENT_BYTES).then(|| &bytes[..end])
}

fn environment(entries: &[String]) -> Option<Vec<(String, String)>> {
    let mut seen = BTreeSet::new();
    if entries.len() > MAX_ENV {
        return None;
    }
    entries
        .iter()
        .map(|entry| {
            let (key, value) = entry.split_once('=')?;
            let valid = !key.is_empty()
                && key.len() <= 128
                && key.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
                && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                && value.len() <= 8192
                && !value.contains('\0')
                && seen.insert(key.to_owned());
            valid.then(|| (key.to_owned(), value.to_owned()))
        })
        .collect()
}

fn host_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 253
        && name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
