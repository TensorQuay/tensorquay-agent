//! Built-in environments in the database (the built-in environments design): the environments the install built,
//! registered from the images on the engine host as `last_set_by = 'builtin'`. The file's `[[templates]]` entry of the
//! same name wins, an administrator's environment keeps its name, and a built-in an administrator deleted stays deleted
//! by name. A built-in follows the image its canonical tag names: a moved tag is an audited update, never a silent one.
use super::{Change, audit::audit_detail, audit_builtin, templates::AdminTemplate};
use crate::{
    GatewayError,
    config::{TemplateInput, validate},
    environments::builtin::BuiltIn,
};
use serde_json::json;
use sqlx::PgConnection;

/// Registers or updates the built-ins found on the host, in the caller's settings transaction; `file` names the file's
/// templates (they win).
/// Returns whether it wrote. A built-in no longer on the host keeps its row: tasks may use it. One the database
/// refuses (a check constraint) is logged as an error naming it and left out, in its own savepoint, so the others
/// still register (GW-I-159).
///
/// # Errors
/// Storage failure.
pub(crate) async fn register(
    connection: &mut PgConnection,
    found: &[BuiltIn],
    file: &[&str],
) -> Result<bool, GatewayError> {
    let mut wrote = false;
    for built in found {
        if file.contains(&built.name) {
            continue;
        }
        if deleted(connection, built.name).await? {
            wrote |= keep_deleted_build(connection, built).await?;
            continue;
        }
        if !valid(built) {
            continue;
        }
        let mut savepoint = sqlx::Connection::begin(&mut *connection).await?;
        match register_one(&mut savepoint, built).await {
            Ok(stored) => {
                savepoint.commit().await?;
                wrote |= stored;
            }
            Err(GatewayError::Conflict(REFUSED)) => savepoint.rollback().await?,
            Err(error) => return Err(error),
        }
    }
    Ok(wrote)
}

/// Stands for a write of `built` the database refused by a check constraint, already logged by [`refused`].
const REFUSED: &str = "builtin_refused";

/// Logs a write the database refused as what it is, naming the environment and the constraint (never
/// `store_unavailable`), and returns [`REFUSED`]; any other failure is a storage failure.
fn refused(name: &str, error: &sqlx::Error) -> GatewayError {
    let Some(database) = error.as_database_error() else {
        return GatewayError::StoreUnavailable;
    };
    if database.code().as_deref() != Some("23514") {
        return GatewayError::StoreUnavailable;
    }
    tracing::error!(
        template = name,
        constraint = database.constraint().unwrap_or("unnamed"),
        "built-in environment not registered: the database refused it; the others are registered"
    );
    GatewayError::Conflict(REFUSED)
}

/// Registers or updates one built-in; returns whether it wrote.
async fn register_one(
    connection: &mut PgConnection,
    built: &BuiltIn,
) -> Result<bool, GatewayError> {
    let capabilities: Vec<String> = built
        .capabilities
        .iter()
        .map(|capability| capability.as_str().to_owned())
        .collect();
    let created = time::OffsetDateTime::from_unix_timestamp(built.created)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    let current = sqlx::query!(
            "SELECT version,last_set_by,image,display_name,capabilities FROM templates WHERE name=$1 FOR UPDATE",
            built.name
        )
        .fetch_optional(&mut *connection)
        .await?;
    let Some(current) = current else {
        return create(connection, built, (&capabilities, created)).await;
    };
    if current.last_set_by != "builtin" {
        return adopt_base(connection, built).await;
    }
    if current.image == built.image
        && current.display_name == built.display_name
        && current.capabilities == capabilities
    {
        return Ok(false);
    }
    let version = current.version + 1;
    // A new image needs checking again, as an administrator's new image does; the tasks in it move at their next
    // session (ADR-0032 decision 7).
    sqlx::query!(
            "UPDATE templates SET display_name=$2,image=$3,capabilities=$4,version=$5,builtin_image_created=$6,\
             builtin_updated_at=CURRENT_TIMESTAMP,check_state='unchecked',check_reason=NULL,checked_version=NULL,\
             base=NULL,image_id=NULL,updated_at=CURRENT_TIMESTAMP WHERE name=$1",
            built.name,
            built.display_name,
            built.image,
            &capabilities,
            version,
            created
        )
        .execute(&mut *connection)
        .await
        .map_err(|error| refused(built.name, &error))?;
    audit_builtin(
        connection,
        &Change {
            area: "templates",
            actor: None,
            item: built.name,
            action: "replace",
            before: Some(current.version),
            after: Some(version),
            value_changed: true,
        },
    )
    .await?;
    tracing::info!(
        template = built.name,
        version,
        "a built-in environment moved to a newer build of its image"
    );
    Ok(true)
}

