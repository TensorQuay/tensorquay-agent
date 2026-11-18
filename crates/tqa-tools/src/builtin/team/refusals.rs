//! A run that may not lead (ADR-0034's rounds amendment, A6 and A7): the role's whole answer, the sentence for each
//! reason, and the team tools it knows by name but refuses in words, never as an unknown tool.
use super::client::TeamClient;
use super::done::AcceptanceShell;
use super::tools::{TeamRole, tools_with_check};
use crate::{PreparedAction, Tool, ToolContext, ToolError, ToolOutput, ToolSpec};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

/// `GET /runtime/teams/role`'s whole answer: the role, and why a run may not lead (ADR-0034's rounds amendment, A2).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoleAnswer {
    /// Which team tools the run is offered.
    pub role: TeamRole,
    /// Why it may not lead (`none` and `ended`), for the sentence a team tool's call is answered with.
    pub reason: Option<String>,
}

impl RoleAnswer {
    /// The answer the gateway gave.
    #[must_use]
    pub fn parse(role: &str, reason: Option<&str>) -> Self {
        Self {
            role: TeamRole::parse(role),
            reason: reason.map(str::to_owned),
        }
    }
}

/// The team tools a run with `answer` gets (ADR-0034's rounds amendment, A6 and A7). A run that may not lead
/// (`none`) still knows the twelve team tool names, offers none of them, and answers a call to any with its
/// reason's sentence; between rounds (`ended`) `read_reports` works and the other eleven say why. Nothing it refuses
/// reaches the gateway, and its model's tool list is as before.
pub fn tools_for_answer(
    client: Arc<dyn TeamClient>,
    answer: &RoleAnswer,
    check: Option<AcceptanceShell>,
) -> Vec<Arc<dyn Tool>> {
    match answer.role {
        TeamRole::None => refused_all(&client, sentence(answer.reason.as_deref()), &[]),
        TeamRole::Ended => between_rounds(&client, check),
        role => tools_with_check(client, role, check),
    }
}

/// A lead between rounds (A2): `read_reports` and `start_member` offered; the other lead tools known and answering
/// `between_rounds`' sentence until `start_member` opens the next round, then working as a lead's; the helper's two
/// refused in the same words.
fn between_rounds(
    client: &Arc<dyn TeamClient>,
    check: Option<AcceptanceShell>,
) -> Vec<Arc<dyn Tool>> {
    let sentence = sentence(Some("between_rounds"));
    let mut tools: Vec<Arc<dyn Tool>> = tools_with_check(client.clone(), TeamRole::MayLead, check)
        .into_iter()
        .map(|tool| match tool.spec().name.as_str() {
            "start_member" => tool,
            "read_reports" => Arc::new(AlwaysOffered(tool)) as Arc<dyn Tool>,
            _ => Arc::new(UntilRound {
                inner: tool,
                sentence,
            }) as Arc<dyn Tool>,
        })
        .collect();
    tools.extend(
        tools_with_check(client.clone(), TeamRole::Member, None)
            .into_iter()
            .map(|tool| {
                Arc::new(RefusedInWords {
                    spec: tool.spec().clone(),
                    sentence,
                }) as Arc<dyn Tool>
            }),
    );
    tools
}

/// A tool offered whatever the run's state: a lead between rounds reads its end note before any round opens.
struct AlwaysOffered(Arc<dyn Tool>);

#[async_trait]
impl Tool for AlwaysOffered {
    fn spec(&self) -> &ToolSpec {
        self.0.spec()
    }
    fn null_means_absent(&self) -> bool {
        self.0.null_means_absent()
    }
    async fn call(&self, args: Value, cx: &ToolContext) -> Result<ToolOutput, ToolError> {
        self.0.call(args, cx).await
    }
    async fn call_prepared(
        &self,
        action: PreparedAction,
        cx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        self.0.call_prepared(action, cx).await
    }
}

/// A lead tool between rounds: answered in words until a round opens (the tool is offered again), then the tool.
struct UntilRound {
    inner: Arc<dyn Tool>,
    sentence: &'static str,
}

#[async_trait]
impl Tool for UntilRound {
    fn spec(&self) -> &ToolSpec {
        self.inner.spec()
    }
    fn null_means_absent(&self) -> bool {
        self.inner.null_means_absent()
    }
    fn offered(&self) -> bool {
        self.inner.offered()
    }
    async fn call(&self, args: Value, cx: &ToolContext) -> Result<ToolOutput, ToolError> {
        if self.inner.offered() {
            self.inner.call(args, cx).await
        } else {
            Err(ToolError::channel("teams_unavailable", self.sentence))
        }
    }
    async fn call_prepared(
        &self,
        action: PreparedAction,
        cx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        if self.inner.offered() {
            self.inner.call_prepared(action, cx).await
        } else {
            Err(ToolError::channel("teams_unavailable", self.sentence))
        }
    }
}

/// What a team tool's call is answered with when the run may not lead, for the role's `reason` (A6 and A7); a
/// missing or unknown reason reads as switched off.
#[must_use]
pub fn sentence(reason: Option<&str>) -> &'static str {
    match reason {
        Some("was_member") => {
            "This task was a helper, and a helper can't lead a team. Do the work yourself, and say so."
        }
        Some("runtime_too_old") => {
            "This environment can't lead helpers (its runtime is too old). Do the work yourself, and say so; the \
             person can start the task in a newer environment."
        }
        Some("team_ended") => {
            "This team has ended for good (its lead task was deleted or its person's account was turned off). Do \
             the work yourself, and say so."
        }
        Some("between_rounds") => {
            "Your team's round has ended. Read its end note with read_reports, or start the next round with \
             start_member; the other team tools come back once a round runs."
        }
        _ => {
            "Helpers are switched off for this installation. Do the work yourself, and say in your answer that you \
             worked without helpers."
        }
    }
}

/// Every team tool name but `except`, each answering `sentence`: the lead's ten and the helper's two.
fn refused_all(
    client: &Arc<dyn TeamClient>,
    sentence: &'static str,
    except: &[&str],
) -> Vec<Arc<dyn Tool>> {
    tools_with_check(client.clone(), TeamRole::Lead, None)
        .into_iter()
        .chain(tools_with_check(client.clone(), TeamRole::Member, None))
        .filter(|tool| !except.contains(&tool.spec().name.as_str()))
        .map(|tool| {
            Arc::new(RefusedInWords {
                spec: tool.spec().clone(),
                sentence,
            }) as Arc<dyn Tool>
        })
        .collect()
}

/// A team tool the run isn't offered, known by name so a call to it is answered in words (`teams_unavailable`) and
/// never as an unknown tool (A6). It never calls the gateway.
struct RefusedInWords {
    spec: ToolSpec,
    sentence: &'static str,
}

#[async_trait]
impl Tool for RefusedInWords {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn null_means_absent(&self) -> bool {
        true
    }
    fn offered(&self) -> bool {
        false
    }
    async fn call(&self, _args: Value, _cx: &ToolContext) -> Result<ToolOutput, ToolError> {
        Err(ToolError::channel("teams_unavailable", self.sentence))
    }
}
