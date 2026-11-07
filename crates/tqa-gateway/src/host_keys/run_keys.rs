//! Which keys a run gets (ADR-0037 §2, R1, R5, R6): its sender's own keys and the shared keys for them, that the
//! task's network list allows, and for a spawned agent at most its caller's.
use crate::egress::{AllowList, Decision};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// A key as a run holds it: where and how it goes, never its value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RunKey {
    pub(crate) id: Uuid,
    /// The environment variable the task sees it by.
    pub(crate) name: String,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) path_prefix: Option<String>,
    pub(crate) scheme: String,
    pub(crate) user_name: Option<String>,
    /// A shared key rather than the sender's own.
    pub(crate) shared: bool,
}

/// The keys a run starts with, by name. `candidates` are the sender's own keys and the shared keys whose people
/// include the sender, as the store holds them now (a deleted key is not among them); `sender_active` is false once
/// the sender is disabled; `caller` is a spawned agent's caller's keys. A name held twice, which the store refuses
/// but a race could let through, goes to the sender's own key, so a variable is set once.
pub(crate) fn choose(
    sender_active: bool,
    candidates: Vec<RunKey>,
    network: &AllowList,
    egress_disabled: bool,
    caller: Option<&BTreeSet<Uuid>>,
) -> Vec<RunKey> {
    if !sender_active || egress_disabled {
        return Vec::new();
    }
    let mut by_name = BTreeMap::<String, RunKey>::new();
    for key in candidates {
        let allowed = matches!(
            network.decide(&key.host, key.port),
            Decision::Allowed { .. }
        );
        if !allowed || caller.is_some_and(|keys| !keys.contains(&key.id)) {
            continue;
        }
        match by_name.get(&key.name) {
            Some(held) if !held.shared || key.shared => {}
            _ => {
                by_name.insert(key.name.clone(), key);
            }
        }
    }
    by_name.into_values().collect()
}

/// The sender's own keys and the shared keys for them, as the store holds them now, and whether the sender is active.
///
/// # Errors
/// Storage failure.
pub(crate) async fn load(
    connection: &mut sqlx::PgConnection,
    sender: Uuid,
) -> Result<(bool, Vec<RunKey>), crate::GatewayError> {
    let active = sqlx::query_scalar!(
        "SELECT disabled_at IS NULL AS \"active!\" FROM users WHERE id=$1",
        sender
    )
    .fetch_optional(&mut *connection)
    .await?
    .unwrap_or(false);
    let rows = sqlx::query!(
        "SELECT id, name, host, port, path_prefix, scheme, user_name, owner_id IS NULL AS \"shared!\" \
         FROM host_keys k WHERE scheme IN ('bearer', 'basic') AND (owner_id=$1 OR (owner_id IS NULL AND (for_everyone \
         OR EXISTS (SELECT 1 FROM host_key_users u WHERE u.key_id=k.id AND u.user_id=$1)))) ORDER BY name, id",
        sender
    )
    .fetch_all(connection)
    .await?;
    let keys = rows
        .into_iter()
        .map(|row| {
            Ok(RunKey {
                id: row.id,
                name: row.name,
                host: row.host,
                port: u16::try_from(row.port).map_err(|_| crate::GatewayError::StoreUnavailable)?,
                path_prefix: row.path_prefix,
                scheme: row.scheme,
                user_name: row.user_name,
                shared: row.shared,
            })
        })
        .collect::<Result<_, crate::GatewayError>>()?;
    Ok((active, keys))
}

#[cfg(test)]
#[path = "run_keys_tests.rs"]
mod tests;
