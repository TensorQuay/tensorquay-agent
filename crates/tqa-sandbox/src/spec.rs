use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

use secrecy::{ExposeSecret, SecretString};
use tqa_types::{ConversationId, WorkspaceId};
use url::Url;

use crate::{
    BackendKind, ByteSize, CpuLimit, EgressPolicy, ImageRef, SandboxError, egress::valid_url,
};

/// Host-local storage for the sandbox's single model-visible workspace.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceSource {
    /// An absolute directory, canonicalised before Docker creation.
    HostDirectory(PathBuf),
    /// An existing Docker-managed volume, provisioned for uid 1000 by its owner.
    NamedVolume(String),
    /// A block image for a future microVM backend; Docker rejects it.
    BlockImage(PathBuf),
}

/// Stable workspace identity and its separately provisioned storage source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceMount {
    /// Validated workspace identity owned and authorised by the gateway.
    pub id: WorkspaceId,
    /// The single permitted workspace source.
    pub source: WorkspaceSource,
}

/// Host-local storage for the conversation's runtime state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StateSource {
    /// An existing absolute host directory, canonicalised before Docker creation and bind-mounted.
    HostDirectory(PathBuf),
    /// An ext4 image file attached as a block device by the microVM backend; Docker rejects it.
    BlockImage(PathBuf),
}

/// Per-conversation runtime state, separate from every model-visible workspace.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StateMount {
    /// The single permitted state source.
    pub source: StateSource,
}

impl StateMount {
    /// State in a host directory, the Docker backend's form.
    #[must_use]
    pub fn directory(path: impl Into<PathBuf>) -> Self {
        Self {
            source: StateSource::HostDirectory(path.into()),
        }
    }

    /// The host directory, when the state is one.
    #[must_use]
    pub fn host_directory(&self) -> Option<&Path> {
        match &self.source {
            StateSource::HostDirectory(path) => Some(path),
            StateSource::BlockImage(_) => None,
        }
    }
}

/// Operator and per-user skill directories, bind-mounted read-only at fixed paths when set.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SkillsMounts {
    /// The install tier, mounted at `/skills/install`.
    pub install: Option<PathBuf>,
    /// The conversation owner's tier, mounted at `/skills/user`.
    pub user: Option<PathBuf>,
}

impl SkillsMounts {
    /// Each configured tier with its fixed target inside the sandbox.
    pub fn tiers(&self) -> impl Iterator<Item = (&Path, &'static str)> {
        [
            (self.install.as_deref(), "/skills/install"),
            (self.user.as_deref(), "/skills/user"),
        ]
        .into_iter()
        .filter_map(|(path, target)| path.map(|path| (path, target)))
    }

    /// Whether no tier is configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.install.is_none() && self.user.is_none()
    }
}

/// Sandbox creation input. The short-lived runtime token is redacted by its secret type.
#[derive(Clone, Debug)]
pub struct SandboxSpec {
    /// Conversation binding; authorisation and the write lease belong to the gateway.
    pub conversation_id: ConversationId,
    /// The model-visible workspace storage.
    pub workspace: WorkspaceMount,
    /// Private runtime state mounted separately at `/run/tqa/state`.
    pub state: StateMount,
    /// Immutable image identity.
    pub image: ImageRef,
    /// CPU bandwidth limit.
    pub cpu: CpuLimit,
    /// Physical memory and equal total memory-plus-swap limit.
    pub memory: ByteSize,
    /// Optional disk quota; unsupported Docker requests fail instead of being ignored.
    pub disk: Option<ByteSize>,
    /// Positive process limit, at most 65,536.
    pub pids: u32,
    /// Explicit external routing policy.
    pub egress: EgressPolicy,
    /// Runtime registration endpoint, preserved unchanged in the environment.
    pub gateway_url: Url,
    /// A short-lived credential delivered only through a private read-only file mount.
    pub runtime_token: SecretString,
    /// At most 64 non-secret entries; reserved and secret-looking names are rejected.
    pub env: BTreeMap<String, String>,
    /// Read-only skill directories; the microVM backend refuses them until it has a block image of the trees.
    pub skills: SkillsMounts,
    /// An environment declaring `desktop` (ADR-0039 section 10): a 1 GiB `/dev/shm`, the browser's own tmpfs at
    /// `/run/tqa-browser` and the display's framebuffer file at `/run/tqa-display`. The microVM backend refuses it until its rootfs has the Desktop.
    pub desktop: bool,
}

