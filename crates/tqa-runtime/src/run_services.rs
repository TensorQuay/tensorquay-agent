//! Per-run prompt discovery and fixed provider/tool construction.
use crate::{
    IoCause, Journal, ProcessCleanup, RuntimeConfig, RuntimeError, RuntimeLimits, session::Session,
    time::Time,
};
use std::sync::{Arc, Mutex, PoisonError};
use tokio_util::sync::CancellationToken;
use tqa_core::{PromptInputs, RunOutcome, Runner, RunnerDeps, UserTurn};
use tqa_llm::Jitter;
use tqa_tools::{ToolLimits, ToolRegistry, WorkspaceRoot, builtin};
use tqa_types::{
    Capability, ContextLoaded, Environment, LoadedInstruction, LoadedSkill, ProfileSnapshot,
    ServerFrame, SkillTier, SkillsFilter, SkippedSkill, WorkspaceSnapshot,
};

#[derive(Clone)]
pub(crate) struct Services {
    #[cfg(feature = "testing")]
    pub testing: crate::testing::TestingHooks,
    pub config: RuntimeConfig,
    pub limits: RuntimeLimits,
    pub time: Time,
    pub journal: Arc<Journal>,
    pub cleanup: Arc<dyn ProcessCleanup>,
    /// The session's MCP servers, which outlive single runs (ADR-0011).
    pub mcp: Arc<tokio::sync::Mutex<crate::mcp::McpSupervisor>>,
    /// The session's desktop driver, made at its first run whose environment declares `desktop` (ADR-0039).
    pub desktop: crate::desktop::Slot,
    pub repository_cloner: Arc<dyn tqa_tools::RepositoryCloner>,
    pub jitter: Arc<Mutex<Box<dyn Jitter>>>,
    /// This runtime's link, settled by the runner after a model attempt's transport failure (CORE-U-062).
    pub gateway_link: Arc<dyn tqa_core::GatewayLink>,
    /// The conversation's screenshot store over the link, for a `computer` call's captures (ADR-0039 section 9).
    pub screen_store: Arc<crate::screen_store::LinkScreenStore>,
}
pub(crate) struct Invocation {
    pub session: Session,
    pub history: Vec<ServerFrame>,
    pub message: Option<UserTurn>,
    /// The run's host keys, from the message that started it; none for a resumed run (ADR-0037 §3).
    pub host_keys: Vec<tqa_types::HostKeyGrant>,
    pub cancel: CancellationToken,
    pub shutdown: CancellationToken,
}
impl Services {
    pub async fn invoke(&self, input: Invocation) -> Result<RunOutcome, RuntimeError> {
        let result = self.execute(input).await;
        let cleanup = self.cleanup.reap().await;
        cleanup?;
        result
    }
    async fn execute(&self, mut input: Invocation) -> Result<RunOutcome, RuntimeError> {
        #[cfg(feature = "testing")]
        self.testing.before_run.checkpoint(&input.shutdown).await;
        if let Some(failure) = self.initialize_workspace(&mut input).await? {
            return tqa_core::fail_before_run(
                self.journal.as_ref(),
                tqa_core::PreRunFailure {
                    history: &input.history,
                    message: input.message,
                    code: failure.code,
                    reason: failure.reason,
                },
                &input.shutdown,
            )
            .await
            .map_err(|error| core_error(&error, &self.journal));
        }
        // Nothing to run (no message, no unfinished run, no tool call left open): the start-up pass after
        // registration. It returns before the tools are prepared, so a message waiting behind it doesn't pay for their
        // network calls and MCP servers twice (a hanging role call made its first run wait twice its bound).
        if input.message.is_none()
            && matches!(
                tqa_context::resume_point(&input.history),
                Ok(tqa_context::ResumePoint::Idle { ref dangling }) if dangling.is_empty()
            )
        {
            return Ok(RunOutcome::Idle);
        }
        let (workspace, prompt, mut tools) = self.prepare(&input).await?;
        let mut history = std::mem::take(&mut input.history);
        self.prepare_mcp(&input, &mut tools, &mut history).await?;
        input.history = history;
        self.prepare_channels(&input, &mut tools).await?;
        self.prepare_teams(&input, &mut tools).await?;
        let fallback_routes = crate::provider::fallbacks(&self.config, &input.session, &self.time)?;
        let decision = self.decision_engine(&input.session, &tools)?;
        let runner = Runner::new(RunnerDeps {
            conversation_id: input.session.conversation.clone(),
            provider: crate::provider::build(&self.config, &input.session, &self.time)?,
            tools: Arc::new(tools),
            decision,
            sink: self.journal.clone(),
            clock: self.time.clock.clone(),
            sleeper: self.time.sleeper.clone(),
            prompt,
            spill_dir: self
                .config
                .workspace
                .join(".tqa")
                .join(input.session.conversation.as_str())
                .join("spill"),
            limits: input.session.limits,
            route: input.session.route,
            workspace,
            env_allow: vec![],
            tool_limits: ToolLimits::RECOMMENDED,
            jitter: Box::new(SharedJitter(self.jitter.clone())),
            fallback_routes,
            gateway_link: Some(self.gateway_link.clone()),
            // In the runtime's state, which never overlaps the workspace: the agent's own tools can't reach them there.
            screenshots: Some(tqa_core::Screenshots::new(
                &self.config.state_dir.join("screenshots"),
            )),
            screen_store: Some(self.screen_store.clone()),
        })
        .map_err(|_| RuntimeError::Configuration("core services"))?;
        runner
            .run(&input.history, input.message, input.cancel, input.shutdown)
            .await
            .map_err(|error| core_error(&error, &self.journal))
    }
    /// The engine the session names, over the gateway relay with the runtime's own token; without one, the model
    /// decides and the loop is unchanged (ADR-0013 point 1).
    fn decision_engine(
        &self,
        session: &Session,
        tools: &ToolRegistry,
    ) -> Result<Arc<dyn tqa_core::DecisionEngine>, RuntimeError> {
        let Some(engine) = &session.decision_engine else {
            return Ok(Arc::new(tqa_core::ModelDecides));
        };
        let mut base = self.config.origin.clone();
        base.set_path(&engine.relay_path);
        let routes = std::iter::once(&session.route)
            .chain(&session.fallback_routes)
            .map(|route| tqa_types::decision_v1::Route {
                name: route.name.clone(),
                context_window: route.context_window,
                max_output: route.max_output,
                native_tools: route.native_tools,
            })
            .collect();
        let relay = crate::decision_relay::RelayDecisionEngine::new(
            crate::decision_relay::RelayEngineConfig {
                name: engine.name.clone(),
                base,
                token: self.config.token.clone(),
                timeout: std::time::Duration::from_millis(u64::from(engine.timeout_ms)),
                exposure: engine.exposure,
                tools: tools
                    .specs()
                    .map(|spec| {
                        (
                            spec.name.as_str().to_owned(),
                            spec.description.clone(),
                            spec.parameters.clone(),
                        )
                    })
                    .collect(),
                routes,
                points: engine.points.clone(),
            },
        )
        .map_err(|_| RuntimeError::Configuration("decision engine client"))?;
        Ok(Arc::new(relay))
    }
    /// Starts or restarts the session's MCP servers for this run, registers their tools and journals each transition.
    /// The session's MCP servers, ready for this run, and their `mcp.server` events journaled. Each event is also added
    /// to `history`, the run's view of the journal, as the workspace's events are: the run's first event follows them, and
    /// a view that ended before them would see a gap and stop the run (the c14a connector P0, RT-I-043).
    async fn prepare_mcp(
        &self,
        input: &Invocation,
        tools: &mut ToolRegistry,
        history: &mut Vec<ServerFrame>,
    ) -> Result<(), RuntimeError> {
        let events = {
            let mut mcp = self.mcp.lock().await;
            mcp.configure(&input.session.mcp).await;
            mcp.prepare_run(tools, &self.config.workspace).await
        };
        for event in events {
            let seq = self
                .journal
                .append(event.clone(), tqa_core::EventContext::default())
                .await?;
            history.push(ServerFrame::Event {
                seq,
                run_id: None,
                step: None,
                ts: tqa_core::LOCAL_PROJECTION_TIMESTAMP.into(),
                event,
            });
        }
        Ok(())
    }
    /// The channel tools, when the conversation belongs to a channel at this run's start (ADR-0014; asked at every run
    /// start, since membership changes between runs) and the profile's allow-list lets them in. The question is bounded:
    /// a gateway that doesn't answer in time leaves the tools out of this run.
    /// The run's agent-team tools, by the role the gateway gives it (ADR-0034 slice 3): a member its two, a lead its
    /// own, a task that may lead the lead's with `start_member` alone offered until it succeeds, otherwise none. They
    /// come from the role, not the profile's tool list: a member must always be able to report to its lead. An older
    /// gateway, or one that doesn't answer in time, gives none.
    async fn prepare_teams(
        &self,
        input: &Invocation,
        tools: &mut ToolRegistry,
    ) -> Result<(), RuntimeError> {
        let client = Arc::new(crate::teams::GatewayTeams::new(
            &self.config.origin,
            &self.config.token,
        )?);
        // A role the gateway can't give in time leaves this run without team tools rather than hold up its start; a
        // member then can't report this run, so the timeout is logged (an older gateway's 404 is not).
        let role = if let Ok(role) =
            tokio::time::timeout(ROLE_TIMEOUT, client.role(&input.shutdown)).await
        {
            role
        } else {
            tracing::warn!(
                "the agent-team role call timed out; the team tools are left out of this run"
            );
            builtin::team::TeamRole::None
        };
        // A lead's `done` runs the administrator's acceptance check through the run's own shell tool (ADR-0034
        // point 10): the same directory, environment and process cleanup as any shell command.
        let check = tools
            .get("shell")
            .cloned()
            .map(builtin::team::AcceptanceShell::new);
        for tool in builtin::team::tools_with_check(client, role, check) {
            tools
                .register(tool)
                .map_err(|_| RuntimeError::Configuration("team tool schemas"))?;
        }
        Ok(())
    }
    async fn prepare_channels(
        &self,
        input: &Invocation,
        tools: &mut ToolRegistry,
    ) -> Result<(), RuntimeError> {
        let allowed = input
            .session
            .profile
            .as_ref()
            .and_then(|profile| profile.tools.as_deref());
        let wanted = |name: &str| {
            allowed.is_none_or(|allowed| allowed.iter().any(|allowed| allowed == name))
        };
        if !CHANNEL_TOOLS.iter().any(|name| wanted(name)) {
            return Ok(());
        }
        let client = Arc::new(crate::channels::GatewayChannels::new(
            &self.config.origin,
            &self.config.token,
        )?);
        let found = tokio::time::timeout(
            MEMBERSHIP_TIMEOUT,
            builtin::channel::tools_for_run(client, &input.shutdown),
        )
        .await
        .unwrap_or_default();
        for tool in found {
            if wanted(tool.spec().name.as_str()) {
                tools
                    .register(tool)
                    .map_err(|_| RuntimeError::Configuration("channel tool schemas"))?;
            }
        }
        Ok(())
    }
    async fn prepare(
        &self,
        input: &Invocation,
    ) -> Result<(WorkspaceRoot, PromptInputs, ToolRegistry), RuntimeError> {
        let config = self.config.clone();
        let workspace = input.session.workspace.clone();
        let cleanup = self.cleanup.clone();
        let environment = input.session.environment.clone();
        let conversation = input.session.conversation.clone();
        let profile = input.session.profile.clone();
        let network = input.session.network.clone();
        let host_keys = input.host_keys.clone();
        let host_ca = input.session.host_ca.clone();
        // The desktop is made here, off the blocking preparation: starting the browser is asynchronous.
        // What the route lets the computer tool offer: `screenshot` needs a route that takes images (ADR-0039 section 3).
        let view = tqa_tools::builtin::computer::View {
            vision: input.session.route.vision,
            pixel_grounding: input.session.route.pixel_grounding,
            limits: input.session.computer_limits,
            // A resumed run's caps carry on from what its journal shows it spent.
            spent: tqa_tools::builtin::computer::Spent::from_journal(&input.history),
            ..tqa_tools::builtin::computer::View::default()
        };
        let desktop = if environment.capabilities.contains(&Capability::Desktop) {
            // A desktop session: the display's clients (the applications the agent opens) outlive the reaps, as the
            // runtime's own browser does (ADR-0039 section 10).
            let spared = self.cleanup.supervised();
            spared.display_clients(std::path::Path::new(crate::desktop::DISPLAY_SOCKET));
            Some((crate::desktop::driver(&self.desktop, &spared).await, view))
        } else {
            None
        };
        let mut preparation = tokio::task::spawn_blocking(move || {
            prepare(
                &config,
                (
                    &workspace,
                    &environment,
                    profile.as_ref(),
                    &network,
                    desktop,
                ),
                (&host_keys, host_ca.as_deref()),
                &cleanup,
                &conversation,
            )
        });
        let prepared = self
            .time
            .wait(self.limits.io_timeout, &input.shutdown, &mut preparation)
            .await;
        match prepared {
            Ok(result) => result.map_err(|_| RuntimeError::Worker)?,
            Err(error) => {
                preparation.abort();
                Err(error)
            }
        }
    }
    async fn initialize_workspace(
        &self,
        input: &mut Invocation,
    ) -> Result<Option<crate::workspace::WorkspaceFailure>, RuntimeError> {
        crate::workspace::WorkspaceInitializer {
            workspace: self.config.workspace.clone(),
            state_dir: self.config.state_dir.clone(),
            proxy: self.config.proxy.clone(),
            cloner: self.repository_cloner.clone(),
            sleeper: self.time.sleeper.clone(),
        }
        .initialize(
            input.session.workspace.clone.as_ref(),
            &mut input.history,
            self.journal.as_ref(),
            &input.shutdown,
        )
        .await
    }
}
/// The core's failure as the runtime's: a gap between the core's view of the journal and the sequence it was given is a
/// history fault, not storage (its log said "storage I/O failed" in the c14a connector P0); any other sink failure names
/// the journal's own cause (the WAL write, its sync, a closed channel), which the core's sink keeps only as a code.
fn core_error(error: &tqa_core::CoreError, journal: &Journal) -> RuntimeError {
    match error {
        tqa_core::CoreError::Sink(sink) if sink.code == tqa_types::ErrorCode::SeqGap => {
            RuntimeError::History
        }
        tqa_core::CoreError::Sink(_) => journal
            .take_failure()
            .unwrap_or_else(|| RuntimeError::io("event sink")),
        tqa_core::CoreError::History(_) => RuntimeError::History,
        _ => RuntimeError::Worker,
    }
}
struct SharedJitter(Arc<Mutex<Box<dyn Jitter>>>);
impl Jitter for SharedJitter {
    fn next_fraction(&mut self) -> f64 {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .next_fraction()
    }
}
/// The channel tools' names (ADR-0014 point 6).
const CHANNEL_TOOLS: [&str; 4] = tqa_tools::builtin::CHANNEL_TOOL_NAMES;
/// How long a run start waits for its channel memberships.
const MEMBERSHIP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
/// How long a run start waits for its agent-team role: the gateway answers it from a few indexed rows (and at once while
/// agent teams are off), so a longer wait is a sick gateway, not a slow answer (ADR-0034 slice 3).
const ROLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// The session's desktop driver and what the route lets the tool offer, for an environment that declares `desktop`,
/// else none (ADR-0039).
type Desktop = Option<(
    Arc<dyn tqa_tools::builtin::computer::driver::ScreenDriver>,
    tqa_tools::builtin::computer::View,
)>;

