//! Process-owned services and cancellation; the gateway never links the agent core.

#[cfg(feature = "testing")]
pub(crate) mod contract_check;
mod endpoints;
mod long_reads;
mod router;

use crate::secrets::reads::ReadAudit;
use crate::{
    GatewayConfig, GatewayError, GatewaySecrets,
    auth::{LoginRate, Passwords, TokenFailures},
    events::{EventStore, StoreResources},
    link::LinkRegistry,
    storage::Storage,
    telemetry::Telemetry,
};
use axum::Router;
use sqlx::PgPool;
use std::sync::Arc;
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tqa_llm::Registry;

pub(crate) struct State {
    pub config: GatewayConfig,
    pub secrets: GatewaySecrets,
    pub pool: PgPool,
    /// Routes today, then egress rules and limits (ADR-0028); read one snapshot per operation.
    pub settings: Arc<crate::settings::Settings>,
    pub passwords: Passwords,
    pub dummy_hash: String,
    pub login_rate: LoginRate,
    /// Failed personal-token authentications per address (task 063).
    pub token_failures: TokenFailures,
    pub storage: Storage,
    pub events: EventStore,
    pub links: Arc<LinkRegistry>,
    pub runtime_tokens: crate::RuntimeTokens,
    pub manager: Arc<crate::SandboxManager>,
    pub secret_store: Arc<dyn crate::SecretStore>,
    /// The install CA for host keys, beside the install key (ADR-0037 §6); none without secret files.
    pub host_ca: Option<Arc<crate::host_ca::HostCa>>,
    /// Which keyed-call notices went out, for their rate limits (ADR-0037, Amendment 1).
    pub host_keys: crate::host_keys::Live,
    /// Tests only: keyed hosts that closed their connection (GW-I-180).
    #[cfg(feature = "testing")]
    pub keyed_closes: crate::egress::KeyedCloses,
    #[cfg(feature = "testing")]
    pub contract: contract_check::Check,
    /// The roots the proxy verifies a keyed host's certificate against: the public ones (ADR-0037 §4), never the
    /// install CA. Tests add their own with [`Gateway::trust_upstream_root`].
    pub upstream_roots: std::sync::RwLock<Arc<rustls::RootCertStore>>,
    /// What internal egress rules may not reach (ADR-0035 amendment), renewed by the manager.
    pub internal: Arc<crate::egress::internal::Guard>,
    /// When each secret due for rotation was last named in the log (vault slice 1).
    pub reminders: crate::secrets::rotation::Reminders,
    /// The queue of secret read records to their writer (vault slice 2).
    pub read_audit: crate::secrets::reads::ReadAudit,
    pub relay: crate::relay::Relay,
    /// The MCP proxy's resolver for connector hosts (ADR-0031).
    pub mcp: crate::mcp_proxy::McpProxy,
    pub web: crate::web::Web,
    /// Wakes channel readers on a new message (ADR-0014).
    pub channels: crate::channels::Hub,
    /// Each agent team's version and its `team.changed` frames (ADR-0034, S6).
    pub team_changes: crate::teams::changed::Changes,
    pub telemetry: Arc<Telemetry>,
    /// Where the engine's environment images are read from (the built-in environments design).
    pub environment_images: crate::environments::Images,
    /// Wakes the background purge of deleted tasks and workspaces (delete means gone).
    pub purge: crate::purge::Purge,
    /// Wakes the agent teams' reconciler when a stop is left to it (the lead's `done`, the budget).
    pub reconciler: crate::teams::reconciler::Nudge,
    pub cancel: CancellationToken,
    pub tasks: TaskTracker,
}

/// Owns gateway services, database connections and background upgrade/password tasks.
#[derive(Clone)]
pub struct Gateway {
    state: Arc<State>,
}

/// Injectable sandbox boundaries; transport deadlines remain independent of the idle clock.
pub struct SandboxDependencies {
    /// The sandbox lifecycle implementation used by the manager.
    pub backend: Arc<dyn tqa_sandbox::SandboxBackend>,
    /// Monotonic time and wakeups for the manager's idle policy.
    pub idle_clock: Arc<dyn crate::IdleClock>,
}

struct Startup {
    config: GatewayConfig,
    secrets: GatewaySecrets,
    pool: PgPool,
    registry: Registry,
    cancel: CancellationToken,
}

