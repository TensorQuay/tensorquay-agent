//! Validated, bounded TOML configuration. Credentials are a separate environment-only input.

use crate::GatewayError;
use serde::Deserialize;
use std::{
    net::SocketAddr,
    path::{Component, Path, PathBuf},
    time::Duration,
};
use tqa_types::SandboxBackend;
use url::Url;

mod admission;
mod agent_teams;
mod builtin_images;
mod computer;
pub(crate) use computer::{MAX_ACTIONS_PER_RUN, MAX_SCREENSHOTS_PER_RUN};
mod desktop;
pub(crate) use agent_teams::AgentTeams;
pub(crate) use builtin_images::BuiltinImages;
mod ca_bundle;
mod host_keys;
mod workspaces;
pub(crate) use workspaces::BusyPolicy;
mod egress;
pub(crate) use egress::EgressConfig;
mod environment;
mod firecracker;
mod mcp;
pub(crate) use mcp::{
    MAX_MCP_SERVERS, McpCatalogue, McpServer, ToolFilter as McpToolFilterInput,
    valid_server_name as valid_mcp_name,
};
#[cfg(test)]
mod ca_file_tests;
mod metrics;
mod profiles;
impl GatewayConfig {
    /// What the file sets that still works but should change, for start-up's log and `tqa-gateway check`: fixed text,
    /// never a value from the file.
    #[must_use]
    pub fn warnings(&self) -> Vec<&'static str> {
        let mut warnings = Vec::new();
        if self.egress.seed.iter().any(|rule| rule.internal)
            && self.egress.internal_never.is_empty()
        {
            warnings.push(
                "egress rules marked internal refuse until [egress] internal_never names this machine's own addresses, \
                 unless the gateway runs in the host's network namespace (it then finds them itself)",
            );
        }
        // SEC-033: never refused, never the value; the start-up log names the connector or engine.
        if self
            .connectors
            .iter()
            .filter_map(|connector| connector.secret_ref.as_deref())
            .chain(
                self.decision_engines
                    .iter()
                    .filter_map(|engine| engine.key_ref.as_deref()),
            )
            .any(tqa_types::is_key_shaped_name)
        {
            warnings.push(
                "a [[connectors]] or [[decision_engines]] entry names a secret that looks like a key, not a name: \
                 rename the secret and the reference (the start-up log names the entry)",
            );
        }
        warnings
    }
}

#[cfg(test)]
mod removed_tests;
mod sandbox;
pub(crate) mod sizes;
mod skills;
mod template_checks;
mod templates;
mod toml_error;
mod web;
pub(crate) use sandbox::SandboxConfig;
pub(crate) use sizes::{Ceiling, Size};
pub use templates::Template;
pub(crate) use templates::{TemplateInput, validate};
pub(crate) use tqa_types::RunLimits;
pub(crate) use web::WebConfig;
mod secrets;
pub use secrets::GatewaySecrets;

const MAX_CONFIG_BYTES: usize = 64 * 1024;