/// The run's workspace, prompt and tools; a profile narrows the built-in tools and skills and adds its instructions
/// (ADR-0012, RT-I-028).
fn prepare(
    config: &RuntimeConfig,
    (snapshot, environment, profile, network, desktop): (
        &WorkspaceSnapshot,
        &Environment,
        Option<&ProfileSnapshot>,
        &tqa_types::NetworkSnapshot,
        Desktop,
    ),
    (host_keys, host_ca): (&[tqa_types::HostKeyGrant], Option<&str>),
    cleanup: &Arc<dyn ProcessCleanup>,
    conversation: &tqa_types::ConversationId,
) -> Result<(WorkspaceRoot, PromptInputs, ToolRegistry), RuntimeError> {
    let workspace =
        WorkspaceRoot::open(&config.workspace).map_err(|_| RuntimeError::io("workspace open"))?;
    let (instructions, loaded, skills) = instruction_slot(config, &workspace, profile)?;
    let commands = crate::commands::available(&std::env::var_os("PATH").unwrap_or_default());
    let prompt = PromptInputs {
        system: tqa_prompt::system_prompt(&tqa_prompt::WorkspaceFacts {
            root_display: &snapshot.display,
            os: std::env::consts::OS,
            environment,
            network,
            commands: &commands,
            host_keys,
        }),
        instructions,
        loaded: Some(loaded),
    };
    let shell = config
        .proxy
        .clone()
        .map_or_else(builtin::Shell::new, |proxy| {
            builtin::Shell::new().with_proxy(proxy)
        })
        .with_host_keys(host_keys);
    // A Desktop session: screen input is the computer tool's, and the shell says so (TOOL-U-064).
    let shell = if environment.capabilities.contains(&Capability::Desktop) {
        shell.with_desktop()
    } else {
        shell
    };
    let shell = match ca_bundle(config, host_ca)? {
        Some(bundle) => shell.with_ca_bundle(bundle),
        None => shell,
    };
    let mut tools = Builtins {
        registry: ToolRegistry::new(),
        allowed: profile.and_then(|profile| profile.tools.as_deref()),
    };
    for (tool, required) in [
        (
            Arc::new(builtin::ReadFile::new()) as Arc<dyn tqa_tools::Tool>,
            None,
        ),
        (Arc::new(builtin::WriteFile::new()), None),
        (Arc::new(builtin::EditFile::new()), None),
        (Arc::new(builtin::Glob::new()), None),
        (Arc::new(builtin::Grep::new()), None),
        (Arc::new(builtin::ReadNotes::new(conversation)), None),
        (Arc::new(builtin::WriteNotes::new(conversation)), None),
        (
            Arc::new(crate::shell::ReapingTool {
                tool: shell,
                cleanup: cleanup.clone(),
            }),
            Some(Capability::Shell),
        ),
    ] {
        if required.is_some_and(|capability| !environment.capabilities.contains(&capability)) {
            continue;
        }
        tools.register(tool)?;
    }
    // The computer tool joins only an environment that declares `desktop` (capabilities are declared, never detected;
    // ADR-0039), and a profile's allow-list may still leave it out.
    if let Some((desktop, view)) = desktop {
        tools.register(Arc::new(builtin::Computer::with_view(desktop, view)))?;
    }
    // The skill tool joins the catalog only when there is a skill to load (ADR-0029).
    if !skills.skills.is_empty() {
        let tool = tqa_prompt::SkillTool::new(&skills)
            .map_err(|_| RuntimeError::Configuration("skill tool"))?;
        tools.register(Arc::new(tool))?;
    }
    if let Some(proxy) = &config.proxy {
        let fetch = builtin::FetchUrl::new(proxy.clone(), &config.token)
            .map_err(|_| RuntimeError::Configuration("egress proxy tool"))?;
        tools.register(Arc::new(fetch))?;
    }
    Ok((workspace, prompt, tools.registry))
}

