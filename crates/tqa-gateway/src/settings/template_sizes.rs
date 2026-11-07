//! A template's size (the environment sizes design) as its columns store it and as the API and the seed record show it.
use serde_json::{Value, json};

use crate::{
    GatewayError,
    config::{Size, Template},
};

const MIB: u64 = 1024 * 1024;

/// The size fields as the API and the seed record show them: `cpus` a whole number when it is one (so a seed from
/// before halves keeps its digest), and `pids` and `workspace_mib` only when set, unless `all` (the API shows each).
pub(super) fn sized(mut record: Value, size: &crate::config::Size, all: bool) -> Value {
    if let Value::Object(fields) = &mut record {
        let cpus = size.cpu_millicores.map(|millicores| {
            if millicores % 1000 == 0 {
                json!(millicores / 1000)
            } else {
                json!(crate::config::sizes::cpus(millicores))
            }
        });
        fields.insert("cpus".into(), cpus.unwrap_or(Value::Null));
        fields.insert(
            "memory_mib".into(),
            json!(size.memory_bytes.map(|bytes| bytes / MIB)),
        );
        for (key, value) in [
            ("pids", size.pids.map(u64::from)),
            (
                "workspace_mib",
                size.workspace_bytes.map(|bytes| bytes / MIB),
            ),
        ] {
            if all || value.is_some() {
                fields.insert(key.into(), json!(value));
            }
        }
    }
    record
}

/// The size columns a size names (the keys of a `clamped_from` note).
pub(super) fn named_sizes(size: &crate::config::Size) -> Vec<String> {
    [
        ("cpu_millicores", size.cpu_millicores.is_some()),
        ("memory_bytes", size.memory_bytes.is_some()),
        ("pids", size.pids.is_some()),
        ("workspace_bytes", size.workspace_bytes.is_some()),
    ]
    .into_iter()
    .filter(|(_, named)| *named)
    .map(|(field, _)| field.to_owned())
    .collect()
}

/// The size of stored columns (`None` for any value out of range, which the validator then refuses).
pub(super) fn stored_size(
    cpu: Option<i32>,
    memory: Option<i64>,
    pids: Option<i32>,
    workspace: Option<i64>,
) -> crate::config::Size {
    crate::config::Size {
        cpu_millicores: cpu.and_then(|value| u32::try_from(value).ok()),
        memory_bytes: memory.and_then(|value| u64::try_from(value).ok()),
        pids: pids.and_then(|value| u32::try_from(value).ok()),
        workspace_bytes: workspace.and_then(|value| u64::try_from(value).ok()),
    }
}

/// A template's stored columns: capability names, CPU in millicores, memory, processes and workspace disk.
pub(super) struct Columns {
    pub(super) capabilities: Vec<String>,
    pub(super) cpu: Option<i32>,
    pub(super) memory: Option<i64>,
    pub(super) pids: Option<i32>,
    pub(super) workspace: Option<i64>,
}

pub(super) fn columns(template: &Template) -> Result<Columns, GatewayError> {
    Ok(Columns {
        capabilities: template
            .environment()
            .capabilities
            .iter()
            .map(|capability| capability.as_str().to_owned())
            .collect(),
        ..stored_columns(&template.size())?
    })
}

/// A size's columns (no capabilities).
fn stored_columns(size: &Size) -> Result<Columns, GatewayError> {
    let out_of_range = |_| GatewayError::Configuration("a template size exceeds its bound");
    Ok(Columns {
        capabilities: Vec::new(),
        cpu: size
            .cpu_millicores
            .map(i32::try_from)
            .transpose()
            .map_err(out_of_range)?,
        memory: size
            .memory_bytes
            .map(i64::try_from)
            .transpose()
            .map_err(out_of_range)?,
        pids: size
            .pids
            .map(i32::try_from)
            .transpose()
            .map_err(out_of_range)?,
        workspace: size
            .workspace_bytes
            .map(i64::try_from)
            .transpose()
            .map_err(out_of_range)?,
    })
}

/// A size as its note stores it, in column units (`cpu_millicores`, `memory_bytes`, `pids`, `workspace_bytes`).
pub(super) fn noted_size(note: &Value) -> Size {
    let field = |name: &str| note.get(name).and_then(Value::as_u64);
    Size {
        cpu_millicores: field("cpu_millicores").and_then(|value| u32::try_from(value).ok()),
        memory_bytes: field("memory_bytes"),
        pids: field("pids").and_then(|value| u32::try_from(value).ok()),
        workspace_bytes: field("workspace_bytes"),
    }
}