/// A built-in customised before its base was kept (16i's Desktop, refused for its entrypoint, GW-I-278) takes `built`
/// as its base while it is still on that image, and is checked again; its settings stay as the administrator set them.
/// Returns whether it wrote.
async fn adopt_base(connection: &mut PgConnection, built: &BuiltIn) -> Result<bool, GatewayError> {
    let adopted = sqlx::query!(
        "UPDATE templates SET builtin_base_image=$2,check_state='unchecked',check_reason=NULL,checked_version=NULL,\
         base=NULL,image_id=NULL,updated_at=CURRENT_TIMESTAMP \
         WHERE name=$1 AND customised_builtin AND builtin_base_image IS NULL AND image=$2",
        built.name,
        built.image
    )
    .execute(&mut *connection)
    .await
    .map_err(|error| refused(built.name, &error))?
    .rows_affected()
        > 0;
    if adopted {
        tracing::info!(
            template = built.name,
            "a customised built-in environment takes its build as its base and is checked again"
        );
    }
    Ok(adopted)
}

/// Hands a customised built-in back to the rebuilds, at version `expected`, in the caller's settings transaction: it
/// takes `built` (the image its canonical tag names now, or why the engine couldn't say), with that image's name and
/// tools, and is checked again. The template's own refusals come first. Its
/// size, description and enabled switch stay. Audited as the administrator's replace, with `admin->builtin` as detail.
///
/// # Errors
/// `404`; `409` `conflict`, `not_customised`, or `template_is_default` when the default's image or tools would change
/// (as for an edit); the engine's own error from `built`; storage failure.
pub(crate) async fn reset(
    connection: &mut PgConnection,
    (name, expected): (&str, i64),
    built: Result<BuiltIn, GatewayError>,
    actor: uuid::Uuid,
) -> Result<AdminTemplate, GatewayError> {
    let current = sqlx::query!(
        "SELECT version,customised_builtin,is_default,image,capabilities FROM templates WHERE name=$1 FOR UPDATE",
        name
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(GatewayError::NotFound)?;
    if current.version != expected {
        return Err(GatewayError::Conflict("conflict"));
    }
    if !current.customised_builtin {
        return Err(GatewayError::Conflict("not_customised"));
    }
    let built = built?;
    let capabilities: Vec<String> = built
        .capabilities
        .iter()
        .map(|capability| capability.as_str().to_owned())
        .collect();
    if current.is_default && (current.image != built.image || current.capabilities != capabilities)
    {
        return Err(GatewayError::Conflict("template_is_default"));
    }
    let version = current.version + 1;
    let created = time::OffsetDateTime::from_unix_timestamp(built.created)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    sqlx::query!(
        "UPDATE templates SET display_name=$2,image=$3,capabilities=$4,version=$5,last_set_by='builtin',\
         customised_builtin=false,builtin_base_image=NULL,builtin_image_created=$6,builtin_updated_at=CURRENT_TIMESTAMP,check_state='unchecked',\
         check_reason=NULL,checked_version=NULL,base=NULL,image_id=NULL,updated_at=CURRENT_TIMESTAMP WHERE name=$1",
        name,
        built.display_name,
        built.image,
        &capabilities,
        version,
        created
    )
    .execute(&mut *connection)
    .await?;
    audit_detail(
        connection,
        &Change {
            area: "templates",
            actor: Some(actor),
            item: name,
            action: "replace",
            before: Some(expected),
            after: Some(version),
            value_changed: true,
        },
        "admin->builtin",
    )
    .await?;
    tracing::info!(
        template = name,
        version,
        "a customised built-in environment was reset to the built-in"
    );
    super::templates::entry(connection, name).await
}

/// Records that an administrator deleted the built-in `name`, in the caller's transaction.
///
/// # Errors
/// Storage failure.
pub(crate) async fn bury(
    connection: &mut PgConnection,
    name: &str,
    build: &str,
) -> Result<(), GatewayError> {
    sqlx::query!(
        "INSERT INTO builtin_deletions (name,image) VALUES ($1,$2) ON CONFLICT (name) DO UPDATE SET deleted_at=CURRENT_TIMESTAMP,image=EXCLUDED.image",
        name,
        build
    )
    .execute(connection)
    .await?;
    Ok(())
}

async fn deleted(connection: &mut PgConnection, name: &str) -> Result<bool, GatewayError> {
    Ok(sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM builtin_deletions WHERE name=$1) AS \"deleted!\"",
        name
    )
    .fetch_one(connection)
    .await?)
}

/// A deleted built-in's current build stays one of the checks' bases (GW-I-279): an administrator adds Desktop again
/// from its image, and Desktop, built FROM Workstation with its own init, passes only against its own build. Returns
/// whether it wrote.
async fn keep_deleted_build(
    connection: &mut PgConnection,
    built: &BuiltIn,
) -> Result<bool, GatewayError> {
    Ok(sqlx::query!(
        "UPDATE builtin_deletions SET image=$2 WHERE name=$1 AND image IS DISTINCT FROM $2",
        built.name,
        built.image
    )
    .execute(connection)
    .await?
    .rows_affected()
        > 0)
}

/// Whether the file's validator (GW-U-041) takes the built-in as it stands: its display name comes from a label.
fn valid(built: &BuiltIn) -> bool {
    let input = serde_json::from_value::<TemplateInput>(json!({
        "name": built.name, "display_name": built.display_name, "image": built.image,
        "capabilities": built.capabilities, "default": false,
    }));
    match input.map(validate) {
        Ok(Ok(_)) => true,
        Ok(Err((field, reason))) => {
            tracing::warn!(
                template = built.name,
                field,
                reason = reason.as_str(),
                "a built-in environment was not registered: its image's labels are invalid"
            );
            false
        }
        Err(_) => false,
    }
}