impl Gateway {
    /// Reads the bounded registry, opens `PostgreSQL` and applies migrations before creating the router.
    ///
    /// # Errors
    /// Fails closed on invalid configuration, unavailable dependencies or cancelled startup.
    pub async fn connect(
        config: GatewayConfig,
        secrets: GatewaySecrets,
        cancel: CancellationToken,
    ) -> Result<Self, GatewayError> {
        let bytes = step(
            "models file",
            crate::io::bounded(
                config.io_timeout(),
                &cancel,
                crate::io::read_bounded(&config.models_path, tqa_llm::MAX_REGISTRY_BYTES),
            ),
        )
        .await?;
        let source = std::str::from_utf8(&bytes)
            .map_err(|_| GatewayError::Configuration("model registry is not UTF-8"))?;
        let registry = Registry::from_toml(source)
            .map_err(|_| GatewayError::Configuration("model registry is invalid"))?;
        // The connection and the migrations name themselves (db::connect).
        let pool = Box::pin(crate::db::connect(&config, &secrets, &cancel)).await?;
        // Boxed: the startup state carries the whole configuration, so the future is large.
        Box::pin(Self::build(
            Startup {
                config,
                secrets,
                pool,
                registry,
                cancel,
            },
            None,
            Arc::new(crate::manager::SystemClock::new()),
        ))
        .await
    }

    /// Uses an externally provided `PostgreSQL` pool and validated registry, applying the same migrations.
    ///
    /// # Errors
    /// Returns the same bounded startup errors as [`Self::connect`].
    pub async fn from_pool(
        config: GatewayConfig,
        secrets: GatewaySecrets,
        pool: PgPool,
        registry: Registry,
    ) -> Result<Self, GatewayError> {
        let cancel = CancellationToken::new();
        crate::db::migrate(&config, &pool, &cancel).await?;
        Box::pin(Self::build(
            Startup {
                config,
                secrets,
                pool,
                registry,
                cancel,
            },
            None,
            Arc::new(crate::manager::SystemClock::new()),
        ))
        .await
    }

    /// Builds with an explicitly supplied backend for controlled embeddings and lifecycle tests.
    ///
    /// # Errors
    /// Returns bounded startup, migration or inventory reconciliation failures.
    pub async fn from_pool_with_backend(
        config: GatewayConfig,
        secrets: GatewaySecrets,
        pool: PgPool,
        registry: Registry,
        backend: Arc<dyn tqa_sandbox::SandboxBackend>,
    ) -> Result<Self, GatewayError> {
        Self::from_pool_with_dependencies(
            config,
            secrets,
            pool,
            registry,
            SandboxDependencies {
                backend,
                idle_clock: Arc::new(crate::manager::SystemClock::new()),
            },
        )
        .await
    }

    /// Builds with an explicit monotonic idle-policy clock; transport deadlines retain real time.
    ///
    /// # Errors
    /// Returns the same startup errors as [`Self::from_pool_with_backend`].
    pub async fn from_pool_with_dependencies(
        config: GatewayConfig,
        secrets: GatewaySecrets,
        pool: PgPool,
        registry: Registry,
        dependencies: SandboxDependencies,
    ) -> Result<Self, GatewayError> {
        let cancel = CancellationToken::new();
        crate::db::migrate(&config, &pool, &cancel).await?;
        Box::pin(Self::build(
            Startup {
                config,
                secrets,
                pool,
                registry,
                cancel,
            },
            Some(dependencies.backend),
            dependencies.idle_clock,
        ))
        .await
    }

    /// The runtime link registry and the runtime-token service that binds sandboxes to it.
    fn runtime_links_for(
        config: &GatewayConfig,
        pool: &PgPool,
        cancel: &CancellationToken,
    ) -> (Arc<LinkRegistry>, crate::RuntimeTokens) {
        let links = Arc::new(
            LinkRegistry::new(
                config.limits.live_conversations,
                config.limits.action_capacity,
                cancel.clone(),
            )
            .with_workspace_requests(config.files.link_requests),
        );
        let tokens = crate::RuntimeTokens {
            pool: pool.clone(),
            links: links.clone(),
            cancel: cancel.clone(),
            timeout: config.io_timeout(),
            reservation_secs: config.limits.token_reservation_secs,
        };
        (links, tokens)
    }

    /// The parts built from the configuration alone: the model relay, the web front and the telemetry.
    fn from_config(
        config: &GatewayConfig,
    ) -> Result<(crate::relay::Relay, crate::web::Web, Arc<Telemetry>), GatewayError> {
        Ok((
            crate::relay::Relay::new(&config.relay)?,
            crate::web::Web::new(config.web.as_ref())?,
            Arc::new(Telemetry::new()?),
        ))
    }