/// The image's own CA roots, which the install CA joins.
const SYSTEM_BUNDLE: &str = "/etc/ssl/certs/ca-certificates.crt";

/// Writes the run's CA bundle, the image's roots and then the install CA, to the state directory (ADR-0037 §6). None
/// without an install CA, or when the image has no roots to extend: a bundle of the install CA alone would break every
/// other site, while without it only keyed hosts fail, with a certificate error.
fn ca_bundle(
    config: &RuntimeConfig,
    host_ca: Option<&str>,
) -> Result<Option<std::path::PathBuf>, RuntimeError> {
    let Some(host_ca) = host_ca else {
        return Ok(None);
    };
    let Ok(system) = std::fs::read_to_string(SYSTEM_BUNDLE) else {
        tracing::warn!("the image has no CA roots; keyed hosts' certificates won't be trusted");
        return Ok(None);
    };
    let path = config.state_dir.join("host-ca-bundle.pem");
    let separator = if system.ends_with('\n') { "" } else { "\n" };
    std::fs::write(&path, format!("{system}{separator}{host_ca}"))
        .map_err(|error| RuntimeError::Io(IoCause::of("host CA bundle write", &error)))?;
    Ok(Some(path))
}

/// The built-in tools of a run: only those a profile's allow-list names, when it has one. Another tool is never
/// registered, so the core answers a call to it as unknown (CORE-U-031). MCP tools are not built-ins and never pass
/// through here: the conversation's servers and connectors, which the profile narrows at the gateway, decide them.
struct Builtins<'a> {
    registry: ToolRegistry,
    allowed: Option<&'a [String]>,
}