/// Startup settings parsed once from exact TOML; callers cannot mutate the validated fields.
#[derive(Clone, Debug)]
pub struct GatewayConfig {
    pub(crate) listen: SocketAddr,
    pub(crate) metrics_listen: Option<SocketAddr>,
    pub(crate) public_origin: Url,
    pub(crate) storage_root: PathBuf,
    /// Where the install CA for keys to web addresses lives (`[host_keys] ca_directory`, ADR-0037 §6).
    pub(crate) host_ca_directory: PathBuf,
    pub(crate) models_path: PathBuf,
    pub(crate) session_ttl: Duration,
    pub(crate) event_payload_cap_bytes: usize,
    pub(crate) stream_keepalive: Duration,
    pub(crate) backend: SandboxBackend,
    pub(crate) image: Option<tqa_sandbox::ImageRef>,
    pub(crate) templates: Vec<Template>,
    /// The built-ins pinned to an image by `[runtime.builtin_images]` (GW-U-120).
    pub(crate) builtin_images: BuiltinImages,
    /// The template checks' credential-file scan bounds (`[template_checks]`, file-only).
    pub(crate) template_scan: tqa_sandbox::template::secret_files::Bounds,
    /// Shared agent profiles to seed (ADR-0012).
    pub(crate) profiles: Vec<crate::profiles::Profile>,
    // Boxed: rarely set, and `GatewayConfig` is carried by value in many futures.
    pub(crate) egress: Box<egress::EgressConfig>,
    pub(crate) admission: admission::Admission,
    pub(crate) workspaces: workspaces::Workspaces,
    /// `[agent_teams]`: the install switch, off by default, and the team limits (ADR-0034).
    pub(crate) agent_teams: agent_teams::AgentTeams,
    /// `[desktop]`: the Desktop environment's limits (ADR-0039).
    pub(crate) desktop: desktop::Desktop,
    /// `[computer]`: the `computer` tool's per-run caps (ADR-0039 section 4).
    pub(crate) computer: computer::Computer,
    /// `[tasks]`: how tasks behave, file-only.
    pub(crate) tasks: Tasks,
    pub(crate) sandbox: SandboxConfig,
    /// Present exactly when the backend is Firecracker; boxed like `egress`.
    pub(crate) firecracker: Option<Box<firecracker::FirecrackerSettings>>,
    pub(crate) limits: Limits,
    pub(crate) run_limits: RunLimits,
    pub(crate) secret_store: crate::SecretStoreConfig,
    pub(crate) relay: crate::relay::RelayConfig,
    pub(crate) web: Option<WebConfig>,
    // Boxed like `egress`: `GatewayConfig` is carried by value in many futures.
    pub(crate) files: Box<crate::files::FilesConfig>,
    pub(crate) skills: skills::Skills,
    /// The install's MCP server catalogue (`[[mcp.servers]]`, ADR-0011).
    pub(crate) mcp: mcp::McpCatalogue,
    /// The `[[connectors]]` seed (ADR-0031), validated; the database holds the live connectors.
    pub(crate) connectors: Vec<crate::connectors::ConnectorDocument>,
    /// The `[[decision_engines]]` seed (ADR-0013 point 6), validated; the database holds the live engines.
    pub(crate) decision_engines: Vec<crate::decision_engines::EngineDocument>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    listen: SocketAddr,
    #[serde(default)]
    metrics_listen: metrics::Input,
    public_origin: String,
    storage: Storage,
    models_path: PathBuf,
    #[serde(default = "session_ttl")]
    session_ttl: u64,
    #[serde(default = "event_payload_cap")]
    event_payload_cap_bytes: usize,
    #[serde(default = "keepalive")]
    stream_keepalive: u64,
    #[serde(default)]
    runtime: Runtime,
    #[serde(default)]
    sandbox: SandboxConfig,
    firecracker: Option<firecracker::FirecrackerInput>,
    #[serde(default)]
    limits: Limits,
    #[serde(default = "run_limits")]
    run_limits: RunLimits,
    #[serde(default)]
    secret_store: crate::SecretStoreConfig,
    #[serde(default)]
    relay: crate::relay::RelayConfig,
    web: Option<WebConfig>,
    #[serde(default)]
    templates: Vec<templates::TemplateInput>,
    #[serde(default)]
    template_checks: template_checks::TemplateChecksInput,
    #[serde(default)]
    profiles: Vec<crate::profiles::ProfileInput>,
    egress: Option<egress::EgressInput>,
    #[serde(default)]
    admission: admission::AdmissionInput,
    #[serde(default)]
    workspaces: workspaces::WorkspacesInput,
    #[serde(default)]
    agent_teams: agent_teams::AgentTeamsInput,
    #[serde(default)]
    desktop: desktop::DesktopInput,
    #[serde(default)]
    computer: computer::ComputerInput,
    #[serde(default)]
    host_keys: host_keys::HostKeysInput,
    #[serde(default)]
    tasks: Tasks,
    #[serde(default)]
    files: crate::files::FilesConfig,
    #[serde(default)]
    skills: skills::SkillsInput,
    #[serde(default)]
    mcp: mcp::McpInput,
    #[serde(default)]
    connectors: Vec<crate::connectors::ConnectorDocument>,
    #[serde(default)]
    decision_engines: Vec<crate::decision_engines::EngineDocument>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Storage {
    root: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Runtime {
    backend: SandboxBackend,
    image: Option<String>,
    /// `[runtime.builtin_images]`: a built-in environment's image by digest, instead of the engine's tag (GW-U-120).
    #[serde(default)]
    builtin_images: std::collections::BTreeMap<String, String>,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
            backend: SandboxBackend::Docker,
            image: None,
            builtin_images: std::collections::BTreeMap::new(),
        }
    }
}