    /// The event store; with agent teams on, it starts with the reads still outstanding before a restart, a named
    /// start-up step.
    async fn events_for(
        config: &GatewayConfig,
        pool: &PgPool,
        storage: &Storage,
        (cancel, telemetry): (&CancellationToken, &Arc<Telemetry>),
    ) -> Result<EventStore, GatewayError> {
        let events = EventStore::new(
            config,
            StoreResources {
                pool: pool.clone(),
                storage: storage.clone(),
                cancel: cancel.clone(),
                telemetry: telemetry.clone(),
            },
        );
        if let Some(reads) = events.team_reads() {
            step("agent team reads", reads.load(pool)).await?;
        }
        Ok(events)
    }

    /// The live settings, a named start-up step.
    async fn settings_for(
        config: &GatewayConfig,
        secrets: &GatewaySecrets,
        pool: &PgPool,
        cancel: &CancellationToken,
        registry: Registry,
    ) -> Result<Arc<crate::settings::Settings>, GatewayError> {
        step(
            "settings",
            Box::pin(crate::settings::Settings::load(
                config, secrets, pool, cancel, registry,
            )),
        )
        .await
    }

    /// The storage root, the dummy password hash and the secret store, each a named start-up step.
    async fn stores(
        config: &GatewayConfig,
        secrets: &GatewaySecrets,
        pool: &PgPool,
        (passwords, cancel): (&Passwords, &CancellationToken),
    ) -> Result<
        (
            Storage,
            String,
            Arc<dyn crate::SecretStore>,
            Option<Arc<crate::host_ca::HostCa>>,
        ),
        GatewayError,
    > {
        let storage = step("storage", async {
            Storage::prepare(config, cancel.clone())
                .await?
                .with_skills(config, secrets.secret_files.as_ref())
                .await
        })
        .await?;
        let dummy_hash = step(
            "password hashing",
            passwords.hash_at_start(crate::auth::token::mint()?),
        )
        .await?;
        // Boxed: opening the store (and its seed) is a large future.
        let secret_store = step(
            "secret store",
            Box::pin(crate::secrets::open(pool, config, secrets, cancel)),
        )
        .await?;
        // Git credentials become host keys, once each (ADR-0037 §7); a failure leaves the rest for the next start.
        if let Err(error) =
            crate::host_keys::copy_git_credentials(pool, secret_store.as_ref()).await
        {
            tracing::warn!(
                code = error.code(),
                "Git credentials weren't all copied into host keys"
            );
        }
        // With the secret store (keys need it), in its own private directory: never beside the install key, which a
        // deployment mounts read-only (GW-I-179).
        let host_ca = match secrets.secret_files {
            Some(_) => Some(Arc::new(
                step(
                    "host keys CA",
                    crate::host_ca::HostCa::load(
                        &config.host_ca_directory,
                        std::time::Duration::from_secs(config.sandbox.max_lifetime_seconds.into()),
                        time::OffsetDateTime::now_utc(),
                    ),
                )
                .await?,
            )),
            None => None,
        };
        // The install CA signs leaves for sandboxes; it must never verify a keyed host upstream (ADR-0037 §4, T7).
        if let (Some(host_ca), Some(pem)) = (&host_ca, &config.egress.ca_pem)
            && host_ca.among(pem)
        {
            return Err(GatewayError::Configuration(
                "egress.ca_file holds the install CA's certificate, which can never verify a keyed host",
            ));
        }
        Ok((storage, dummy_hash, secret_store, host_ca))
    }

