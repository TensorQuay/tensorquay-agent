//! Explicit link-only projection using the shared protocol snapshot types.
use crate::{GatewayError, app::State};
use tqa_types::{
    ClientFrame, ConversationId, ReasoningSnapshot, RouteSnapshot, SandboxState, SandboxSummary,
    ServerFrame, WireProtocol, WorkspaceSnapshot,
};

/// A desktop task's step limit is at least this (ADR-0039 section 4, the owner's question 4); a profile may narrow it.
const DESKTOP_MAX_STEPS: u32 = 500;

/// The install's run limits for this environment: a desktop task takes many small steps, so its limit is at least
/// [`DESKTOP_MAX_STEPS`].
fn install_limits(
    mut install: tqa_types::RunLimits,
    environment: &tqa_types::Environment,
) -> tqa_types::RunLimits {
    if environment
        .capabilities
        .contains(&tqa_types::Capability::Desktop)
    {
        install.max_steps = install.max_steps.max(DESKTOP_MAX_STEPS);
    }
    install
}

/// The `computer` tool's caps for a desktop environment, from the administrator's settings (ADR-0039 section 4); none
/// for any other, whose runtime has no `computer` tool. The limits' bounds are the protocol's, so they always fit.
fn computer_limits(
    limits: &crate::settings::limits::Limits,
    environment: &tqa_types::Environment,
) -> Result<Option<tqa_types::ComputerLimits>, GatewayError> {
    if !environment
        .capabilities
        .contains(&tqa_types::Capability::Desktop)
    {
        return Ok(None);
    }
    tqa_types::ComputerLimits::new(
        limits.computer_max_screenshots_per_run,
        limits.computer_max_actions_per_run,
    )
    .map(Some)
    .map_err(|_| GatewayError::Configuration("computer limits out of their bounds"))
}

/// Publishes the registered sandbox's state before the runtime replays its write-ahead log.
pub(super) async fn ready(
    state: &State,
    conversation: &ConversationId,
) -> Result<(), GatewayError> {
    let resumed = state.events.watermark(conversation).await? != 0.into();
    let activity = state.events.run_activity(conversation).await?;
    notice(state, conversation, activity.registered_state(), resumed)
}

/// Publishes the client-facing sandbox state as an unsequenced control frame.
pub(super) fn notice(
    state: &State,
    conversation: &ConversationId,
    sandbox: SandboxState,
    resumed: bool,
) -> Result<(), GatewayError> {
    state.events.control(
        conversation,
        ServerFrame::SessionReady {
            conversation_id: conversation.clone(),
            resumed,
            sandbox: SandboxSummary::new(state.config.backend, sandbox),
            queue: None,
        },
    )
}

