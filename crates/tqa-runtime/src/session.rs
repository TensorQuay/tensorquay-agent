//! Session snapshots cloned into each run; refreshed routes apply to future runs.
use crate::RuntimeError;
use tqa_types::{
    ClientFrame, ConversationId, DecisionEngineSnapshot, Environment, McpServer, NetworkSnapshot,
    ProfileSnapshot, RouteSnapshot, RunLimits, Seq, WorkspaceSnapshot,
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Session {
    pub conversation: ConversationId,
    pub route: RouteSnapshot,
    /// The task's network allow-list for the prompt (ADR-0035 §3); a changed list applies from the next registration.
    pub network: NetworkSnapshot,
    pub limits: RunLimits,
    pub workspace: WorkspaceSnapshot,
    pub environment: Environment,
    pub protocol: u16,
    /// Admin-declared fallback routes, adopted with the route on `session.update`.
    pub fallback_routes: Vec<RouteSnapshot>,
    /// MCP servers; a changed list applies at the next run (ADR-0011).
    pub mcp: Vec<McpServer>,
    /// The agent profile the runs apply: instructions, a built-in tool allow-list and a skills filter (ADR-0012).
    pub profile: Option<ProfileSnapshot>,
    /// The decision engine the runs consult over the relay; none means the model decides (ADR-0013, revision 9).
    pub decision_engine: Option<DecisionEngineSnapshot>,
    /// The gateway's announced protocol revision; absent from gateways older than revision 9. Not frozen: a gateway
    /// upgraded between registrations announces its new revision.
    pub gateway_revision: Option<u16>,
    /// The install CA the sandbox trusts for keyed hosts, PEM (revision 11, ADR-0037 §6); none from a gateway without
    /// one. Not frozen: a rotation reaches the next registration.
    pub host_ca: Option<String>,
    /// The keys of a run in progress, minted for this sandbox, which that run gets when it resumes (revision 11,
    /// ADR-0037 §3). Not frozen: every registration brings its own.
    pub run_host_keys: Vec<tqa_types::HostKeyGrant>,
    /// The `computer` tool's per-run caps (revision 14, ADR-0039 section 4); the defaults when the gateway sent none.
    /// Not frozen: an administrator's change reaches the next registration.
    pub computer_limits: tqa_types::ComputerLimits,
}
impl Session {
    pub fn parse(frame: ClientFrame) -> Result<(Self, Seq), RuntimeError> {
        let ClientFrame::SessionConfig {
            conversation_id,
            last_stored_seq,
            route,
            network,
            limits,
            workspace,
            environment,
            protocol,
            fallback_routes,
            mcp,
            profile,
            decision_engine,
            gateway_revision,
            host_ca,
            run_host_keys,
            computer_limits,
            ..
        } = frame
        else {
            return Err(RuntimeError::Frame);
        };
        Ok((
            Self {
                conversation: conversation_id,
                route,
                network,
                limits,
                workspace,
                environment,
                protocol,
                fallback_routes,
                mcp,
                profile,
                decision_engine,
                gateway_revision,
                host_ca,
                run_host_keys,
                computer_limits: computer_limits.unwrap_or_default(),
            },
            last_stored_seq,
        ))
    }
    pub fn compare_frozen(&self, next: &Self) -> Result<(), RuntimeError> {
        for (changed, field) in [
            (self.conversation != next.conversation, "conversation_id"),
            (self.protocol != next.protocol, "protocol"),
            (self.limits != next.limits, "limits"),
            (self.workspace != next.workspace, "workspace"),
            (self.environment != next.environment, "environment"),
            (self.profile != next.profile, "profile"),
            (
                self.decision_engine != next.decision_engine,
                "decision_engine",
            ),
        ] {
            if changed {
                return Err(RuntimeError::SnapshotChanged(field));
            }
        }
        Ok(())
    }
}
