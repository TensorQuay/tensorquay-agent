//! Agent-team roles (ADR-0034 point 2, the slice S1 contract): which role may call which route, whom a post is for,
//! and who reads what. The role comes from the stored rows, never from the request; pure (GW-U-069).
use crate::error::GatewayError;
use tqa_types::{TeamMessageType, TeamPost};

/// A conversation's role in its agent team.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Role {
    /// The lead: starts members, assigns, asks, decides.
    Lead,
    /// A member: writes only to its lead.
    Member,
}

/// An agent-team route, by what it does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Route {
    /// `start_member`.
    StartMember,
    /// A post of this type.
    Post(TeamMessageType),
    /// A read of the caller's own messages (`read_reports`, `wait_for_lead`).
    Read,
    /// `take_files`.
    TakeFiles,
    /// `stop_member`.
    StopMember,
    /// `done`.
    Done,
    /// `read_helper`.
    ReadHelper,
}

/// Whom a message is for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Addressee {
    /// The lead.
    Lead,
    /// One member, by name.
    Member(String),
    /// Every member.
    Members,
    /// The lead and every member.
    Team,
    /// The person the team works for.
    Person,
}

/// Whether a caller with `role` (`None`: in no agent team) may call `route`.
///
/// # Errors
/// `not_a_member` for a conversation in no team (it may only start one), `not_lead` for a member on a lead route,
/// and `members_only` for the lead on a member's post. A post of a type only the gateway or a person writes is
/// `invalid_request` for every agent.
pub(crate) fn allowed(role: Option<Role>, route: Route) -> Result<(), GatewayError> {
    use TeamMessageType as T;
    let lead_only = matches!(
        route,
        Route::StartMember
            | Route::TakeFiles
            | Route::StopMember
            | Route::Done
            | Route::ReadHelper
            | Route::Post(T::Assign | T::Question | T::Decision | T::PersonQuestion)
    );
    let member_only = matches!(route, Route::Post(T::Answer | T::Report | T::Ask));
    let nobody = matches!(
        route,
        Route::Post(T::PersonAnswer | T::PersonMessage | T::Notice | T::Done)
    );
    match role {
        // `TeamPost` doesn't decode these types, so a request never names one; refused all the same.
        _ if nobody => Err(GatewayError::BadRequest(
            "the gateway or a person writes this message type",
        )),
        None if route == Route::StartMember => Ok(()),
        None => Err(GatewayError::Denied("not_a_member")),
        Some(Role::Member) if lead_only => Err(GatewayError::Denied("not_lead")),
        Some(Role::Lead) if member_only => Err(GatewayError::Denied("members_only")),
        Some(_) => Ok(()),
    }
}

/// Whom a post is for: an `assign` its named member, a question or decision every member, the lead's question the
/// person, and a member's post the lead.
pub(crate) fn addressee(post: &TeamPost) -> Addressee {
    match post {
        TeamPost::Assign { to, .. } => Addressee::Member(to.clone()),
        TeamPost::Question { .. } | TeamPost::Decision { .. } => Addressee::Members,
        TeamPost::PersonQuestion { .. } => Addressee::Person,
        TeamPost::Answer { .. } | TeamPost::Report { .. } | TeamPost::Ask { .. } => Addressee::Lead,
    }
}

/// Whether a reader with `role` and `name` reads a message for `to`: a member what is for it, for every member or for
/// the team; the lead what is for the lead or the team. Nobody reads what is for the person: people read through the
/// human API.
pub(crate) fn reads(role: Role, name: &str, to: &Addressee) -> bool {
    match (role, to) {
        (Role::Member, Addressee::Member(member)) => member == name,
        (Role::Member, Addressee::Members | Addressee::Team)
        | (Role::Lead, Addressee::Lead | Addressee::Team) => true,
        _ => false,
    }
}

#[cfg(test)]
#[path = "roles_tests.rs"]
mod tests;
