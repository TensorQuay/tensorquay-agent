//! A run that may not lead (ADR-0034's rounds amendment, A6 and A7): the role's whole answer, the sentence for each
//! reason, and the team tools it knows by name but refuses in words, never as an unknown tool.
use super::client::TeamClient;
use super::done::AcceptanceShell;
use super::tools::{TeamRole, tools_with_check};
use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolSpec};
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
        TeamRole::Ended => {
            // Step 1: no round can open yet, so the tools a round brings back say one_team's sentence (A7).
            let mut tools = tools_with_check(client.clone(), TeamRole::Ended, check);
            tools.extend(refused_all(
                &client,
                sentence(Some("one_team")),
                &["read_reports"],
            ));
            tools
        }
        role => tools_with_check(client, role, check),
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
        Some("one_team") => {
            "This task's team has ended, and for now a task leads one team. Do the work yourself, and say so; to \
             lead helpers again, the person can start a new task."
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
