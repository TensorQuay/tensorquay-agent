//! The session's MCP servers (ADR-0011 decision 3, ADR-0031). Servers live for the session.
//! At each run start a stdio server is started, restarted once after a crash, or disabled after a second crash, and a
//! changed tool list applies here, at the run boundary; a gateway connector is connected through the proxy. Their tools
//! are registered into the run's registry, and each transition is an `mcp.server` event. A failing server never stops the
//! run or the other servers.

use std::{path::Path, sync::Arc, time::Duration};

use secrecy::SecretString;
use tqa_tools::ToolRegistry;
use tqa_types::{Event, McpServer, McpServerState, McpTransport};
use url::Url;

use super::{
    GatewayServer, ListedTool, McpFailure, StdioServer,
    tool::{Backend, McpTool},
};
use crate::SupervisedGroups;

/// Crashes a server may have before it is disabled for the session.
const MAX_CRASHES: u8 = 2;

/// What a run-start check did to a server.
enum Outcome {
    /// Still running as before; nothing to report.
    Unchanged,
    /// Started or restarted.
    Up(McpServerState),
    /// Disabled for the session, with a reason code.
    Disabled(&'static str),
}

/// What a running server needs at a run boundary.
enum Settled {
    /// It keeps running as it is.
    Keep,
    /// It is (re)started, reported with this state.
    Start(McpServerState),
    /// It is disabled for the session.
    Disable(&'static str),
}

struct Slot {
    config: McpServer,
    server: Option<Backend>,
    crashes: u8,
    disabled: bool,
}

/// Owns the session's MCP servers across runs.
pub struct McpSupervisor {
    slots: Vec<Slot>,
    groups: SupervisedGroups,
    handshake: Duration,
    gateway: Option<(Url, SecretString)>,
}

impl McpSupervisor {
    /// A supervisor with no servers. Spawned groups are recorded in `groups` for the reaper, and connectors are reached at
    /// `gateway`'s origin with its token; without it a connector is `transport_unavailable`.
    #[must_use]
    pub const fn new(
        groups: SupervisedGroups,
        handshake: Duration,
        gateway: Option<(Url, SecretString)>,
    ) -> Self {
        Self {
            slots: Vec::new(),
            groups,
            handshake,
            gateway,
        }
    }

    /// Applies the session's list: unchanged servers keep running, removed or changed ones are stopped, new ones wait for
    /// the next run.
    pub async fn configure(&mut self, configs: &[McpServer]) {
        let mut previous = std::mem::take(&mut self.slots);
        for config in configs {
            match previous.iter().position(|slot| &slot.config == config) {
                Some(index) => self.slots.push(previous.remove(index)),
                None => self.slots.push(Slot {
                    config: config.clone(),
                    server: None,
                    crashes: 0,
                    disabled: false,
                }),
            }
        }
        for slot in previous {
            self.retire(slot.server).await;
        }
    }

    /// Starts what is needed for this run, registers the servers' tools and returns the transitions as events.
    pub async fn prepare_run(
        &mut self,
        registry: &mut ToolRegistry,
        workspace: &Path,
    ) -> Vec<Event> {
        let mut events = Vec::new();
        for index in 0..self.slots.len() {
            if self.slots[index].disabled {
                continue;
            }
            let reported = match self.check(index, workspace).await {
                Outcome::Unchanged => None,
                Outcome::Up(state) => Some(state),
                Outcome::Disabled(reason) => {
                    let config = &self.slots[index].config;
                    events.push(event(
                        config,
                        McpServerState::Disabled,
                        0,
                        Some(reason.to_owned()),
                    ));
                    continue;
                }
            };
            let Some(backend) = self.slots[index].server.clone() else {
                continue;
            };
            let config = &self.slots[index].config;
            let (registered, skipped) = register_tools(&backend, config, registry);
            if let Some(state) = reported {
                let reason = skipped.then(|| "tool_skipped".to_owned());
                events.push(event(config, state, registered, reason));
            }
        }
        events
    }