/// The default Workstation takes once its checks pass: the file's implicit Minimal, when nobody chose a default
/// (no `[[templates]]`, and no administrator has written an environment), so New task's suggestions have its tools.
/// Otherwise `None`: an operator's or administrator's default stands, and Minimal stays the fallback.
async fn default_to_take(
    connection: &mut PgConnection,
    name: &str,
    implicit: bool,
) -> Result<Option<(String, i64)>, GatewayError> {
    if name != "workstation" || !implicit {
        return Ok(None);
    }
    let touched = sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM templates WHERE last_set_by='admin' OR customised_builtin) \
         OR EXISTS (SELECT 1 FROM admin_audit WHERE area='templates' AND actor_kind='admin') AS \"touched!\""
    )
    .fetch_one(&mut *connection)
    .await?;
    if touched {
        return Ok(None);
    }
    Ok(sqlx::query!(
        "SELECT name,version FROM templates WHERE is_default AND last_set_by='seed' FOR UPDATE"
    )
    .fetch_optional(&mut *connection)
    .await?
    .map(|row| (row.name, row.version)))
}

/// Makes `name` the default when it is Workstation, its current version just passed its checks and nobody chose a
/// default ([`default_to_take`]): the implicit Minimal gives it up, and both moves are audited as the built-ins'.
/// Returns whether it moved.
///
/// # Errors
/// Storage failure.
pub(crate) async fn take_default(
    connection: &mut PgConnection,
    name: &str,
    implicit: bool,
) -> Result<bool, GatewayError> {
    let Some((previous, version)) = default_to_take(connection, name, implicit).await? else {
        return Ok(false);
    };
    let Some(own) = sqlx::query_scalar!(
        "SELECT version FROM templates WHERE name=$1 AND last_set_by='builtin' AND NOT customised_builtin \
         AND NOT is_default AND check_state='verified' AND checked_version=version FOR UPDATE",
        name
    )
    .fetch_optional(&mut *connection)
    .await?
    else {
        return Ok(false);
    };
    // Clear the old default first: the one-default index is checked row by row. A version move keeps its check.
    for (item, is_default, before) in [(previous.as_str(), false, version), (name, true, own)] {
        sqlx::query!(
            "UPDATE templates SET is_default=$2,version=version+1,\
             checked_version=CASE WHEN checked_version=version THEN version+1 ELSE checked_version END,\
             updated_at=CURRENT_TIMESTAMP WHERE name=$1",
            item,
            is_default
        )
        .execute(&mut *connection)
        .await?;
        audit_builtin(
            connection,
            &Change {
                area: "templates",
                actor: None,
                item,
                action: "replace",
                before: Some(before),
                after: Some(before + 1),
                value_changed: true,
            },
        )
        .await?;
    }
    tracing::info!(
        template = name,
        previous,
        "a built-in environment passed its checks and became the default"
    );
    Ok(true)
}

/// Adds a built-in no row holds yet, never as the default, unless the installation holds all the environments it may.
/// Workstation takes the default only once its checks pass on this backend ([`take_default`]): a Workstation the
/// backend can't run (a microVM install without its rootfs) as the default refused every task made without a template
/// (E2E-VM-013, GW-I-172).
async fn create(
    connection: &mut PgConnection,
    built: &BuiltIn,
    (capabilities, created): (&[String], time::OffsetDateTime),
) -> Result<bool, GatewayError> {
    let stored = sqlx::query_scalar!("SELECT count(*) AS \"count!\" FROM templates")
        .fetch_one(&mut *connection)
        .await?;
    if stored >= i64::try_from(super::templates::MAX_STORED_TEMPLATES).unwrap_or(i64::MAX) {
        tracing::warn!(
            template = built.name,
            "a built-in environment was not registered: the installation holds as many environments as it may"
        );
        return Ok(false);
    }
    let own = crate::environments::builtin::own_size(built.name);
    let (cpu, memory, pids) = (
        own.map(|size| size.cpu_millicores),
        own.map(|size| size.memory_bytes),
        own.map(|size| size.pids),
    );
    sqlx::query!(
        "INSERT INTO templates (name,display_name,image,capabilities,cpu_millicores,memory_bytes,pids,is_default,version,\
         last_set_by,builtin_image_created) VALUES ($1,$2,$3,$4,$6,$7,$8,false,1,'builtin',$5)",
        built.name,
        built.display_name,
        built.image,
        capabilities,
        created,
        cpu,
        memory,
        pids
    )
    .execute(&mut *connection)
    .await
    .map_err(|error| refused(built.name, &error))?;
    audit_builtin(
        connection,
        &Change {
            area: "templates",
            actor: None,
            item: built.name,
            action: "create",
            before: None,
            after: Some(1),
            value_changed: true,
        },
    )
    .await?;
    tracing::info!(
        template = built.name,
        "a built-in environment was registered"
    );
    Ok(true)
}