/// The note as the API shows it: each noted size in its API field (`cpus`, `memory_mib`, …).
pub(super) fn shown_note(note: &Value) -> Value {
    let mut shown = sized(json!({}), &noted_size(note), true);
    if let Value::Object(fields) = &mut shown {
        fields.retain(|_, value| !value.is_null());
    }
    shown
}

/// What each size had before the ceiling lowered it: the note's value where it has one, the stored one otherwise.
fn original(stored: &Size, note: Option<&Value>) -> Size {
    let noted = note.map(noted_size).unwrap_or_default();
    Size {
        cpu_millicores: noted.cpu_millicores.or(stored.cpu_millicores),
        memory_bytes: noted.memory_bytes.or(stored.memory_bytes),
        pids: noted.pids.or(stored.pids),
        workspace_bytes: noted.workspace_bytes.or(stored.workspace_bytes),
    }
}

/// `size` within `ceiling`.
fn within(size: &Size, ceiling: &crate::config::Ceiling) -> Size {
    Size {
        cpu_millicores: size
            .cpu_millicores
            .map(|own| own.min(ceiling.cpu_millicores())),
        memory_bytes: size.memory_bytes.map(|own| own.min(ceiling.memory_bytes())),
        pids: size.pids.map(|own| own.min(ceiling.pids())),
        workspace_bytes: size
            .workspace_bytes
            .map(|own| own.min(ceiling.workspace_bytes())),
    }
}

/// The note for `original` under `ceiling`: each size above it, in column units; `None` when none is.
fn note(original: &Size, ceiling: &crate::config::Ceiling) -> Option<Value> {
    let above = ceiling.exceeded(original);
    let mut noted = serde_json::Map::new();
    for (field, column, value) in [
        (
            "cpus",
            "cpu_millicores",
            original.cpu_millicores.map(u64::from),
        ),
        ("memory_mib", "memory_bytes", original.memory_bytes),
        ("pids", "pids", original.pids.map(u64::from)),
        ("workspace_mib", "workspace_bytes", original.workspace_bytes),
    ] {
        if let Some(value) = value.filter(|_| above.contains(&field)) {
            noted.insert(column.into(), json!(value));
        }
    }
    (!noted.is_empty()).then_some(Value::Object(noted))
}

/// Fits every stored size to the installation's ceiling, at start. A size above it (an upgrade's fold from the Limits
/// page, or a ceiling the file lowered since) is lowered to it, and the environment keeps a note of what it had
/// (`clamped_from`) until an administrator saves it; a ceiling raised again gives a noted size back, up to the new
/// ceiling. Tasks already got the ceiling at run time, so nothing needs checking again. Each environment changed is
/// audited like a seed change and logged. Returns whether any changed.
///
/// # Errors
/// Storage failure.
pub(super) async fn clamp(
    connection: &mut sqlx::PgConnection,
    ceiling: &crate::config::Ceiling,
) -> Result<bool, GatewayError> {
    let rows = sqlx::query!(
        "SELECT name,cpu_millicores,memory_bytes,pids,workspace_bytes,clamped_from FROM templates ORDER BY name FOR UPDATE"
    )
    .fetch_all(&mut *connection)
    .await?;
    let mut changed = false;
    for row in rows {
        let stored = stored_size(
            row.cpu_millicores,
            row.memory_bytes,
            row.pids,
            row.workspace_bytes,
        );
        let original = original(&stored, row.clamped_from.as_ref());
        let (fitted, noted) = (within(&original, ceiling), note(&original, ceiling));
        if fitted == stored && noted == row.clamped_from {
            continue;
        }
        let Columns {
            cpu,
            memory,
            pids,
            workspace,
            ..
        } = stored_columns(&fitted)?;
        let version = sqlx::query_scalar!(
            "UPDATE templates SET cpu_millicores=$2,memory_bytes=$3,pids=$4,workspace_bytes=$5,clamped_from=$6,\
             version=version+1,checked_version=CASE WHEN checked_version=version THEN version+1 ELSE checked_version END,\
             updated_at=CURRENT_TIMESTAMP WHERE name=$1 RETURNING version",
            row.name,
            cpu,
            memory,
            pids,
            workspace,
            noted
        )
        .fetch_one(&mut *connection)
        .await?;
        let change = crate::settings::Change {
            area: "templates",
            actor: None,
            item: &row.name,
            action: "replace",
            before: Some(version - 1),
            after: Some(version),
            value_changed: true,
        };
        crate::settings::audit(connection, &change).await?;
        let noted = noted.map_or_else(String::new, |noted| noted.to_string());
        tracing::warn!(
            template = row.name,
            lowered_from = noted,
            "an environment's size was fitted to the installation's ceiling"
        );
        changed = true;
    }
    Ok(changed)
}