    /// Builds the gateway from its started dependencies, each named start-up step naming its own timeout (GW-U-060).
    /// The `State` fields built from `config` come before `config` in its literal: fields are evaluated in order.
    #[allow(
        clippy::too_many_lines,
        reason = "one State literal, a field per service; splitting it only threads them through helpers"
    )]
    async fn build(
        startup: Startup,
        backend: Option<Arc<dyn tqa_sandbox::SandboxBackend>>,
        idle_clock: Arc<dyn crate::IdleClock>,
    ) -> Result<Self, GatewayError> {
        let Startup {
            config,
            secrets,
            pool,
            registry,
            cancel,
        } = startup;
        let tasks = TaskTracker::new();
        let (relay, web, telemetry) = Self::from_config(&config)?;
        let (upstream_roots, internal) = egress_trust(&config);
        let passwords = Passwords::new(&config, tasks.clone(), cancel.clone());
        let (storage, dummy_hash, secret_store, host_ca) = Box::pin(Self::stores(
            &config,
            &secrets,
            &pool,
            (&passwords, &cancel),
        ))
        .await?;
        let events = Self::events_for(&config, &pool, &storage, (&cancel, &telemetry)).await?;
        let (links, runtime_tokens) = Self::runtime_links_for(&config, &pool, &cancel);
        let settings = Box::pin(Self::settings_for(
            &config, &secrets, &pool, &cancel, registry,
        ))
        .await?;
        let (manager, own_backend) = Box::pin(build_manager(
            &config,
            backend,
            crate::manager::Services {
                internal: internal.clone(),
                settings: settings.clone(),
                telemetry: telemetry.clone(),
                pool: pool.clone(),
                tokens: runtime_tokens.clone(),
                links: links.clone(),
                storage: storage.clone(),
                events: events.clone(),
                cancel: cancel.clone(),
                tasks: tasks.clone(),
                idle_clock,
            },
        ))
        .await?;
        let state = Arc::new(State {
            login_rate: LoginRate::new(config.limits.login_addresses),
            token_failures: TokenFailures::new(config.limits.login_addresses),
            read_audit: ReadAudit::new(config.secret_store.read_audit_queue),
            config,
            secrets,
            pool,
            settings,
            passwords,
            dummy_hash,
            storage,
            events,
            links,
            runtime_tokens,
            manager,
            secret_store,
            host_ca,
            host_keys: crate::host_keys::Live::default(),
            #[cfg(feature = "testing")]
            keyed_closes: crate::egress::KeyedCloses::default(),
            #[cfg(feature = "testing")]
            contract: contract_check::Check::default(),
            upstream_roots,
            internal,
            reminders: crate::secrets::rotation::Reminders::default(),
            relay,
            mcp: crate::mcp_proxy::McpProxy::default(),
            web,
            channels: crate::channels::Hub::default(),
            team_changes: crate::teams::changed::Changes::default(),
            telemetry,
            environment_images: crate::environments::Images::default(),
            purge: crate::purge::Purge::default(),
            reconciler: crate::teams::reconciler::Nudge::default(),
            cancel,
            tasks,
        });
        let gateway = Self { state };
        // Before the gateway listens: a page that reconnects after a restart finds the hand-back already recorded.
        crate::screen::hand_back_at_start(&gateway.state).await;
        start_background(&gateway, own_backend);
        Ok(gateway)
    }

    /// Starts the template checker (ADR-0032) with the engine side `check`: every template version gets checked once,
    /// in the background; it never delays start-up or readiness.
    pub fn start_template_checks(&self, check: Arc<dyn crate::TemplateCheck>) {
        self.state
            .tasks
            .spawn(crate::environments::checks::run(self.state.clone(), check));
    }

    /// Reads the engine's environment images from `source` (the built-in environments design): the built-ins registered
    /// from them and the administrators' image list. The gateway uses its own engine when it connected its own backend;
    /// embedders and tests pass theirs.
    pub fn use_environment_images(&self, source: Arc<dyn crate::EnvironmentImageSource>) {
        self.state.environment_images.set(source);
    }

    /// Registers the built-in environments from the engine's images now, as the gateway does at start. An engine that
    /// doesn't answer changes nothing.
    ///
    /// # Errors
    /// Storage failure or a deadline while writing.
    pub async fn built_in_environments_now(&self) -> Result<(), GatewayError> {
        crate::environments::register_built_ins(&self.state).await
    }

    /// Registers the built-in environments in the background, again until the engine answers, as the gateway's start
    /// does (GW-I-213): owned by the gateway's tasks and stopped by its shutdown.
    pub fn keep_registering_built_ins(&self) {
        let state = self.state.clone();
        self.state
            .tasks
            .spawn(async move { crate::environments::keep_registering(&state).await });
    }

    /// Builds all documented HTTP and runtime-link routes with their shared boundaries.
    #[must_use = "the router must be served to accept requests"]
    pub fn router(&self) -> Router {
        router::build(self.state.clone())
    }

    /// Builds only the operational metrics endpoint for a separate listener; disabled settings expose none.
    #[must_use = "the metrics router must be served on its separate listener"]
    pub fn metrics_router(&self) -> Router {
        let router = Router::new();
        let router = if self.state.config.metrics_listen.is_some() {
            router.route("/metrics", axum::routing::get(endpoints::metrics))
        } else {
            router
        };
        router
            .layer(axum::middleware::map_response(router::security_headers))
            .with_state(self.state.clone())
    }

    /// Serves an already-bound listener until cancellation; peer addresses come from the transport.
    ///
    /// # Errors
    /// Returns a redacted I/O failure if the HTTP server stops unexpectedly.
    pub async fn serve(&self, listener: tokio::net::TcpListener) -> Result<(), GatewayError> {
        // TCP_NODELAY on every connection (`net`): without it the end of every other relayed model stream waited
        // out the runtime's delayed ACK (GW-I-181).
        axum::serve(
            crate::net::listener(listener),
            self.router()
                .into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(self.state.cancel.clone().cancelled_owned())
        .await
        .map_err(GatewayError::from)
    }

    /// Serves the egress policy proxy on an already-bound listener until cancellation. Bind it only on the sandbox
    /// network: it must never be published outside it (ADR-0021).
    ///
    /// # Errors
    /// Returns a redacted I/O failure if accepting connections fails.
    pub async fn serve_egress(
        &self,
        listener: tokio::net::TcpListener,
        resolver: Arc<dyn crate::EgressResolver>,
    ) -> Result<(), GatewayError> {
        crate::egress::serve(self.state.clone(), listener, resolver).await
    }

    /// Cancels owned requests, streams and runtime links. Call [`Self::shutdown`] to join their work.
    pub fn cancel(&self) {
        self.state.cancel.cancel();
    }

    /// Cancels and joins owned background work and closes the pool within the configured I/O deadline.
    ///
    /// # Errors
    /// Returns `Timeout` if blocking password work or outstanding database work misses the deadline.
    pub async fn shutdown(&self) -> Result<(), GatewayError> {
        self.cancel();
        self.state.tasks.close();
        tokio::time::timeout(self.state.config.io_timeout(), async {
            self.state.tasks.wait().await;
            self.state.pool.close().await;
        })
        .await
        .map_err(|_| GatewayError::Timeout)
    }

    /// Borrows the trusted internal journal service; HTTP callers must pass ownership checks first.
    #[must_use]
    pub fn events(&self) -> &EventStore {
        &self.state.events
    }

    /// Provides sandbox backends with the live runtime registration lookup.
    #[must_use]
    pub fn readiness(&self) -> Arc<dyn tqa_sandbox::ReadinessSource> {
        self.state.links.clone()
    }

    /// Creates an owned conversation without allocating sandbox credentials.
    ///
    /// # Errors
    /// Rejects unknown model routes, invalid metadata and unavailable storage.
    pub async fn create_for_user(
        &self,
        owner: uuid::Uuid,
        title: &str,
        route: &str,
    ) -> Result<crate::Conversation, GatewayError> {
        crate::conversations::create_owned(&self.state, owner, title, route).await
    }

    /// Borrows the internal credential service used by the sandbox manager.
    #[must_use]
    pub fn runtime_tokens(&self) -> &crate::RuntimeTokens {
        &self.state.runtime_tokens
    }

    /// Borrows the internal sandbox lifecycle owner; it is not an HTTP administration API.
    #[must_use]
    pub fn sandboxes(&self) -> &Arc<crate::SandboxManager> {
        &self.state.manager
    }

    /// Runs one workspace retention sweep now (it also runs hourly while retention is on); returns how many
    /// workspaces it deleted. For operations and tests; there is no HTTP endpoint for it.
    ///
    /// # Errors
    /// A store or storage failure.
    pub async fn workspace_retention_now(&self) -> Result<u64, GatewayError> {
        crate::workspaces::retention(&self.state).await
    }

    /// Checks secret rotation reminders now (it also runs at start and hourly): sets the gauge and logs each newly
    /// due secret; returns how many are due. For operations and tests; there is no HTTP endpoint for it.
    ///
    /// # Errors
    /// A store failure.
    pub async fn rotation_reminders_now(&self) -> Result<u64, GatewayError> {
        crate::secrets::rotation::check(&self.state).await
    }

    /// Waits until every secret read recorded so far is written (vault slice 2). For tests; there is no endpoint.
    ///
    /// # Errors
    /// The writer has stopped.
    pub async fn read_audit_flush_now(&self) -> Result<(), GatewayError> {
        self.state.read_audit.flush().await
    }

    /// Holds the secret read writer (`true`) or lets it run, so a test can fill its queue. There is no endpoint.
    pub fn read_audit_hold(&self, held: bool) {
        self.state.read_audit.hold(held);
    }

    /// Runs one audit retention sweep now (it also runs hourly): read records past `read_audit_days` and
    /// administration records past `admin_audit_days`. Returns how many of each it deleted. There is no endpoint.
    ///
    /// # Errors
    /// A store failure.
    pub async fn audit_retention_now(&self) -> Result<(u64, u64), GatewayError> {
        crate::secrets::reads::sweep(&self.state).await
    }

    /// Uses the host key whose value is `reference` as the proxy does (ADR-0037 §8), for `host:port` on behalf of
    /// `(conversation, user, run)`: its row is committed before the value is read. Whether a value came back, or the
    /// outcome that refused it; never the value. For tests; the proxy calls the same path.
    ///
    /// # Errors
    /// The refusal's outcome (`unrecorded`, `revoked`, `unavailable`).
    pub async fn host_key_use(
        &self,
        reference: &str,
        (host, port): (&str, u16),
        (conversation, user, run): (uuid::Uuid, uuid::Uuid, &str),
    ) -> Result<(), &'static str> {
        let key_use = crate::host_keys::Use {
            reference,
            host,
            port,
            context: crate::secrets::reads::ReadContext {
                conversation: Some(conversation),
                user: Some(user),
                run: Some(run.to_owned()),
            },
        };
        crate::host_keys::value(&self.state, &key_use)
            .await
            .map(drop)
            .map_err(crate::host_keys::Outcome::as_str)
    }

    /// Records a refused use of that host key, `outcome` naming why (`wrong_host`, `foreign_run`, ...), as the proxy
    /// does. For tests.
    pub async fn host_key_refusal(
        &self,
        reference: &str,
        (host, port): (&str, u16),
        (conversation, user, run): (uuid::Uuid, uuid::Uuid, &str),
        outcome: &str,
    ) {
        let Some(outcome) = crate::host_keys::Outcome::parse(outcome) else {
            return;
        };
        let key_use = crate::host_keys::Use {
            reference,
            host,
            port,
            context: crate::secrets::reads::ReadContext {
                conversation: Some(conversation),
                user: Some(user),
                run: Some(run.to_owned()),
            },
        };
        crate::host_keys::refuse(&self.state, &key_use, outcome).await;
    }

    /// Waits until at least `count` keyed hosts have closed their connection, as the proxy saw it: for tests that must
    /// send after a close, never racing it (GW-I-180). Only with the `testing` feature, never in a release build.
    #[cfg(feature = "testing")]
    pub async fn keyed_upstream_closes(&self, count: usize) {
        self.state.keyed_closes.at_least(count).await;
    }

    /// Sets the gateway's own addresses for the internal-rule inventory and renews it now (ADR-0035 amendment): for
    /// tests, whose host's `/proc` would decide whether the host's interfaces look visible. Only with the `testing`
    /// feature, never in a release build (REL-U-009).
    #[cfg(feature = "testing")]
    pub async fn internal_inventory_now(&self, own: Vec<std::net::IpAddr>) {
        self.state.internal.set_own(own);
        self.sandboxes().renew_internal_inventory().await;
    }

    /// Runs one pass of the agent teams' reconciler now, as its loop does every 15 s (ADR-0034 point 11): for tests,
    /// which backdate stored state rather than wait minutes. Answers how many actions it took. Only with the `testing`
    /// feature, never in a release build (REL-U-009).
    ///
    /// # Errors
    /// Storage failure reading the teams.
    #[cfg(feature = "testing")]
    pub async fn reconcile_agent_teams_now(&self) -> Result<usize, GatewayError> {
        Box::pin(crate::teams::reconciler::pass(&self.state)).await
    }

    /// Holds the agent teams' reconciler loop (`true`) or lets it run, so a test can look at a team still `stopping`
    /// after the stop's nudge; [`Gateway::reconcile_agent_teams_now`] still runs. Only with the `testing` feature.
    #[cfg(feature = "testing")]
    pub fn agent_teams_hold(&self, held: bool) {
        self.state.reconciler.hold(held);
    }

    /// Trusts `certificate` (DER) as an upstream root for keyed hosts, beside the public ones: for tests whose keyed
    /// host is a local server. Only with the `testing` feature, never in a release build (REL-U-009).
    ///
    /// # Errors
    /// A certificate that isn't a usable root.
    #[cfg(feature = "testing")]
    pub fn trust_upstream_root(&self, certificate: &[u8]) -> Result<(), GatewayError> {
        let mut roots = self
            .state
            .upstream_roots
            .write()
            .map_err(|_| GatewayError::StoreUnavailable)?;
        let mut next = rustls::RootCertStore::clone(&roots);
        next.add(rustls::pki_types::CertificateDer::from(
            certificate.to_vec(),
        ))
        .map_err(|_| GatewayError::Configuration("not a usable root certificate"))?;
        *roots = Arc::new(next);
        Ok(())
    }

    /// Tells the task's streams about a keyed call as the proxy does (ADR-0037, Amendment 1): `outcome` `ok` for a
    /// use, else the refusal; `key` the key's id and name, none for `absent`. Rate-limited as the proxy's are. For
    /// tests; the proxy calls the same path.
    pub async fn host_key_notice(
        &self,
        conversation: &tqa_types::ConversationId,
        (run, sender): (&str, uuid::Uuid),
        key: Option<(uuid::Uuid, &str)>,
        (host, port, outcome): (&str, u16, &str),
    ) {
        let Some(outcome) = crate::host_keys::Outcome::parse(outcome) else {
            return;
        };
        let call = crate::host_keys::Call {
            conversation,
            run,
            sender,
            key: key.map(|(id, _)| id),
            name: key.map(|(_, name)| name),
            host,
            port,
        };
        crate::host_keys::notify(&self.state, &call, outcome).await;
    }

    /// Decides now the channel proposals whose window has closed (ADR-0014 point 9), as the periodic sweep does;
    /// how many were decided. For tests.
    ///
    /// # Errors
    /// A store failure.
    pub async fn channel_decisions_now(&self) -> Result<usize, GatewayError> {
        crate::channels::api::sweep(&self.state).await
    }

    /// Holds the next event-stream open at `point` until the returned hold is released, so a test can land a delete
    /// before or after the stream subscribes. There is no endpoint.
    #[must_use]
    pub fn hold_stream_open(
        &self,
        point: crate::events::OpenPoint,
    ) -> crate::events::StreamOpenHold {
        self.state.events.open_gate().hold(point)
    }

    /// Holds the background purge (`true`) or lets it run, so a test can look at a deleted task or workspace before it
    /// goes; [`Gateway::purge_now`] still runs. There is no endpoint.
    pub fn purge_hold(&self, held: bool) {
        self.state.purge.hold(held);
    }

    /// Runs one purge cycle now (it also runs a minute after the last, or when a delete wakes it), on one connection:
    /// what it finished, deferred and failed. There is no endpoint.
    ///
    /// # Errors
    /// The connection could not be had in time.
    pub async fn purge_now(&self) -> Result<crate::purge::Cycle, GatewayError> {
        Box::pin(crate::purge::cycle(&self.state)).await
    }

    /// The credential the MCP proxy would inject for `user`'s call to connector `name` (ADR-0031 decision 3): their
    /// own while it is bound to the connector's current origin, else the shared one, else none. For the proxy and
    /// tests; there is no HTTP endpoint for it.
    ///
    /// # Errors
    /// `404` for an unknown or disabled connector, or a store or secret-store failure.
    pub async fn connector_credential(
        &self,
        user: uuid::Uuid,
        name: &str,
    ) -> Result<Option<crate::ConnectorCredential>, GatewayError> {
        crate::connectors::credentials::resolve(&self.state, user, name, None).await
    }

    /// Replaces the resolver the MCP proxy uses for connector hosts (the system's by default); for tests that script
    /// DNS answers.
    pub fn set_connector_resolver(&self, resolver: Arc<dyn crate::EgressResolver>) {
        self.state.mcp.set_resolver(resolver);
    }

    /// Borrows the trusted internal credential store; there is no HTTP endpoint for its values.
    #[must_use]
    pub fn secret_store(&self) -> &dyn crate::SecretStore {
        self.state.secret_store.as_ref()
    }

    /// Returns the latest supervisor heartbeat without changing run activeness.
    ///
    /// # Errors
    /// Fails closed if the runtime registry is unavailable.
    pub fn last_heartbeat(
        &self,
        conversation: &tqa_types::ConversationId,
    ) -> Result<Option<crate::RuntimeHeartbeat>, GatewayError> {
        self.state.links.heartbeat(conversation)
    }

    /// Runtime workspace replies dropped since start: unknown, finished or another link's request ids, or a flooded
    /// request (ADR-0030). A rising count shows a misbehaving runtime.
    #[must_use]
    pub fn workspace_replies_dropped(&self) -> u64 {
        self.state.links.workspace.dropped()
    }
}

