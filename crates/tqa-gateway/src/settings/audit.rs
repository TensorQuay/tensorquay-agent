//! One `admin_audit` row per settings change, by an administrator or a seed import (ADR-0028), and per change a user
//! makes to their own credentials (ADR-0033): the area, the item, the action and the versions. Never a
//! value, and never a hash of a secret value.
use crate::GatewayError;
use sqlx::PgConnection;

/// One change to record.
pub(crate) struct Change<'a> {
    /// `secrets`, `routes`, `egress` or `limits`.
    pub area: &'static str,
    /// The administrator (or, for `audit_user`, the user) who made it; `None` for a seed import.
    pub actor: Option<uuid::Uuid>,
    /// The item's key within its area, for example a secret reference or a route name.
    pub item: &'a str,
    /// `create`, `replace` or `delete`.
    pub action: &'static str,
    pub before: Option<i64>,
    pub after: Option<i64>,
    /// Whether the stored value changed (a replace can write the same value).
    pub value_changed: bool,
}

/// Records `change` in the caller's transaction.
///
/// # Errors
/// Storage failure.
pub(crate) async fn audit(
    connection: &mut PgConnection,
    change: &Change<'_>,
) -> Result<(), GatewayError> {
    let kind = if change.actor.is_some() {
        "admin"
    } else {
        "seed"
    };
    insert(connection, kind, change).await
}

/// Records a built-in environment's registration or update (the built-in environments design), in the caller's
/// transaction: the gateway did it from the images on the engine host, not an administrator or the file.
///
/// # Errors
/// Storage failure.
pub(crate) async fn audit_builtin(
    connection: &mut PgConnection,
    change: &Change<'_>,
) -> Result<(), GatewayError> {
    insert(connection, "builtin", change).await
}

/// Records a change `user` made to their own credential, in the caller's transaction: `area` is `git_credentials`,
/// `connector_credentials`, `channels` or `account`; `item` the credential's opaque store reference, or `password`
/// for the account's own (never a remote, a URL or a value). A task or workspace delete is `tasks` or `workspaces`,
/// `item` its id.
///
/// # Errors
/// Storage failure.
pub(crate) async fn audit_user(
    connection: &mut PgConnection,
    user: uuid::Uuid,
    area: &'static str,
    item: &str,
    action: &'static str,
) -> Result<(), GatewayError> {
    let change = Change {
        area,
        actor: Some(user),
        item,
        action,
        before: None,
        after: None,
        // A set always stores the value it was given; a delete stores none.
        value_changed: action != "delete",
    };
    insert(connection, "user", &change).await
}

/// Records something the gateway did on its own, by a rule an administrator set (a workspace deleted by retention), in
/// the caller's transaction: actor `system`, no user.
///
/// # Errors
/// Storage failure.
pub(crate) async fn audit_system(
    connection: &mut PgConnection,
    area: &'static str,
    item: &str,
    action: &'static str,
) -> Result<(), GatewayError> {
    let change = Change {
        area,
        actor: None,
        item,
        action,
        before: None,
        after: None,
        value_changed: false,
    };
    insert(connection, "system", &change).await
}

/// Records an administrator's or a seed import's change with a bounded `detail`, `<old>-><new>` of a closed setting
/// such as an engine's exposure or budget, in the caller's transaction.
///
/// # Errors
/// Storage failure.
pub(crate) async fn audit_detail(
    connection: &mut PgConnection,
    change: &Change<'_>,
    detail: &str,
) -> Result<(), GatewayError> {
    let kind = if change.actor.is_some() {
        "admin"
    } else {
        "seed"
    };
    write(connection, kind, change, Some(detail)).await
}

async fn insert(
    connection: &mut PgConnection,
    kind: &str,
    change: &Change<'_>,
) -> Result<(), GatewayError> {
    write(connection, kind, change, None).await
}

async fn write(
    connection: &mut PgConnection,
    kind: &str,
    change: &Change<'_>,
    detail: Option<&str>,
) -> Result<(), GatewayError> {
    sqlx::query!(
        "INSERT INTO admin_audit (id,actor_kind,actor_user_id,area,item,action,before_version,after_version,value_hash_changed,detail) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
        uuid::Uuid::now_v7(), kind, change.actor, change.area, change.item, change.action, change.before, change.after,
        change.value_changed, detail
    )
    .execute(connection)
    .await?;
    Ok(())
}
