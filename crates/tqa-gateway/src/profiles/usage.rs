//! Making a conversation from a profile (ADR-0012 decision 5): the profile, checked for use now, and the request fields
//! it sets.
use sqlx::PgConnection;
use uuid::Uuid;

use super::{References, document::Profile};
use crate::{GatewayError, app::State};

/// The profile a conversation is made from, at the version read.
pub(crate) struct Chosen {
    pub id: Uuid,
    pub version: i64,
    pub profile: Profile,
}

/// The caller's own profile or a shared one, usable now.
///
/// # Errors
/// `422 unknown_reference` (`profile`) for one the caller may not see, so another user's is indistinguishable from
/// none; `409 profile_unusable` with the first field the install lacks, disables or does not offer; storage failures.
pub(crate) async fn chosen(
    connection: &mut PgConnection,
    state: &State,
    owner: Uuid,
    id: Uuid,
) -> Result<Chosen, GatewayError> {
    let record = sqlx::query!(
        "SELECT id,document,version FROM agent_profiles WHERE id=$1 AND (owner_id=$2 OR owner_id IS NULL)",
        id,
        owner
    )
    .fetch_optional(connection)
    .await?
    .ok_or(GatewayError::UnknownReference { field: "profile" })?;
    let profile: Profile = serde_json::from_value(record.document)?;
    let snapshot = state.settings.current();
    if let Some(field) = super::unusable(
        &snapshot,
        state.config.mcp.servers(),
        &References::of(&profile),
    ) {
        return Err(GatewayError::ProfileUnusable { field });
    }
    Ok(Chosen {
        id: record.id,
        version: record.version,
        profile,
    })
}

/// A request field the profile may set: the profile's value when it sets one (the request may repeat it), else the
/// request's.
///
/// # Errors
/// `422 set_by_profile` for a request naming another value.
pub(crate) fn set<'a>(
    field: &'static str,
    requested: Option<&'a str>,
    by_profile: Option<&'a str>,
) -> Result<Option<&'a str>, GatewayError> {
    match (requested, by_profile) {
        (Some(requested), Some(set)) if requested != set => {
            Err(GatewayError::SetByProfile { field })
        }
        (requested, set) => Ok(set.or(requested)),
    }
}