/// Global resource bounds, not per-user policy. Durations are seconds in TOML.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Limits {
    pub database_connections: u32,
    pub io_timeout: u64,
    pub body_bytes: usize,
    pub ingestion_bytes: usize,
    pub broadcast_capacity: usize,
    pub action_capacity: usize,
    pub live_conversations: usize,
    pub login_addresses: usize,
    pub password_workers: usize,
    /// How long one password check or hash may take, its wait for a worker included: its own bound, not
    /// `io_timeout`, so sign-in survives a loaded host (GW-I-129, the R2 soak at load 31).
    pub password_seconds: u64,
    pub page_size: u32,
    pub idempotency_hours: u32,
    /// How long an invitation link stays valid (task 063).
    pub invitation_hours: u32,
    pub idempotency_purge_seconds: u64,
    pub token_reservation_secs: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            database_connections: 16,
            io_timeout: 10,
            body_bytes: 1024 * 1024,
            ingestion_bytes: 16 * 1024 * 1024,
            broadcast_capacity: 1024,
            action_capacity: 64,
            live_conversations: 4096,
            login_addresses: 10_000,
            password_workers: 4,
            password_seconds: 30,
            page_size: 1000,
            idempotency_hours: 24,
            invitation_hours: 168,
            idempotency_purge_seconds: 60,
            token_reservation_secs: 120,
        }
    }
}

const fn session_ttl() -> u64 {
    86_400
}
const fn event_payload_cap() -> usize {
    64 * 1024
}
const fn keepalive() -> u64 {
    15
}

impl Input {
    /// The self-contained bounds: each value checked on its own, before sections that depend on others.
    fn validate_bounds(&self) -> Result<(), GatewayError> {
        if !(60..=31 * 86_400).contains(&self.session_ttl) {
            return Err(invalid("session_ttl must be 60 through 2678400 seconds"));
        }
        if !(1..=tqa_types::MAX_FRAME_BYTES).contains(&self.event_payload_cap_bytes) {
            return Err(invalid("event_payload_cap_bytes must be 1 through 262144"));
        }
        if !(1..=300).contains(&self.stream_keepalive) {
            return Err(invalid("stream_keepalive must be 1 through 300 seconds"));
        }
        self.limits.validate()?;
        self.run_limits
            .validate()
            .map_err(|_| invalid("run limits exceed protocol bounds"))?;
        self.secret_store.validate()?;
        self.relay.validate()?;
        if let Some(web) = &self.web {
            web.validate()?;
        }
        Ok(())
    }
}

impl GatewayConfig {
    /// Reads at most 64 KiB of TOML within a ten-second startup deadline.
    ///
    /// # Errors
    /// Rejects unavailable, oversized, non-UTF-8 or invalid configuration, or cancelled startup.
    pub async fn read(
        path: &Path,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<Self, GatewayError> {
        // The file's own problems are named; the bounded read stays the backstop (a file changing in between).
        let metadata = crate::io::bounded(Duration::from_secs(10), cancel, async {
            Ok(tokio::fs::metadata(path).await)
        })
        .await?;
        match metadata {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(invalid("the configuration file does not exist"));
            }
            Err(_) => return Err(invalid("the configuration file cannot be read")),
            Ok(metadata) if !metadata.is_file() => {
                return Err(invalid("the configuration path is not a regular file"));
            }
            Ok(metadata) if metadata.len() > MAX_CONFIG_BYTES as u64 => {
                return Err(invalid("configuration exceeds 64 KiB"));
            }
            Ok(_) => {}
        }
        let bytes = crate::io::bounded(
            Duration::from_secs(10),
            cancel,
            crate::io::read_bounded(path, MAX_CONFIG_BYTES),
        )
        .await
        .map_err(|error| match error {
            GatewayError::Io => invalid("the configuration file cannot be read"),
            other => other,
        })?;
        let source =
            std::str::from_utf8(&bytes).map_err(|_| invalid("configuration must be UTF-8"))?;
        Self::from_toml(source)
    }