/// Checks templates against the engine the gateway connected (ADR-0032). A bad engine address leaves them unchecked:
/// file-provided templates stay offered, API-created ones wait for a working engine.
fn start_engine_checks(gateway: &Gateway) {
    let rootfs = gateway.state.manager.rootfs_source();
    match crate::environments::EngineCheck::connect(
        crate::environments::ENGINE_READ_TIMEOUT,
        &gateway.state.config.sandbox.network,
        rootfs,
    ) {
        Ok(check) => {
            let check = Arc::new(check);
            check.start_sweep();
            gateway.start_template_checks(check);
        }
        Err(error) => tracing::warn!(
            code = error.code(),
            "template checks are off: the engine address is unusable"
        ),
    }
}

/// Registers the built-in environments from the engine the gateway connected (the built-in environments design), in the
/// background: start-up never waits for the engine.
fn start_built_ins(gateway: &Gateway) {
    match tqa_sandbox::docker::EnvironmentImages::from_defaults(
        crate::environments::ENGINE_READ_TIMEOUT,
    ) {
        Ok(images) => {
            gateway.use_environment_images(Arc::new(images));
            gateway.keep_registering_built_ins();
        }
        Err(error) => tracing::warn!(
            code = error.code(),
            "built-in environments not refreshed: the engine address is unusable"
        ),
    }
}