/// The `session.config` for a runtime at or above the protocol floor.
pub(super) async fn encode(
    state: &State,
    conversation: &ConversationId,
    revision: u16,
) -> Result<String, GatewayError> {
    let id = crate::conversations::uuid(conversation)?;
    let record = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        sqlx::query!(
            "SELECT model_route,template,mcp_servers,connectors,profile FROM conversations WHERE id=$1 AND state='active'",
            id
        )
        .fetch_optional(&state.pool)
        .await?
        .ok_or(GatewayError::NotFound)
    })
    .await?;
    let name = record.model_route;
    // The profile's engine, from the conversation's own copy (never a ProfileSnapshot field): the link has already
    // refused a runtime older than revision 9 for it.
    let engine = record
        .profile
        .as_ref()
        .and_then(|profile| profile.get("decision_engine"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let environment = crate::io::bounded(
        state.config.io_timeout(),
        &state.cancel,
        crate::environments::binding(&state.pool, id),
    )
    .await?
    .environment()?;
    // The install's limits apply at every session; a profile copy only narrows them (ADR-0012).
    let install = install_limits(state.config.run_limits, &environment);
    let (limits, profile) = crate::profiles::effective(install, record.profile)?;
    // One snapshot for the primary route, its fallbacks and the connectors (ADR-0028).
    let settings = state.settings.current();
    let mcp = super::mcp::entries(
        &state.config.mcp,
        &settings.connectors,
        &record.template,
        (&record.mcp_servers, &record.connectors),
    );
    let decision_engine = engine
        .map(|engine| decision_engine(&settings.decision_engines, &engine))
        .transpose()?;
    let route = route_snapshot(state, &settings.registry, &name)?;
    let fallback_routes = fallback_snapshots(state, &settings.registry, &name)?;
    let last_stored_seq = state.events.watermark(conversation).await?;
    // A workspace created from git is cloned by the runtime into the empty workspace (057, task 066a).
    let workspace = crate::io::bounded(
        state.config.io_timeout(),
        &state.cancel,
        crate::workspaces::of_conversation(&state.pool, id),
    )
    .await?;
    let frame = ClientFrame::SessionConfig {
        gateway_revision: Some(tqa_types::PROTOCOL_REVISION),
        run_host_keys: run_host_keys(state, conversation, revision).await,
        // The administrator's caps for a desktop environment; a change applies at the next session.
        computer_limits: computer_limits(&settings.limits, &environment)?,
        // Only a runtime that can take it (revision 11): an older one would refuse the whole configuration.
        host_ca: state
            .host_ca
            .as_ref()
            .filter(|_| revision >= tqa_types::HOST_KEYS_REVISION)
            .map(|ca| ca.bundle_pem()),
        conversation_id: conversation.clone(),
        last_stored_seq,
        route,
        // The sites the sandbox may reach, for the prompt (ADR-0035); a change applies at the next session.
        network: crate::egress::network_snapshot(&settings, state.config.egress.disabled),
        limits,
        environment,
        workspace: WorkspaceSnapshot {
            display: "/workspace".into(),
            clone: workspace
                .source_url
                .map(|url| tqa_types::WorkspaceClone { url, r#ref: None }),
        },
        protocol: 1,
        fallback_routes,
        mcp,
        profile,
        decision_engine,
    };
    let encoded = serde_json::to_string(&frame)?;
    ClientFrame::parse(encoded.as_bytes())
        .map_err(|_| GatewayError::Configuration("invalid runtime session snapshot"))?;
    Ok(encoded)
}

/// The runtime's view of the engine a profile names: its relay path, deadline and exposure, never its address or
/// credential. A missing or disabled engine refuses the session, never runs it without the engine (ADR-0013 point 6).
///
/// # Errors
/// `409 decision_engine_unavailable` when the engine is gone or disabled.
/// Whether the engine `name` is stored and enabled.
pub(super) fn engine_available(
    engines: &[crate::decision_engines::EngineDocument],
    name: &str,
) -> bool {
    engines
        .iter()
        .any(|engine| engine.name == name && engine.enabled)
}

fn decision_engine(
    engines: &[crate::decision_engines::EngineDocument],
    name: &str,
) -> Result<tqa_types::DecisionEngineSnapshot, GatewayError> {
    let engine = engines
        .iter()
        .find(|engine| engine.name == name && engine.enabled)
        .ok_or(GatewayError::Conflict("decision_engine_unavailable"))?;
    Ok(tqa_types::DecisionEngineSnapshot {
        name: engine.name.clone(),
        relay_path: tqa_types::DecisionEngineSnapshot::relay_path_for(&engine.name),
        timeout_ms: engine.timeout_ms,
        exposure: engine.exposure,
        points: engine.protocol.points(),
    })
}

/// The route's admin-declared fallbacks (074), in order, each as the runtime sees any route: its relay path and
/// the same request limit. The registry has already refused unknown names, cycles and more than three.
///
/// # Errors
/// Fails when a fallback is no longer registered or uses an unsupported protocol.
pub(crate) fn fallback_snapshots(
    state: &State,
    registry: &tqa_llm::Registry,
    name: &str,
) -> Result<Vec<RouteSnapshot>, GatewayError> {
    let route = registry.get(name).ok_or(GatewayError::StoreUnavailable)?;
    route
        .fallbacks
        .iter()
        .map(|fallback| route_snapshot(state, registry, fallback))
        .collect()
}

/// The runtime's view of a registered route: its relay path and limits, never credentials or upstream settings.
///
/// # Errors
/// Fails when the stored route is no longer registered or uses an unsupported protocol.
pub(crate) fn route_snapshot(
    state: &State,
    registry: &tqa_llm::Registry,
    name: &str,
) -> Result<RouteSnapshot, GatewayError> {
    let route = registry.get(name).ok_or(GatewayError::StoreUnavailable)?;
    let mut relay = state.config.public_origin.clone();
    relay
        .path_segments_mut()
        .map_err(|()| GatewayError::StoreUnavailable)?
        .clear()
        .extend(["relay", "r", &route.name]);
    // The relay refuses a larger body; the runtime keeps its requests (images included) within it (071).
    let max_request_bytes = u32::try_from(state.config.relay.body_bytes)
        .map_err(|_| GatewayError::Configuration("relay body limit exceeds the protocol range"))?;
    snapshot(route, relay.path().to_owned(), max_request_bytes)
}

/// The route's first-response bound in whole seconds: its setting, else 300 for a local route and 60 otherwise.
pub(crate) fn first_response_secs(route: &tqa_llm::Route) -> u32 {
    u32::try_from(
        route
            .first_response(crate::relay::local_route(route))
            .as_secs(),
    )
    .unwrap_or(u32::MAX)
}

fn snapshot(
    route: &tqa_llm::Route,
    relay_path: String,
    max_request_bytes: u32,
) -> Result<RouteSnapshot, GatewayError> {
    let protocol = match route.protocol {
        tqa_llm::Protocol::OpenAiChat => WireProtocol::OpenAiChat,
        tqa_llm::Protocol::OpenAiResponses => WireProtocol::OpenAiResponses,
        tqa_llm::Protocol::AnthropicMessages => WireProtocol::AnthropicMessages,
        _ => {
            return Err(GatewayError::Configuration(
                "unsupported session wire protocol",
            ));
        }
    };
    let snapshot = RouteSnapshot {
        name: route.name.clone(),
        protocol,
        model: route.model.clone(),
        relay_path,
        context_window: route.limits.context_window,
        max_output: route.limits.max_output,
        native_tools: route.native_tools,
        parallel_tools: route.parallel_tools,
        vision: route.vision,
        reasoning: route.reasoning.as_ref().map(|reasoning| ReasoningSnapshot {
            allowed: reasoning.allowed.clone(),
            default: reasoning.default.clone(),
            budget_tokens: reasoning.budget_tokens.clone(),
        }),
        max_request_bytes: Some(max_request_bytes),
        cache_hint: route.cache_hint,
        // The relay waits this long for the model's first byte (its own setting, else by where the model runs); the
        // runtime's wait follows it.
        first_response_secs: Some(first_response_secs(route)),
        // The administrator's route setting arrives with the gateway's half of ADR-0039; off until then.
        pixel_grounding: false,
    };
    snapshot
        .validate()
        .map_err(|_| GatewayError::Configuration("invalid runtime route snapshot"))?;
    Ok(snapshot)
}

/// The keys of a run in progress, newly minted for the runtime registering now, which gives them to the run when it
/// resumes (G4b); none for a runtime below revision 11. Keys are a convenience, never a condition: when they can't be
/// minted the configuration goes without them.
async fn run_host_keys(
    state: &State,
    conversation: &ConversationId,
    revision: u16,
) -> Vec<tqa_types::HostKeyGrant> {
    let (Ok(id), Some((sandbox, _))) = (
        crate::conversations::uuid(conversation),
        state.links.sandbox(conversation),
    ) else {
        return Vec::new();
    };
    if revision < tqa_types::HOST_KEYS_REVISION {
        return Vec::new();
    }
    let settings = state.settings.current();
    let minted = crate::io::bounded(
        state.config.io_timeout(),
        &state.cancel,
        crate::host_keys::for_resume(
            &state.pool,
            (id, sandbox.as_str()),
            (&settings.egress, state.config.egress.disabled),
        ),
    )
    .await;
    minted.unwrap_or_else(|error| {
        tracing::warn!(conversation_id = %conversation, code = error.code(), "a resumed run goes without its host keys");
        Vec::new()
    })
}