    /// Parses exact TOML, validates origins, paths and resource bounds, and applies documented defaults.
    ///
    /// # Errors
    /// Returns `Configuration`, or `ConfigurationField` naming a field the TOML parse refused by its path and why;
    /// never a value from the file.
    pub fn from_toml(source: &str) -> Result<Self, GatewayError> {
        if source.len() > MAX_CONFIG_BYTES {
            return Err(invalid("configuration exceeds 64 KiB"));
        }
        // Settings ADR-0035 removed are named before the typed parse, which would only call them unknown.
        let table: toml::Table = toml_error::parse(source)?;
        removed(&table)?;
        let input: Input = toml_error::parse(source)?;
        let origin = public_origin(&input.public_origin)?;
        validate_path(&input.storage.root, true)?;
        validate_path(&input.models_path, false)?;
        input.validate_bounds()?;
        let image = input
            .runtime
            .image
            .as_deref()
            .map(sandbox::image)
            .transpose()?;
        let templates = templates::templates(input.templates, image.as_ref())?;
        let builtin_images = BuiltinImages::new(input.runtime.builtin_images.clone())?;
        let profiles = profiles::profiles(input.profiles)?;
        let mcp = mcp::mcp(input.mcp, &templates)?;
        let connectors = connectors_seed(input.connectors, &mcp)?;
        let decision_engines = engines_seed(input.decision_engines)?;
        let egress = egress::egress(input.egress, input.listen)?;
        let admission = admission::admission(input.admission, input.sandbox.max_live)?;
        let workspaces = workspaces::workspaces(input.workspaces)?;
        input.sandbox.validate(input.runtime.backend)?;
        sizes_within_ceiling(&input.sandbox.ceiling, &templates, &workspaces)?;
        let firecracker = firecracker::firecracker(
            input.firecracker,
            &firecracker::Context {
                backend: input.runtime.backend,
                storage_root: &input.storage.root,
                sandbox: &input.sandbox,
                egress_listen: (!egress.disabled).then_some(egress.listen),
            },
        )?;
        let skills = skills::skills(input.skills, &input.storage.root)?;
        let host_ca_directory =
            host_keys::ca_directory(input.host_keys, &input.storage.root, skills.root.as_deref())?;
        let metrics_listen = input.metrics_listen.address(input.listen)?;
        Ok(Self {
            listen: input.listen,
            metrics_listen,
            public_origin: origin,
            storage_root: input.storage.root,
            host_ca_directory,
            models_path: input.models_path,
            session_ttl: Duration::from_secs(input.session_ttl),
            event_payload_cap_bytes: input.event_payload_cap_bytes,
            stream_keepalive: Duration::from_secs(input.stream_keepalive),
            backend: input.runtime.backend,
            image,
            templates,
            builtin_images,
            template_scan: template_checks::template_checks(input.template_checks)?,
            profiles,
            egress,
            admission,
            workspaces,
            agent_teams: agent_teams::agent_teams(input.agent_teams)?,
            desktop: desktop::desktop(&input.desktop)?,
            computer: computer::computer(&input.computer)?,
            tasks: input.tasks,
            sandbox: input.sandbox,
            firecracker,
            limits: input.limits,
            run_limits: input.run_limits,
            secret_store: input.secret_store,
            relay: input.relay.for_live_runs(admission.per_user),
            web: input.web,
            files: Box::new(input.files.validate()?),
            skills,
            mcp,
            connectors,
            decision_engines,
        })
    }

