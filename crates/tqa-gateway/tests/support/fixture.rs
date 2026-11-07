//! Owned, bounded HTTP fixtures; every database test opts in explicitly.
#![allow(dead_code)] // Integration targets compile this shared fixture independently.

#[path = "accounts.rs"]
mod accounts;
#[path = "backend.rs"]
mod backend;
#[path = "clock.rs"]
mod clock;
#[path = "runtime.rs"]
pub mod runtime;
#[path = "stream.rs"]
pub mod stream;
#[path = "workspace_link.rs"]
pub mod workspace_link;
use futures::FutureExt;
use secrecy::SecretString;
use sqlx::PgPool;
use std::{panic::AssertUnwindSafe, time::Duration};
use tqa_gateway::{Conversation, Gateway, GatewayConfig, GatewaySecrets};
use tqa_types::{ConversationId, Event, ServerFrame};

pub type TestResult = Result<(), Box<dyn std::error::Error>>;
pub const DEADLINE: Duration = Duration::from_secs(10);

pub struct Fixture {
    pub client: reqwest::Client,
    stream_client: reqwest::Client,
    pub pool: PgPool,
    pub gateway: Gateway,
    pub backend: std::sync::Arc<backend::Backend>,
    pub clock: std::sync::Arc<clock::Clock>,
    config: GatewayConfig,
    /// The gateway file before the test's extra configuration.
    base_config: String,
    secrets: GatewaySecrets,
    registry: tqa_llm::Registry,
    storage: tempfile::TempDir,
    credentials: std::sync::Mutex<std::collections::HashMap<String, SecretString>>,
    origin: String,
    password: String,
    bootstrap: String,
    /// The running server, taken while a restart or shutdown awaits it: a finished handle must never be awaited again,
    /// or a restart that failed after the old server ended panics in the clean-up and hides its own error.
    server: Option<tokio::task::JoinHandle<Result<(), tqa_gateway::GatewayError>>>,
    /// Team responses that didn't match openapi.json, from gateways a restart replaced (GW-U-142).
    contract: std::sync::Mutex<Vec<String>>,
}

pub async fn run<F>(pool: PgPool, test: F) -> TestResult
where
    F: for<'a> AsyncFnOnce(&'a Fixture) -> TestResult,
{
    require_database()?;
    run_configured(pool, None, "", test).await
}

pub async fn run_restarting<F>(pool: PgPool, test: F) -> TestResult
where
    F: for<'a> AsyncFnOnce(&'a mut Fixture) -> TestResult,
{
    run_restarting_with(pool, None, "", test).await
}

