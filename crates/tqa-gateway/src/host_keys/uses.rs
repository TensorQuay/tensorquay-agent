//! A host key's last use, from its read audit rows (ADR-0037 §8, the plan §1.1): what each viewer may see of it. A member
//! sees their own use only, with the task's title for their own task; an administrator sees a shared key's last use and
//! who made it; of a member's own key, only when (least exposure, Amendment 1).
use super::api::Row;
use crate::GatewayError;
use serde_json::{Value, json};
use sqlx::PgConnection;
use std::collections::HashMap;
use uuid::Uuid;

/// Whose view a last use is shaped for.
#[derive(Clone, Copy)]
pub(super) enum Viewer {
    /// A member: their own use of their own or a shared key.
    Member(Uuid),
    /// An administrator looking at shared keys: any person's use, and who.
    AdminShared,
    /// An administrator looking at members' own keys: when, and nothing more.
    AdminPersonal,
}

/// The last use of each of `rows`' keys that `viewer` may see, by the key's sealed reference.
///
/// # Errors
/// A store failure.
pub(super) async fn last(
    connection: &mut PgConnection,
    rows: &[Row],
    viewer: Viewer,
) -> Result<HashMap<String, Value>, GatewayError> {
    let references: Vec<String> = rows.iter().map(|row| row.secret_ref.clone()).collect();
    last_by_ref(connection, &references, viewer).await
}

/// [`last`], for the keys with these sealed references.
///
/// # Errors
/// A store failure.
pub(super) async fn last_by_ref(
    connection: &mut PgConnection,
    references: &[String],
    viewer: Viewer,
) -> Result<HashMap<String, Value>, GatewayError> {
    if references.is_empty() {
        return Ok(HashMap::new());
    }
    let member = match viewer {
        Viewer::Member(user) => Some(user),
        Viewer::AdminShared | Viewer::AdminPersonal => None,
    };
    let latest = sqlx::query!(
        "SELECT DISTINCT ON (r.ref) r.ref, r.last_at, r.count, r.outcome, r.conversation_id, r.user_id, u.email AS \
         \"email?\", c.title AS \"title?\", c.owner_id AS \"conversation_owner?\" FROM secret_reads r LEFT JOIN users u \
         ON u.id=r.user_id LEFT JOIN conversations c ON c.id=r.conversation_id WHERE r.consumer_kind='host' AND \
         r.ref=ANY($1) AND ($2::uuid IS NULL OR r.user_id=$2) ORDER BY r.ref, r.last_at DESC, r.id DESC",
        references,
        member
    )
    .fetch_all(&mut *connection)
    .await?;
    Ok(latest
        .into_iter()
        .map(|read| {
            let at = read
                .last_at
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default();
            let shaped = match viewer {
                Viewer::AdminPersonal => json!({"at": at}),
                Viewer::Member(user) => {
                    // A task's title only for a task the viewer may open: their own.
                    let own_task = read.conversation_owner == Some(user);
                    json!({"at": at, "count": read.count, "outcome": read.outcome,
                        "conversation_id": read.conversation_id.filter(|_| own_task),
                        "task_title": read.title.filter(|_| own_task), "user": null})
                }
                Viewer::AdminShared => {
                    json!({"at": at, "count": read.count, "outcome": read.outcome,
                    "conversation_id": read.conversation_id, "task_title": null,
                    "user": read.user_id.map(|id| json!({"id": id, "email": read.email}))})
                }
            };
            (read.r#ref, shaped)
        })
        .collect())
}