    /// Makes slot `index` usable if it can be.
    async fn check(&mut self, index: usize, workspace: &Path) -> Outcome {
        let state = match self.slots[index].server.clone() {
            None => McpServerState::Ready,
            Some(backend) => match self.settle(index, backend).await {
                Settled::Keep => return Outcome::Unchanged,
                Settled::Disable(reason) => return Outcome::Disabled(reason),
                Settled::Start(state) => state,
            },
        };
        match self.start(&self.slots[index].config, workspace).await {
            Ok(backend) => {
                if let Backend::Stdio(server) = &backend {
                    self.groups.add(server.process_group());
                }
                self.slots[index].server = Some(backend);
                Outcome::Up(state)
            }
            Err(failure) => {
                self.slots[index].disabled = true;
                Outcome::Disabled(failure.code())
            }
        }
    }

    async fn start(&self, config: &McpServer, workspace: &Path) -> Result<Backend, McpFailure> {
        match &config.transport {
            McpTransport::Stdio { .. } => StdioServer::start(config, workspace, self.handshake)
                .await
                .map(|server| Backend::Stdio(Arc::new(server))),
            McpTransport::Gateway { connector } => {
                let (origin, token) = self
                    .gateway
                    .as_ref()
                    .ok_or(McpFailure::TransportUnavailable)?;
                let call_timeout = Duration::from_secs(u64::from(config.call_timeout_seconds));
                GatewayServer::connect(
                    origin,
                    connector,
                    token.clone(),
                    self.handshake,
                    call_timeout,
                )
                .await
                .map(|server| Backend::Gateway(Arc::new(server)))
            }
            _ => Err(McpFailure::TransportUnavailable),
        }
    }

    /// What a running server needs at this boundary: a crashed stdio server is restarted once and disabled on the
    /// second crash, and a changed tool list is listed afresh. A connector's session is renewed by its own calls.
    async fn settle(&mut self, index: usize, backend: Backend) -> Settled {
        let Backend::Stdio(server) = backend else {
            return Settled::Keep;
        };
        if server.exited_now().await {
            self.groups.remove(server.process_group());
            let slot = &mut self.slots[index];
            slot.server = None;
            slot.crashes += 1;
            if slot.crashes >= MAX_CRASHES {
                slot.disabled = true;
                return Settled::Disable("crashed");
            }
            return Settled::Start(McpServerState::Restarted);
        }
        if !server.is_stale() {
            return Settled::Keep;
        }
        drop(server);
        let listed_before = self.slots[index].server.take();
        self.retire(listed_before).await;
        Settled::Start(McpServerState::Ready)
    }

    /// Stops every server, at session end. Only tests call it.
    #[cfg(feature = "testing")]
    pub async fn stop_all(&mut self) {
        for slot in std::mem::take(&mut self.slots) {
            self.retire(slot.server).await;
        }
    }

    async fn retire(&self, backend: Option<Backend>) {
        match backend {
            None => {}
            Some(Backend::Gateway(server)) => server.close().await,
            Some(Backend::Stdio(server)) => {
                self.groups.remove(server.process_group());
                match Arc::try_unwrap(server) {
                    Ok(server) => {
                        server.stop().await;
                    }
                    // A tool of a finished run still holds it: end the group now; the last drop reaps the child.
                    Err(server) => server.kill(),
                }
            }
        }
    }
}

/// Registers a server's tools for the run: the count registered, and whether any tool was refused by a bound.
fn register_tools(
    backend: &Backend,
    config: &McpServer,
    registry: &mut ToolRegistry,
) -> (u32, bool) {
    let listed: &[ListedTool] = match backend {
        Backend::Stdio(server) => server.tools(),
        Backend::Gateway(server) => server.tools(),
    };
    let (mut registered, mut skipped) = (0_u32, false);
    // A tool the server's own list excludes is intended, not skipped.
    for tool in listed
        .iter()
        .filter(|listed| super::tool::allowed(config.tools.as_ref(), &listed.name))
    {
        let tool = McpTool::new(backend.clone(), config, tool);
        match tool.map(|tool| registry.register_mcp(Arc::new(tool))) {
            Some(Ok(())) => registered += 1,
            _ => skipped = true,
        }
    }
    (registered, skipped)
}

fn event(config: &McpServer, state: McpServerState, tools: u32, reason: Option<String>) -> Event {
    Event::McpServer {
        name: config.name.clone(),
        state,
        tools,
        reason,
    }
}