/// As `run_restarting`, starting from `registry` as the models file and `extra_config` appended to the gateway file;
/// `Fixture::reseed` and `Fixture::reconfigure` change them for the next restart (the install key is kept).
pub async fn run_restarting_with<F>(
    pool: PgPool,
    registry: Option<tqa_llm::Registry>,
    extra_config: &str,
    test: F,
) -> TestResult
where
    F: for<'a> AsyncFnOnce(&'a mut Fixture) -> TestResult,
{
    require_database()?;
    let mut fixture = Fixture::new(pool, registry, extra_config).await?;
    let outcome = AssertUnwindSafe(test(&mut fixture)).catch_unwind().await;
    let cleanup = fixture.shutdown().await;
    match outcome {
        // The test's own error first: a clean-up that fails after it must not hide it.
        Ok(result) => result.and(cleanup),
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

pub async fn run_configured<F>(
    pool: PgPool,
    registry: Option<tqa_llm::Registry>,
    extra_config: &str,
    test: F,
) -> TestResult
where
    F: for<'a> AsyncFnOnce(&'a Fixture) -> TestResult,
{
    require_database()?;
    // Boxed so a test's own future stays small (clippy::large_futures) however large the fixture grows.
    let fixture = Box::new(Box::pin(Fixture::new(pool, registry, extra_config)).await?);
    let outcome = AssertUnwindSafe(Box::pin(test(&fixture)))
        .catch_unwind()
        .await;
    let cleanup = fixture.shutdown().await;
    match outcome {
        // The test's own error first: a clean-up that fails after it must not hide it.
        Ok(result) => result.and(cleanup),
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

/// The fixture's configuration with a `[firecracker]` section added, when `TQA_GATEWAY_TEST_BACKEND=firecracker` asks
/// for one and the test did not configure the backend itself.
fn forced_firecracker(extra_config: &str) -> Option<String> {
    let forced =
        std::env::var("TQA_GATEWAY_TEST_BACKEND").is_ok_and(|backend| backend == "firecracker");
    let configured = extra_config
        .lines()
        .any(|line| line.trim() == "[firecracker]");
    (forced && !configured).then(|| {
        format!(
            "{extra_config}\n[firecracker]\nvmm_image='sha256:{}'\ngateway_address='192.0.2.10'\n",
            "b".repeat(64)
        )
    })
}

/// An install key made in `<root>/secrets`, which is then read-only (0500), and the storage root made beforehand.
fn read_only_install_key(
    root: &std::path::Path,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    use std::{
        io::Write as _, os::unix::fs::OpenOptionsExt as _, os::unix::fs::PermissionsExt as _,
    };
    let secrets = root.join("secrets");
    std::fs::create_dir(&secrets)?;
    // The gateway makes its storage root private (0700) itself; it can't make it under a read-only parent later.
    std::fs::create_dir_all(root.join("storage"))?;
    std::fs::set_permissions(root.join("storage"), std::fs::Permissions::from_mode(0o700))?;
    let key = secrets.join("install.key");
    let bytes: [u8; 32] =
        std::array::from_fn(|index| u8::try_from(index * 7 % 251).unwrap_or(0) ^ 0x5a);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&key)?
        .write_all(&bytes)?;
    std::fs::set_permissions(&secrets, std::fs::Permissions::from_mode(0o500))?;
    Ok(key)
}

/// As [`run_configured`], with the secret files on a read-only directory, as a deployment mounts them (GW-I-179).
pub async fn run_read_only_secrets<F>(pool: PgPool, extra_config: &str, test: F) -> TestResult
where
    F: for<'a> AsyncFnOnce(&'a Fixture) -> TestResult,
{
    require_database()?;
    let fixture = Box::new(Box::pin(Fixture::new_with(pool, None, extra_config, true)).await?);
    let outcome = AssertUnwindSafe(Box::pin(test(&fixture)))
        .catch_unwind()
        .await;
    {
        // Writable again, so the temporary directory can be removed with the fixture.
        use std::os::unix::fs::PermissionsExt as _;
        let secrets = fixture.storage.path().join("secrets");
        let _ = std::fs::set_permissions(secrets, std::fs::Permissions::from_mode(0o700));
    }
    let cleanup = fixture.shutdown().await;
    match outcome {
        // The test's own error first: a clean-up that fails after it must not hide it.
        Ok(result) => result.and(cleanup),
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

/// The database tests (`#[sqlx::test]`, ignored by default) need both variables set to one database URL:
/// `DATABASE_URL` for the `sqlx` test harness, which makes each test's own database, and `TQA_DATABASE_URL` for the
/// gateway. Build them with `SQLX_OFFLINE=true` (the committed query cache); otherwise the `sqlx` macros check every
/// query against that URL's empty database at compile time and fail. For example:
/// `SQLX_OFFLINE=true DATABASE_URL=$URL TQA_DATABASE_URL=$URL cargo nextest run -p tqa-gateway --run-ignored all`.
pub fn require_database() -> TestResult {
    let value = std::env::var("TQA_DATABASE_URL")
        .map_err(|_| "ignored database tests require TQA_DATABASE_URL and DATABASE_URL")?;
    if value.is_empty() || std::env::var("DATABASE_URL").ok().as_deref() != Some(value.as_str()) {
        return Err(
            "set DATABASE_URL to the same value as TQA_DATABASE_URL for the SQLx harness".into(),
        );
    }
    Ok(())
}

impl Fixture {
    /// The gateway's configuration.
    pub const fn config(&self) -> &GatewayConfig {
        &self.config
    }

    async fn new(
        pool: PgPool,
        registry: Option<tqa_llm::Registry>,
        extra_config: &str,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Self::new_with(pool, registry, extra_config, false).await
    }

    /// With `read_only_secrets`, the install key is made first in its own directory, which is then read-only, as a
    /// deployment's secret mounts are (GW-I-179).
    async fn new_with(
        pool: PgPool,
        registry: Option<tqa_llm::Registry>,
        extra_config: &str,
        read_only_secrets: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let storage = tempfile::tempdir()?;
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let origin = format!("http://{}", listener.local_addr()?);
        // A `[firecracker]` section selects that backend, and the recording fake validates specs as it would.
        // TQA_GATEWAY_TEST_BACKEND=firecracker adds one to every fixture, so whole suites rerun against the microVM
        // configuration (Gate 8's SEC reruns); off by default.
        let forced = forced_firecracker(extra_config);
        let extra_config = forced.as_deref().unwrap_or(extra_config);
        let firecracker = extra_config
            .lines()
            .any(|line| line.trim() == "[firecracker]");
        let backend = if firecracker { "firecracker" } else { "docker" };
        let source = format!(
            "listen='127.0.0.1:0'\npublic_origin='{origin}'\nmodels_path='models.toml'\nstream_keepalive=1\n[runtime]\nbackend='{backend}'\nimage='example/runtime@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'\n[storage]\nroot='{}'",
            storage.path().join("storage").display()
        );
        let config = GatewayConfig::from_toml(&format!("{source}\n{extra_config}"))?;
        let bootstrap = format!(
            "{}{}",
            uuid::Uuid::now_v7().simple(),
            uuid::Uuid::now_v7().simple()
        );
        let password = format!("synthetic-passphrase-{}", uuid::Uuid::now_v7().simple());
        let secrets = GatewaySecrets::new(
            SecretString::from(std::env::var("TQA_DATABASE_URL")?),
            Some(bootstrap.clone().into()),
        )?
        .with_secret_files(tqa_gateway::SecretFiles::new(
            if read_only_secrets {
                read_only_install_key(storage.path())?
            } else {
                storage.path().join("install.key")
            },
            None,
        )?);
        let registry = match registry {
            Some(registry) => registry,
            None => tqa_llm::Registry::from_toml(
                "[[routes]]\nname='local'\nprotocol='openai_chat'\nbase_url='https://models.example/v1'\nmodel='fixture'\nauth={kind='none'}",
            )?,
        };
        let kind = if firecracker {
            tqa_sandbox::BackendKind::Firecracker
        } else {
            tqa_sandbox::BackendKind::Docker
        };
        let backend = std::sync::Arc::new(backend::Backend::of_kind(kind));
        let clock = std::sync::Arc::new(clock::Clock::new());
        let gateway = Gateway::from_pool_with_dependencies(
            config.clone(),
            secrets.clone(),
            pool.clone(),
            registry.clone(),
            tqa_gateway::SandboxDependencies {
                backend: backend.clone(),
                idle_clock: clock.clone(),
            },
        )
        .await?;
        let running = gateway.clone();
        let server = tokio::spawn(async move { running.serve(listener).await });
        let client = reqwest::Client::builder().timeout(DEADLINE).build()?;
        Ok(Self {
            client,
            stream_client: stream_client()?,
            pool,
            gateway,
            backend,
            clock,
            config,
            base_config: source,
            secrets,
            registry,
            storage,
            credentials: std::sync::Mutex::new(std::collections::HashMap::new()),
            origin,
            password,
            bootstrap,
            server: Some(server),
            contract: std::sync::Mutex::new(Vec::new()),
        })
    }

    async fn shutdown(mut self) -> TestResult {
        self.gateway.cancel();
        self.gateway.shutdown().await?;
        self.stopped().await?;
        // Every team response the test made matches openapi.json, or the test fails naming each that didn't (GW-U-142).
        let mut found = self
            .contract
            .lock()
            .map_err(|_| "contract lock")?
            .split_off(0);
        found.extend(self.gateway.contract_violations());
        if found.is_empty() {
            return Ok(());
        }
        Err(format!(
            "team responses that don't match openapi.json:\n{}",
            found.join("\n")
        )
        .into())
    }

    /// Waits for the running server to end, if one is running.
    async fn stopped(&mut self) -> TestResult {
        if let Some(server) = self.server.take() {
            tokio::time::timeout(DEADLINE, server).await???;
        }
        Ok(())
    }

    /// Replaces the models file the next `restart` seeds from; the install key (and so the seed digest) is kept.
    pub fn reseed(&mut self, registry: tqa_llm::Registry) {
        self.registry = registry;
    }

    /// Starts (without serving) a second gateway on this install and database, as a restart would, and returns how
    /// the start ended; a gateway that did start is shut down again. The running gateway is untouched.
    pub async fn start_again(&self) -> Result<(), tqa_gateway::GatewayError> {
        let gateway = Gateway::from_pool_with_backend(
            self.config.clone(),
            self.secrets.clone(),
            self.pool.clone(),
            self.registry.clone(),
            self.backend.clone(),
        )
        .await?;
        gateway.cancel();
        gateway.shutdown().await
    }

    /// Writes `content` as the secrets seed file (private, 0600) that the next `restart` imports; the install key is kept.
    pub fn seed_secrets(&mut self, content: &str) -> TestResult {
        use std::os::unix::fs::PermissionsExt;
        let seed = self.storage.path().join("secrets-seed.toml");
        std::fs::write(&seed, content)?;
        std::fs::set_permissions(&seed, std::fs::Permissions::from_mode(0o600))?;
        self.secrets = GatewaySecrets::new(
            SecretString::from(std::env::var("TQA_DATABASE_URL")?),
            Some(self.bootstrap.clone().into()),
        )?
        .with_secret_files(tqa_gateway::SecretFiles::new(
            self.storage.path().join("install.key"),
            Some(seed),
        )?);
        Ok(())
    }

    /// Points `[runtime] image` at `image` from the next `restart` on, as an upgrade that rebuilt the runtime does. The
    /// next start uses the base configuration only (no extra configuration).
    pub fn set_runtime_image(&mut self, image: &str) -> TestResult {
        let at = self
            .base_config
            .find("[runtime]")
            .ok_or("no [runtime] section")?;
        let line = self.base_config[at..]
            .lines()
            .find(|line| line.starts_with("image="))
            .ok_or("no image")?
            .to_owned();
        self.base_config = self
            .base_config
            .replacen(&line, &format!("image='{image}'"), 1);
        self.config = GatewayConfig::from_toml(&self.base_config)?;
        Ok(())
    }

    /// Replaces the extra gateway configuration the next `restart` loads.
    pub fn reconfigure(&mut self, extra_config: &str) -> TestResult {
        self.config = GatewayConfig::from_toml(&format!("{}\n{extra_config}", self.base_config))?;
        Ok(())
    }

    pub async fn restart(&mut self) -> TestResult {
        let options = self.pool.connect_options();
        self.gateway.shutdown().await?;
        let replaced = self.gateway.contract_violations();
        self.contract
            .lock()
            .map_err(|_| "contract lock")?
            .extend(replaced);
        self.stopped().await?;
        self.pool = tokio::time::timeout(
            DEADLINE,
            sqlx::postgres::PgPoolOptions::new()
                .max_connections(16)
                .connect_with((*options).clone()),
        )
        .await??;
        self.gateway = Gateway::from_pool_with_backend(
            self.config.clone(),
            self.secrets.clone(),
            self.pool.clone(),
            self.registry.clone(),
            self.backend.clone(),
        )
        .await?;
        let address = url::Url::parse(&self.origin)?
            .socket_addrs(|| None)?
            .into_iter()
            .next()
            .ok_or("fixture address")?;
        let listener = tokio::net::TcpListener::bind(address).await?;
        let running = self.gateway.clone();
        self.server = Some(tokio::spawn(async move { running.serve(listener).await }));
        Ok(())
    }

    /// The configured `[storage] root`, as the gateway resolves it at startup.
    pub fn config_storage_root(&self) -> std::path::PathBuf {
        let root = self.storage.path().join("storage");
        root.canonicalize().unwrap_or(root)
    }
    pub fn origin(&self) -> &str {
        &self.origin
    }
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.origin)
    }
    pub fn password(&self) -> &str {
        &self.password
    }
    pub fn bootstrap(&self) -> &str {
        &self.bootstrap
    }

    pub async fn post(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<reqwest::Response, reqwest::Error> {
        self.client
            .post(self.url(path))
            .header("origin", self.origin())
            .json(body)
            .send()
            .await
    }
    pub async fn get(&self, path: &str, cookie: &str) -> Result<reqwest::Response, reqwest::Error> {
        self.client
            .get(self.url(path))
            .header("cookie", cookie)
            .send()
            .await
    }
    pub async fn action(
        &self,
        path: &str,
        cookie: &str,
        key: Option<&str>,
        body: &serde_json::Value,
    ) -> Result<reqwest::Response, reqwest::Error> {
        let request = self
            .client
            .post(self.url(path))
            .header("origin", self.origin())
            .header("cookie", cookie)
            .json(body);
        let request = if let Some(key) = key {
            request.header("idempotency-key", key)
        } else {
            request
        };
        request.send().await
    }
    pub async fn conversation(
        &self,
        owner: uuid::Uuid,
    ) -> Result<Conversation, tqa_gateway::GatewayError> {
        self.gateway
            .create_for_user(owner, "Fixture", "local")
            .await
    }
    /// Waits (bounded) until the conversation has no sandbox instance record. A retirement (an environment switch,
    /// for one) runs in the background, and admission refuses a new start until it has released the record.
    pub async fn until_released(&self, id: &ConversationId) -> TestResult {
        tokio::time::timeout(DEADLINE, self.wait_for_release(id)).await?
    }

    async fn wait_for_release(&self, id: &ConversationId) -> TestResult {
        let uuid = uuid::Uuid::parse_str(id.as_str())?;
        while sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM sandbox_instances WHERE conversation_id=$1",
        )
        .bind(uuid)
        .fetch_one(&self.pool)
        .await?
            > 0
        {
            tokio::task::yield_now().await;
        }
        Ok(())
    }

    pub async fn until_sequence(&self, id: &ConversationId, sequence: u64) -> TestResult {
        tokio::time::timeout(DEADLINE, self.wait_for_sequence(id, sequence)).await?
    }

    async fn wait_for_sequence(&self, id: &ConversationId, sequence: u64) -> TestResult {
        while self
            .gateway
            .events()
            .read(id, (sequence - 1).into(), 1)
            .await?
            .is_empty()
        {
            tokio::task::yield_now().await;
        }
        Ok(())
    }
}

impl Fixture {
    /// Starts the egress policy proxy on its own loopback listener with an injected resolver.
    pub async fn egress(
        &self,
        resolver: std::sync::Arc<dyn tqa_gateway::EgressResolver>,
    ) -> Result<
        (
            std::net::SocketAddr,
            tokio::task::JoinHandle<Result<(), tqa_gateway::GatewayError>>,
        ),
        Box<dyn std::error::Error>,
    > {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let gateway = self.gateway.clone();
        Ok((
            address,
            tokio::spawn(async move { gateway.serve_egress(listener, resolver).await }),
        ))
    }
}

/// A cookie request with the gateway's origin, then an optional JSON body and an optional Idempotency-Key.
pub async fn call(
    f: &Fixture,
    method: reqwest::Method,
    path: &str,
    cookie: &str,
    (body, key): (Option<serde_json::Value>, Option<&str>),
) -> Result<(u16, serde_json::Value), Box<dyn std::error::Error>> {
    let mut request = f
        .client
        .request(method, f.url(path))
        .header("origin", f.origin())
        .header("cookie", cookie);
    if let Some(body) = body {
        request = request.json(&body);
    }
    if let Some(key) = key {
        request = request.header("idempotency-key", key);
    }
    let response = request.send().await?;
    let status = response.status().as_u16();
    let text = response.text().await?;
    Ok((
        status,
        if text.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_str(&text)?
        },
    ))
}