/// The gateway's background work: the sweeps, and the template checks and built-ins when it connected its own backend.
fn start_background(gateway: &Gateway, own_backend: bool) {
    start_sweeps(&gateway.state);
    if own_backend {
        start_built_ins(gateway);
        start_engine_checks(gateway);
    }
}

/// The periodic sweeps: idempotency retention, and workspace retention when it is on (066c).
/// Runs one named start-up step. A failure is logged at ERROR with the step's name and cause, and a timeout names the
/// step: the gateway exits with this error, and "operation timed out" alone would not say which of its many bounded
/// calls did not answer (GW-U-060, GW-U-071).
async fn step<T>(
    name: &'static str,
    future: impl std::future::Future<Output = Result<T, GatewayError>>,
) -> Result<T, GatewayError> {
    future
        .await
        .map_err(|error| crate::startup::failed(name, error))
}

fn start_sweeps(state: &Arc<State>) {
    crate::actions::start_retention(state);
    crate::telemetry::start_upkeep(state.telemetry.clone(), state.cancel.clone(), &state.tasks);
    state.events.start_hub_sweep(&state.tasks);
    // Agent team members whose first turn a restart or a failed send cut short (ADR-0034, review M1): at start-up,
    // then every few seconds; a turn already sent replays, and one overdue fails with a notice.
    let owned = state.clone();
    state.tasks.spawn(async move {
        let mut every = tokio::time::interval(crate::teams::first_turn::SWEEP_EVERY);
        loop {
            tokio::select! {
                () = owned.cancel.cancelled() => break,
                _ = every.tick() => {}
            }
            if let Err(error) = crate::teams::first_turn::sweep(&owned).await {
                tracing::warn!(code = error.code(), "members' first turns weren't checked");
            }
        }
    });
    // Agent teams' liveness and stopping (ADR-0034 points 9-11): nothing at all while agent teams are off.
    crate::teams::reconciler::start(state);
    crate::workspaces::start_upkeep(state);
    crate::secrets::rotation::start(state);
    crate::secrets::reads::start(state);
    crate::channels::api::start(state);
    crate::purge::start(state);
    crate::teams::changed::start(state);
}

