//! Bounded, credential-free Git transport for workspace initialization.
use crate::{ProxyEndpoint, builtin::shell::group::ProcessGroup};
use async_trait::async_trait;
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command};
use tokio_util::sync::CancellationToken;
use tqa_types::{Sleeper, WorkspaceClone};

const CLONE_TIMEOUT: Duration = Duration::from_mins(2);
const DIAGNOSTIC_BYTES: usize = 16 * 1024;

/// Validated repository, new checkout path and mandatory policy proxy for one clone.
pub struct CloneRequest<'a> {
    /// Public HTTPS URL and optional branch/tag; revalidated before any child starts.
    pub repository: &'a WorkspaceClone,
    /// Absent checkout directory inside a caller-owned staging directory.
    pub destination: &'a Path,
    /// Trusted gateway proxy; direct network access is never a fallback.
    pub proxy: &'a ProxyEndpoint,
}
/// Stable clone failure without a URL, process output or host diagnostic.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum CloneError {
    /// The policy proxy refused the CONNECT tunnel.
    #[error("repository access was denied by egress policy")]
    EgressDenied,
    /// Child execution, TLS, transfer or checkout failed.
    #[error("repository clone failed")]
    Io,
    /// The bounded operation reached its deadline.
    #[error("repository clone timed out")]
    Timeout,
    /// Shutdown interrupted the operation after its process group was stopped.
    #[error("repository clone was interrupted")]
    Stopped,
}
/// Injectable repository transport; the caller owns staging and installation.
#[async_trait]
pub trait RepositoryCloner: Send + Sync {
    /// Clones through the supplied proxy, observing cancellation and joining child cleanup.
    ///
    /// # Errors
    /// Reports a classified failure; partial staging content must never be treated as installed.
    async fn clone_repository(
        &self,
        input: CloneRequest<'_>,
        cancel: &CancellationToken,
    ) -> Result<(), CloneError>;
}
/// Git argv execution with a 120-second deadline and at most 16 KiB of retained diagnostics.
pub struct GitCloner {
    sleeper: Arc<dyn Sleeper>,
    ca_bundle: Option<PathBuf>,
}
impl GitCloner {
    /// Uses Git's system trust store and an explicit deadline source.
    #[must_use]
    pub fn new(sleeper: Arc<dyn Sleeper>) -> Self {
        Self {
            sleeper,
            ca_bundle: None,
        }
    }
    /// Uses an explicitly injected trusted CA bundle; TLS certificate verification remains enabled.
    #[must_use]
    pub fn with_ca_bundle(mut self, path: PathBuf) -> Self {
        self.ca_bundle = Some(path);
        self
    }
    async fn execute(
        &self,
        mut command: Command,
        cancel: &CancellationToken,
    ) -> Result<(), CloneError> {
        if cancel.is_cancelled() {
            return Err(CloneError::Stopped);
        }
        let mut group = ProcessGroup::spawn(&mut command).map_err(|_| CloneError::Io)?;
        let result = tokio::select! {biased;
            () = cancel.cancelled() => Err(CloneError::Stopped),
            () = self.sleeper.sleep(CLONE_TIMEOUT) => Err(CloneError::Timeout),
            result = tokio::time::timeout(CLONE_TIMEOUT, collect(&mut group)) => result.unwrap_or(Err(CloneError::Timeout)),
        };
        group
            .finish(self.sleeper.as_ref())
            .await
            .map_err(|_| CloneError::Io)?;
        result
    }
}
#[async_trait]
impl RepositoryCloner for GitCloner {
    async fn clone_repository(
        &self,
        input: CloneRequest<'_>,
        cancel: &CancellationToken,
    ) -> Result<(), CloneError> {
        input.repository.validate().map_err(|_| CloneError::Io)?;
        let mut command = git_command(GIT_PATH)?;
        configure(&mut command, &input, self.ca_bundle.as_deref())?;
        self.execute(command, cancel).await
    }
}
/// The `PATH` the clone gets, and where `git` is looked up.
const GIT_PATH: &str = "/usr/local/bin:/usr/bin:/bin";

/// `git` as a tool process, found on `path`; a missing `git` is `Io`, as a failed start always was.
fn git_command(path: &str) -> Result<Command, CloneError> {
    crate::process_score::tool_command("git", Some(std::ffi::OsStr::new(path)))
        .map_err(|_| CloneError::Io)
}

fn configure(
    command: &mut Command,
    input: &CloneRequest<'_>,
    ca_bundle: Option<&Path>,
) -> Result<(), CloneError> {
    let home = input.destination.parent().ok_or(CloneError::Io)?;
    command.env_clear().current_dir(home);
    for (key, value) in [
        ("PATH", GIT_PATH),
        ("LANG", "C"),
        ("LC_ALL", "C"),
        ("GIT_CONFIG_NOSYSTEM", "1"),
        ("GIT_CONFIG_GLOBAL", "/dev/null"),
        ("GIT_TERMINAL_PROMPT", "0"),
        ("GIT_ASKPASS", "/bin/false"),
        ("HTTPS_PROXY", input.proxy.as_str()),
        ("https_proxy", input.proxy.as_str()),
        ("NO_PROXY", ""),
        ("no_proxy", ""),
    ] {
        command.env(key, value);
    }
    command.envs(crate::GIT_SAFETY_ENV);
    command.env("HOME", home).env("XDG_CONFIG_HOME", home);
    if let Some(bundle) = ca_bundle {
        command.env("GIT_SSL_CAINFO", bundle);
    }
    for config in [
        "protocol.allow=never",
        "protocol.https.allow=always",
        "credential.helper=",
        "http.sslVerify=true",
        "http.followRedirects=false",
        "http.extraHeader=",
        "core.hooksPath=/dev/null",
        "submodule.recurse=false",
    ] {
        command.args(["-c", config]);
    }
    command.args([
        "clone",
        "--quiet",
        "--no-local",
        "--no-recurse-submodules",
        "--template=",
    ]);
    if let Some(reference) = &input.repository.r#ref {
        command.args(["--branch", reference]);
    }
    command
        .arg("--")
        .arg(&input.repository.url)
        .arg(input.destination);
    child_io(command);
    Ok(())
}
fn child_io(command: &mut Command) {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true);
}
async fn collect(group: &mut ProcessGroup) -> Result<(), CloneError> {
    let mut stderr = group.child.stderr.take().ok_or(CloneError::Io)?;
    let mut diagnostic = VecDeque::with_capacity(DIAGNOSTIC_BYTES);
    let mut bytes = [0; 4096];
    loop {
        let length = stderr.read(&mut bytes).await.map_err(|_| CloneError::Io)?;
        if length == 0 {
            break;
        }
        for &byte in &bytes[..length] {
            if diagnostic.len() == DIAGNOSTIC_BYTES {
                diagnostic.pop_front();
            }
            diagnostic.push_back(byte);
        }
    }
    let status = group.child.wait().await.map_err(|_| CloneError::Io)?;
    group.reaped = true;
    if status.success() {
        return Ok(());
    }
    let diagnostic = diagnostic.make_contiguous();
    if memchr::memmem::find(diagnostic, b"CONNECT tunnel failed, response 403").is_some()
        || memchr::memmem::find(
            diagnostic,
            b"Received HTTP code 403 from proxy after CONNECT",
        )
        .is_some()
    {
        Err(CloneError::EgressDenied)
    } else {
        Err(CloneError::Io)
    }
}

#[cfg(test)]
mod tests;