    /// The shared agent profiles `[[profiles]]` seeds, in file order.
    pub(crate) fn profiles(&self) -> &[crate::profiles::Profile] {
        &self.profiles
    }

    /// The built-ins pinned to an image by digest (`[runtime.builtin_images]`); the rest are found by their tag.
    pub(crate) const fn builtin_images(&self) -> &BuiltinImages {
        &self.builtin_images
    }

    /// Returns the environment templates in configured order; exactly one is the default.
    #[must_use]
    pub fn templates(&self) -> &[Template] {
        &self.templates
    }

    /// Returns the policy proxy's listener, which always runs unless `[egress] disabled = true` (ADR-0028).
    #[must_use]
    pub fn egress_listen(&self) -> Option<SocketAddr> {
        (!self.egress.disabled).then_some(self.egress.listen)
    }

    /// Returns the validated public origin used by CSRF and cookies.
    #[must_use]
    pub fn public_origin(&self) -> &Url {
        &self.public_origin
    }
    /// Returns the maximum session lifetime.
    #[must_use]
    pub const fn session_ttl(&self) -> Duration {
        self.session_ttl
    }
    /// Returns the size threshold above which a serialized event is stored as a blob.
    #[must_use]
    pub const fn event_payload_cap_bytes(&self) -> usize {
        self.event_payload_cap_bytes
    }
    /// Returns the SSE comment interval.
    #[must_use]
    pub const fn stream_keepalive(&self) -> Duration {
        self.stream_keepalive
    }
    /// Where the install CA for keys to web addresses lives (`[host_keys] ca_directory`).
    #[must_use]
    pub fn host_ca_directory(&self) -> &std::path::Path {
        &self.host_ca_directory
    }
    /// Returns the socket configured for the process listener.
    #[must_use]
    pub const fn listen(&self) -> SocketAddr {
        self.listen
    }
    /// Returns the separate metrics listener, or `None` when metrics are disabled.
    #[must_use]
    pub const fn metrics_listen(&self) -> Option<SocketAddr> {
        self.metrics_listen
    }
    /// Whether workspaces and runtime state are block images (the microVM backend): the one test every gateway path
    /// uses, so directory handling and backend selection cannot drift apart.
    pub(crate) const fn uses_block_images(&self) -> bool {
        self.firecracker.is_some()
    }

    /// The configured backend's stable name, as readiness and conversations report it.
    pub(crate) const fn backend_name(&self) -> &'static str {
        match self.backend {
            SandboxBackend::Firecracker => "firecracker",
            _ => "docker",
        }
    }

    pub(crate) const fn io_timeout(&self) -> Duration {
        Duration::from_secs(self.limits.io_timeout)
    }

    /// One password operation's bound, its wait for a worker included (`[limits] password_seconds`).
    pub(crate) const fn password_bound(&self) -> Duration {
        Duration::from_secs(self.limits.password_seconds)
    }
}

impl Limits {
    fn validate(&self) -> Result<(), GatewayError> {
        let valid = (1..=64).contains(&self.database_connections)
            && (1..=300).contains(&self.io_timeout)
            && (1024..=16 * 1024 * 1024).contains(&self.body_bytes)
            && (tqa_types::MAX_FRAME_BYTES..=16 * 1024 * 1024).contains(&self.ingestion_bytes)
            && (1..=65_536).contains(&self.broadcast_capacity)
            && (1..=4096).contains(&self.action_capacity)
            && (1..=65_536).contains(&self.live_conversations)
            && (1..=100_000).contains(&self.login_addresses)
            && (1..=16).contains(&self.password_workers)
            && (1..=300).contains(&self.password_seconds)
            && (1..=1000).contains(&self.page_size)
            && (1..=8760).contains(&self.idempotency_hours)
            && (1..=720).contains(&self.invitation_hours)
            && (1..=3600).contains(&self.idempotency_purge_seconds)
            && (1..=600).contains(&self.token_reservation_secs);
        if !valid {
            return Err(invalid("global limits exceed their supported bounds"));
        }
        Ok(())
    }
}