impl SandboxSpec {
    /// Checks all creation input before any Docker side effect.
    ///
    /// # Errors
    /// Returns `InvalidSpec` for unsupported storage, invalid endpoints or unbounded input.
    pub fn validate(&self, kind: BackendKind) -> Result<(), SandboxError> {
        validate_workspace(&self.workspace.source, kind)?;
        self.validate_state(kind)?;
        self.validate_skills(kind)?;
        if kind == BackendKind::Firecracker && self.desktop {
            return Err(SandboxError::invalid(
                "desktop",
                "the microVM backend has no Desktop rootfs yet",
            ));
        }
        if kind == BackendKind::Docker && self.disk.is_some() {
            return Err(SandboxError::invalid(
                "disk",
                "Docker workspace quotas require external provisioning",
            ));
        }
        if !(1..=65_536).contains(&self.pids) {
            return Err(SandboxError::invalid(
                "pids",
                "expected 1 through 65536 processes",
            ));
        }
        if !valid_url(&self.gateway_url, &["http", "https", "ws", "wss"]) {
            return Err(SandboxError::invalid(
                "gateway_url",
                "expected a credential-free runtime endpoint",
            ));
        }
        let token = self.runtime_token.expose_secret();
        if token.is_empty() || token.len() > 8192 || token.contains('\0') {
            return Err(SandboxError::invalid(
                "runtime_token",
                "invalid short-lived runtime credential",
            ));
        }
        validate_env(&self.env)?;
        self.egress.validate()
    }

    fn validate_state(&self, kind: BackendKind) -> Result<(), SandboxError> {
        if kind == BackendKind::Docker && matches!(self.state.source, StateSource::BlockImage(_)) {
            return Err(SandboxError::invalid(
                "state",
                "block images require the microVM backend",
            ));
        }
        let (StateSource::HostDirectory(path) | StateSource::BlockImage(path)) = &self.state.source;
        if !path.is_absolute()
            || path.parent().is_none()
            || path
                .to_str()
                .is_none_or(|value| value.len() > 4096 || value.contains('\0'))
            || path.components().any(|part| part == Component::ParentDir)
        {
            return Err(SandboxError::invalid(
                "state",
                "expected an absolute non-root state path without traversal",
            ));
        }
        if let WorkspaceSource::HostDirectory(workspace) = &self.workspace.source
            && (path.starts_with(workspace) || workspace.starts_with(path))
        {
            return Err(SandboxError::invalid(
                "state",
                "state must not overlap workspace storage",
            ));
        }
        Ok(())
    }
}

impl SandboxSpec {
    fn validate_skills(&self, kind: BackendKind) -> Result<(), SandboxError> {
        if kind == BackendKind::Firecracker && !self.skills.is_empty() {
            return Err(SandboxError::invalid(
                "skills",
                "read-only skills mounts are not available on the microVM backend yet",
            ));
        }
        let workspace = match &self.workspace.source {
            WorkspaceSource::HostDirectory(path) => Some(path.as_path()),
            WorkspaceSource::NamedVolume(_) | WorkspaceSource::BlockImage(_) => None,
        };
        let (StateSource::HostDirectory(state) | StateSource::BlockImage(state)) =
            &self.state.source;
        for (path, _) in self.skills.tiers() {
            let overlaps = |other: &Path| path.starts_with(other) || other.starts_with(path);
            if !path.is_absolute()
                || path.parent().is_none()
                || path
                    .to_str()
                    .is_none_or(|value| value.len() > 4096 || value.contains('\0'))
                || path.components().any(|part| part == Component::ParentDir)
                || overlaps(state)
                || workspace.is_some_and(overlaps)
            {
                return Err(SandboxError::invalid(
                    "skills",
                    "expected an absolute skills directory apart from workspace and state storage",
                ));
            }
        }
        Ok(())
    }
}

fn validate_workspace(source: &WorkspaceSource, kind: BackendKind) -> Result<(), SandboxError> {
    let valid = match source {
        WorkspaceSource::HostDirectory(path) | WorkspaceSource::BlockImage(path) => {
            !(kind == BackendKind::Docker && matches!(source, WorkspaceSource::BlockImage(_)))
                && path.is_absolute()
                && path.parent().is_some()
                && path.to_str().is_some()
                && !path
                    .components()
                    .any(|part| matches!(part, Component::ParentDir))
        }
        WorkspaceSource::NamedVolume(name) => {
            !name.is_empty()
                && name.len() <= 128
                && name.starts_with(|character: char| character.is_ascii_alphanumeric())
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
        }
    };
    if valid {
        Ok(())
    } else {
        Err(SandboxError::invalid(
            "workspace",
            "unsupported or malformed workspace source",
        ))
    }
}

fn validate_env(env: &BTreeMap<String, String>) -> Result<(), SandboxError> {
    if env.len() > 64 {
        return Err(SandboxError::invalid(
            "env",
            "too many environment variables",
        ));
    }
    for (key, value) in env {
        let name = key.to_ascii_lowercase();
        let secret = ["key", "token", "secret", "password", "credential"]
            .iter()
            .any(|part| name.contains(part));
        let reserved = name.starts_with("tqa_")
            || ["http_proxy", "https_proxy", "no_proxy", "all_proxy"].contains(&name.as_str());
        if key.is_empty()
            || key.len() > 128
            || !key
                .starts_with(|character: char| character.is_ascii_alphabetic() || character == '_')
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            || secret
            || reserved
            || value.len() > 8192
            || value.contains('\0')
        {
            return Err(SandboxError::invalid(
                "env",
                "invalid, reserved or secret-looking environment entry",
            ));
        }
    }
    Ok(())
}
