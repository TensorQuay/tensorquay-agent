//! The configuration device (SBX-F-011): one JSON document for the guest init (`tqa-vm-init`, schema version 1),
//! zero-padded to whole 4 KiB blocks and attached read-only. It is the only place the per-run token exists outside
//! the gateway: never the kernel command line, the VMM configuration, the container environment or labels (ADR-0018).
//! On the host it is a 0600 file in a 0700 per-sandbox directory with one ACL entry for the VMM uid.

use std::{
    fs::{DirBuilder, OpenOptions},
    io::{self, Write},
    net::IpAddr,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

use secrecy::{ExposeSecret, zeroize::Zeroizing};
use serde::Serialize;

use super::acl::{self, Access};
use crate::{BackendKind, EgressMode, SandboxError, SandboxSpec};

/// The guest init's bound on the document.
const MAX_BYTES: usize = 64 * 1024;
const BLOCK: usize = 4096;
/// The device's file name in the per-sandbox directory.
pub(crate) const FILE: &str = "config.img";

#[derive(Serialize)]
struct Document<'a> {
    version: u32,
    token: &'a str,
    env: Vec<String>,
    hosts: Vec<Host>,
    pids: u32,
}

#[derive(Serialize)]
struct Host {
    name: String,
    address: IpAddr,
}

/// The backend property `sync_before_result` (ARCHITECTURE §4.5): a VM crash loses the guest's page cache, so the
/// runtime flushes the workspace before it journals each tool result. Docker's host page cache survives a container
/// kill, so it never sets this.
const SYNC_BEFORE_RESULT: &str = "TQA_SYNC_BEFORE_RESULT=1";

/// The runtime environment as in Docker mode, without the variables the guest init owns (it sets the same token file
/// and state directory paths itself), plus this backend's properties.
fn environment(spec: &SandboxSpec) -> Vec<String> {
    let owned = ["TQA_RUNTIME_TOKEN_FILE=", "TQA_STATE_DIR="];
    let mut env: Vec<String> = crate::docker::environment(spec)
        .into_iter()
        .filter(|entry| !owned.iter().any(|name| entry.starts_with(name)))
        .collect();
    env.push(SYNC_BEFORE_RESULT.to_owned());
    env
}

/// Static host entries: the gateway's names (runtime link and policy proxy) at its address; the guest has no
/// resolver (ADR-0021).
fn hosts(spec: &SandboxSpec, gateway: IpAddr) -> Result<Vec<Host>, SandboxError> {
    if spec.gateway_url.domain().is_none() {
        return Err(SandboxError::invalid(
            "gateway_url",
            "the Firecracker backend needs a gateway host name",
        ));
    }
    let mut names = vec![spec.gateway_url.domain()];
    if spec.egress.mode == EgressMode::Proxy {
        names.push(spec.egress.proxy_url.as_ref().and_then(url::Url::domain));
    }
    let mut hosts: Vec<Host> = Vec::new();
    for name in names.into_iter().flatten() {
        if !hosts.iter().any(|host| host.name == name) {
            hosts.push(Host {
                name: name.to_owned(),
                address: gateway,
            });
        }
    }
    Ok(hosts)
}

/// Encodes the device contents. The buffer is sized once, so the token is never left behind by a reallocation, and
/// it is zeroed when dropped.
///
/// # Errors
/// Returns `InvalidSpec` for an invalid spec, a gateway URL without a host name, or a document above 64 KiB.
pub(crate) fn document(
    spec: &SandboxSpec,
    gateway: IpAddr,
) -> Result<Zeroizing<Vec<u8>>, SandboxError> {
    spec.validate(BackendKind::Firecracker)?;
    let document = Document {
        version: 1,
        token: spec.runtime_token.expose_secret(),
        env: environment(spec),
        hosts: hosts(spec, gateway)?,
        pids: spec.pids,
    };
    let mut bytes = Zeroizing::new(Vec::with_capacity(MAX_BYTES + 1));
    serde_json::to_writer(LimitedWriter(&mut bytes), &document)
        .map_err(|_| SandboxError::invalid("env", "the configuration device exceeds 64 KiB"))?;
    let padded = bytes.len().div_ceil(BLOCK) * BLOCK;
    bytes.resize(padded, 0);
    Ok(bytes)
}

/// Refuses to grow past the bound instead of reallocating.
struct LimitedWriter<'a>(&'a mut Vec<u8>);

impl Write for LimitedWriter<'_> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if self.0.len() + data.len() > MAX_BYTES {
            return Err(io::ErrorKind::FileTooLarge.into());
        }
        self.0.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Writes the device into `directory` (created 0700), private to its owner plus a read-only ACL entry for `uid`.
/// A failure removes the partial file, so no token is left behind.
///
/// # Errors
/// Returns `Preflight` without ACL support, otherwise redacted `Backend` errors.
pub(crate) fn write(directory: &Path, bytes: &[u8], uid: u32) -> Result<PathBuf, SandboxError> {
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(directory)
        .map_err(|error| storage(&error))?;
    let path = directory.join(FILE);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .map_err(|error| storage(&error))?;
    let result = file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| storage(&error))
        .and_then(|()| acl::grant(&file, uid, Access::Read));
    match result {
        Ok(()) => Ok(path),
        Err(error) => {
            let _ = std::fs::remove_file(&path);
            Err(error)
        }
    }
}

/// Removes the per-sandbox directory and everything in it; an absent directory succeeds.
///
/// # Errors
/// Returns a redacted `Backend` error when removal fails.
pub(crate) fn remove(directory: &Path) -> Result<(), SandboxError> {
    match std::fs::remove_dir_all(directory) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(storage(&error)),
        _ => Ok(()),
    }
}

fn storage(error: &io::Error) -> SandboxError {
    SandboxError::Backend(format!(
        "per-sandbox file operation failed: {}",
        error.kind()
    ))
}

#[cfg(test)]
#[path = "device_tests.rs"]
mod tests;