fn validate_path(path: &Path, root: bool) -> Result<(), GatewayError> {
    let valid = path
        .to_str()
        .is_some_and(|s| !s.is_empty() && s.len() <= 4096 && !s.contains('\0'))
        && !path.components().any(|c| c == Component::ParentDir)
        && (!root || (path.is_absolute() && path.parent().is_some()));
    if !valid {
        return Err(invalid(
            "storage and model paths must be bounded and traversal-free; storage must be absolute and non-root",
        ));
    }
    Ok(())
}

/// The per-call permission settings ADR-0035 removed, and the Git settings ADR-0037 §7 removed, stop start-up with what
/// to delete or change; ignoring them would leave an administrator believing a rule still applies.
fn removed(table: &toml::Table) -> Result<(), GatewayError> {
    fn entries(value: Option<&toml::Value>) -> Vec<&toml::Table> {
        value
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(toml::Value::as_table)
            .collect()
    }
    if table.contains_key("git_push") {
        return Err(invalid(
            "gateway.toml: [git_push] was removed in this release (ADR-0037 §7): name its ca_file as [egress] ca_file \
             instead, and delete [git_push]",
        ));
    }
    if table.contains_key("permissions") {
        return Err(invalid(
            "gateway.toml: [permissions] was removed (ADR-0035): there are no per-call permissions or approvals, and \
             the network allow-list is the guardrail; delete [permissions] from the file",
        ));
    }
    if table
        .get("sandbox")
        .and_then(toml::Value::as_table)
        .is_some_and(|sandbox| sandbox.contains_key("park_after_seconds"))
    {
        return Err(invalid(
            "gateway.toml: [sandbox] park_after_seconds was removed (ADR-0035): runs no longer wait for approvals, so \
             nothing parks; delete park_after_seconds",
        ));
    }
    if entries(table.get("profiles"))
        .iter()
        .any(|profile| profile.contains_key("policy") || profile.contains_key("rules"))
    {
        return Err(invalid(
            "gateway.toml: a [[profiles]] entry's policy and rules were removed (ADR-0035): a profile chooses its \
             tools, and the network allow-list is the guardrail; delete policy and rules",
        ));
    }
    let git_push = |profile: &&toml::Table| {
        profile
            .get("tools")
            .and_then(toml::Value::as_array)
            .is_some_and(|tools| tools.iter().any(|tool| tool.as_str() == Some("git_push")))
    };
    if entries(table.get("profiles")).iter().any(git_push) {
        return Err(invalid(
            "gateway.toml: a [[profiles]] entry's tools no longer take git_push (removed in this release, ADR-0037 \
             §7): tasks clone and push with plain git and a key under Keys; delete git_push from tools",
        ));
    }
    if entries(table.get("profiles"))
        .iter()
        .any(|profile| profile.contains_key("repositories"))
    {
        return Err(invalid(
            "gateway.toml: a [[profiles]] entry's repositories was removed in this release (ADR-0037 §7): tasks clone \
             and push with plain git and a key under Keys; delete repositories",
        ));
    }
    let servers = entries(
        table
            .get("mcp")
            .and_then(toml::Value::as_table)
            .and_then(|mcp| mcp.get("servers")),
    );
    if servers
        .iter()
        .chain(entries(table.get("connectors")).iter())
        .any(|entry| entry.contains_key("class"))
    {
        return Err(invalid(
            "gateway.toml: the class of [[mcp.servers]] and [[connectors]] entries was removed (ADR-0035): it only \
             chose per-call permissions; delete class",
        ));
    }
    Ok(())
}

/// `[tasks]` (file-only).
#[derive(Clone, Copy, Debug, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Tasks {
    /// Name a task in a few words after its first run completes, with one small call on its route (on by default).
    pub auto_title: bool,
}

impl Default for Tasks {
    fn default() -> Self {
        Self { auto_title: true }
    }
}