impl Builtins<'_> {
    fn register(&mut self, tool: Arc<dyn tqa_tools::Tool>) -> Result<(), RuntimeError> {
        let name = tool.spec().name.as_str();
        if self
            .allowed
            .is_some_and(|allowed| !allowed.iter().any(|allowed| allowed == name))
        {
            return Ok(());
        }
        self.registry
            .register(tool)
            .map_err(|_| RuntimeError::Configuration("builtin schemas"))
    }
}
/// The instruction slot: repository instructions, then the skills index from the fixed tier mounts, with the
/// `context.loaded` report of both (FR-11).
fn instruction_slot(
    config: &RuntimeConfig,
    workspace: &WorkspaceRoot,
    profile: Option<&ProfileSnapshot>,
) -> Result<(String, ContextLoaded, tqa_prompt::SkillSet), RuntimeError> {
    let files = tqa_prompt::discover(workspace, std::path::Path::new("."))
        .map_err(|_| RuntimeError::io("instructions discovery"))?;
    let mut skills = tqa_prompt::discover_skills(&[
        (SkillTier::Install, &config.skills.join("install")),
        (SkillTier::User, &config.skills.join("user")),
    ]);
    // A profile's filter trims the index and the skill tool alike (ADR-0012); the report shows what the run sees. An
    // allow-list without the skill tool leaves no way to load a skill, so none is indexed either.
    if let Some(profile) = profile {
        let loadable = profile
            .tools
            .as_ref()
            .is_none_or(|tools| tools.iter().any(|tool| tool == "skill"));
        skills
            .skills
            .retain(|skill| loadable && allows(&profile.skills, &skill.name));
    }
    let mut instructions = profile.map_or_else(String::new, |profile| {
        tqa_prompt::render_profile_instructions(&profile.name, profile.instructions.as_deref())
    });
    instructions.push_str(&tqa_prompt::render(&files));
    instructions.push_str(&tqa_prompt::render_skill_index(&skills));
    let loaded = ContextLoaded {
        instructions: files
            .iter()
            .map(|file| LoadedInstruction {
                path: file.path.display().to_string(),
                truncated: file.truncated,
            })
            .collect(),
        skills: skills
            .skills
            .iter()
            .map(|skill| LoadedSkill {
                name: skill.name.clone(),
                tier: skill.tier,
                description: skill.description.clone(),
            })
            .collect(),
        skipped: skills
            .skipped
            .iter()
            .map(|skip| SkippedSkill {
                folder: skip.folder.clone(),
                tier: skip.tier,
                reason: skip.reason,
            })
            .collect(),
    };
    Ok((instructions, loaded, skills))
}
/// Whether a profile's skills filter lets the run see the skill `name`.
fn allows(filter: &SkillsFilter, name: &str) -> bool {
    match filter {
        SkillsFilter::All => true,
        SkillsFilter::Only(names) => names.iter().any(|allowed| allowed == name),
        // None, and any filter this runtime doesn't know, shows nothing.
        _ => false,
    }
}