/// The sandbox manager, each of its start-up steps named (GW-U-060); also whether the backend is the gateway's own
/// (not injected by an embedder).
async fn build_manager(
    config: &GatewayConfig,
    backend: Option<Arc<dyn tqa_sandbox::SandboxBackend>>,
    services: crate::manager::Services,
) -> Result<(Arc<crate::SandboxManager>, bool), GatewayError> {
    let own = backend.is_none();
    let backend = match backend {
        Some(backend) => crate::manager::Backend::injected(backend),
        None => step("sandbox backend", crate::manager::Backend::connect(config)).await?,
    };
    // A fork the gateway was copying when it stopped has failed (066c).
    step(
        "fork recovery",
        crate::io::bounded(
            config.io_timeout(),
            &services.cancel,
            crate::workspaces::recover(&services.pool, &services.storage),
        ),
    )
    .await?;
    let manager = crate::SandboxManager::new(config.clone(), backend, services);
    step("sandbox reconciliation", Box::pin(manager.reconcile())).await?;
    manager.start_admission();
    manager.start_network_sweep();
    manager.start_internal_inventory();
    Ok((manager, own))
}

/// What the egress proxy trusts and guards: the roots for keyed hosts, and what internal rules never reach (the
/// configured `internal_never` and the sandbox pool; the manager adds the engine's networks).
fn egress_trust(
    config: &GatewayConfig,
) -> (
    std::sync::RwLock<Arc<rustls::RootCertStore>>,
    Arc<crate::egress::internal::Guard>,
) {
    let pool =
        crate::egress::internal::Network::parse(&config.sandbox.network_pool.cidr().to_string());
    let guard = crate::egress::internal::Guard::new(
        config.egress.internal_never.clone(),
        pool.into_iter().collect(),
    );
    (public_roots(&config.egress), Arc::new(guard))
}

/// The roots a keyed host's certificate is verified against (ADR-0037 §4): the public ones, and `egress.ca_file`'s,
/// which the configuration already checked are all usable authorities.
fn public_roots(
    egress: &crate::config::EgressConfig,
) -> std::sync::RwLock<Arc<rustls::RootCertStore>> {
    use rustls::pki_types::{CertificateDer, pem::PemObject};
    let mut roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    if let Some(pem) = &egress.ca_pem {
        roots.add_parsable_certificates(CertificateDer::pem_slice_iter(pem).filter_map(Result::ok));
    }
    std::sync::RwLock::new(Arc::new(roots))
}