/// The file's environments and the default workspace quota fit the installation's ceiling.
fn sizes_within_ceiling(
    ceiling: &sizes::Ceiling,
    templates: &[Template],
    workspaces: &workspaces::Workspaces,
) -> Result<(), GatewayError> {
    if workspaces.quota_bytes > ceiling.workspace_bytes() {
        return Err(invalid(
            "[workspaces] quota_bytes must be within [sandbox.ceiling] workspace_mib",
        ));
    }
    if templates
        .iter()
        .any(|template| ceiling.exceeded_by(&template.size()).is_some())
    {
        return Err(invalid(
            "a template's size is above the installation's [sandbox.ceiling] (limit_out_of_range)",
        ));
    }
    Ok(())
}

fn invalid(reason: &'static str) -> GatewayError {
    GatewayError::Configuration(reason)
}

fn run_limits() -> RunLimits {
    RunLimits {
        max_steps: 100,
        wall_clock_secs: 1800,
        tool_result_bytes: 8192,
        ..RunLimits::default()
    }
}

/// `public_origin`: an HTTP(S) origin with only a scheme, a host and an optional port.
fn public_origin(value: &str) -> Result<Url, GatewayError> {
    let origin =
        Url::parse(value).map_err(|_| invalid("public_origin must be an HTTP(S) origin"))?;
    if !matches!(origin.scheme(), "http" | "https")
        || origin.host_str().is_none()
        || !origin.username().is_empty()
        || origin.password().is_some()
        || origin.path() != "/"
        || origin.query().is_some()
        || origin.fragment().is_some()
    {
        return Err(invalid(
            "public_origin must contain only scheme, host and optional port",
        ));
    }
    Ok(origin)
}

/// The most decision engines the file may seed.
const MAX_SEED_ENGINES: usize = 16;

/// Validates the `[[decision_engines]]` seed with the API's validator (the file may also use the file-only fields). A
/// refusal is logged with its field and reason and stops the gateway.
fn engines_seed(
    seed: Vec<crate::decision_engines::EngineDocument>,
) -> Result<Vec<crate::decision_engines::EngineDocument>, GatewayError> {
    if seed.len() > MAX_SEED_ENGINES {
        return Err(invalid("too many [[decision_engines]] (at most 16)"));
    }
    let mut names = std::collections::BTreeSet::new();
    for document in &seed {
        if let Err((field, reason)) =
            crate::decision_engines::validate(document, crate::connectors::Origin::File)
        {
            tracing::error!(
                field,
                reason = reason.as_str(),
                "invalid [[decision_engines]] entry"
            );
            return Err(invalid("invalid [[decision_engines]] entry (see the log)"));
        }
        if !names.insert(document.name.as_str()) {
            return Err(invalid("duplicate [[decision_engines]] name"));
        }
    }
    Ok(seed)
}

/// The most connectors the file may seed.
const MAX_SEED_CONNECTORS: usize = 64;

/// Validates the `[[connectors]]` seed with the API's validator (the file may also use the file-only fields). A refusal
/// is logged with its field and reason and stops the gateway.
fn connectors_seed(
    seed: Vec<crate::connectors::ConnectorDocument>,
    mcp: &mcp::McpCatalogue,
) -> Result<Vec<crate::connectors::ConnectorDocument>, GatewayError> {
    use crate::connectors::{Origin, Refusal, validate};
    if seed.len() > MAX_SEED_CONNECTORS {
        return Err(invalid("too many [[connectors]] (at most 64)"));
    }
    let stdio: Vec<String> = mcp
        .servers()
        .iter()
        .map(|server| server.name().to_owned())
        .collect();
    let mut names = std::collections::BTreeSet::new();
    for document in &seed {
        match validate(document, Origin::File, &stdio) {
            Ok(()) => {}
            Err(Refusal::NameTaken) => {
                return Err(invalid("a [[connectors]] name is taken by [[mcp.servers]]"));
            }
            Err(Refusal::Invalid(field, reason)) => {
                tracing::error!(
                    field,
                    reason = reason.as_str(),
                    "invalid [[connectors]] entry"
                );
                return Err(invalid("invalid [[connectors]] entry (see the log)"));
            }
        }
        if !names.insert(document.name.as_str()) {
            return Err(invalid("duplicate [[connectors]] name"));
        }
    }
    Ok(seed)
}