pub fn stream_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .connect_timeout(DEADLINE)
        .read_timeout(DEADLINE)
        .build()
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.gateway.cancel();
        if let Some(server) = &self.server {
            server.abort();
        }
    }
}

pub fn frame(sequence: u64, text: &str) -> ServerFrame {
    ServerFrame::Event {
        seq: sequence.into(),
        run_id: None,
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event: Event::TextDelta { text: text.into() },
    }
}

/// GW-I-148: an answer from the relays or the MCP proxy names the gateway's own refusal in `X-TQA-Gateway-Error`, equal
/// to its `error.code`, and any other answer carries no such header. Callers use it where no upstream error body is
/// forwarded, so an `error.code` there is always the gateway's own.
///
/// # Errors
/// When the header and the body's code disagree.
pub fn gateway_refusal(
    headers: &reqwest::header::HeaderMap,
    body: &serde_json::Value,
) -> Result<(), String> {
    let header = headers
        .get("x-tqa-gateway-error")
        .map(|value| value.to_str().unwrap_or("(not text)"));
    let code = body["error"]["code"].as_str();
    if header == code {
        Ok(())
    } else {
        Err(format!(
            "X-TQA-Gateway-Error is {header:?} but error.code is {code:?}: {body}"
        ))
    }
}

/// `run.started` as the runtime sends it, the run on the frame and in the event; a relayed call's run is recorded only
/// when its task holds it (GW-I-151). Store it with `gateway.events().append`, as the runtime's frames are.
///
/// # Errors
/// When `run` isn't a valid run id.
pub fn started(sequence: u64, run: &str) -> Result<ServerFrame, tqa_types::InvalidId> {
    Ok(ServerFrame::Event {
        seq: sequence.into(),
        run_id: Some(run.parse()?),
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event: Event::RunStarted {
            run_id: run.parse()?,
        },
    })
}
