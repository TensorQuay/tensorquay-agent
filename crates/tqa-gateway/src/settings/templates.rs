//! Environment templates in the database (ADR-0028, ADR-0032): `[[templates]]` seeds them under the seed-change rule,
//! and the snapshot rebuilds every stored template through the same validator the file uses (GW-U-041).
pub(crate) use super::template_results::{CheckResult, Checked, record_check};
use super::{Change, KeyedDigest, audit, tombstones};
use sizes::{Columns, columns, named_sizes, sized, stored_size};

use crate::{
    GatewayError,
    config::{Template, TemplateInput, validate},
};
use serde_json::{Value, json};
use sqlx::PgConnection;
use subtle::ConstantTimeEq;

#[path = "template_sizes.rs"]
mod sizes;

/// Where a template's image and rootfs checks stand, for the version it is at (ADR-0032).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CheckState {
    /// Not checked yet, or the engine could not answer.
    Unchecked,
    /// Every check passed.
    Verified,
    /// A check failed; its reason is stored.
    Refused,
}

impl CheckState {
    /// The stored `check_state` word; anything else reads as unchecked.
    pub(crate) fn stored(word: &str) -> Self {
        match word {
            "verified" => Self::Verified,
            "refused" => Self::Refused,
            _ => Self::Unchecked,
        }
    }
}

/// Whether the install provides the template, so it stays offered while unchecked: the operator's file always, a
/// built-in only while it is the default (its rebuild is re-checked without refusing tasks). Other built-ins are
/// offered once verified, so the picker and the default agree (GW-I-172, GW-I-174).
pub(crate) fn provided(last_set_by: &str, is_default: bool) -> bool {
    last_set_by == "seed" || (last_set_by == "builtin" && is_default)
}

/// Offered to people: enabled, never after a failed check, and verified unless the install provides it ([`provided`]:
/// those stay offered while unchecked, so an engine that is briefly unreachable at start hides nothing).
pub(crate) fn offered(enabled: bool, check: CheckState, from_seed: bool) -> bool {
    enabled
        && match check {
            CheckState::Verified => true,
            CheckState::Unchecked => from_seed,
            CheckState::Refused => false,
        }
}

/// One stored template with what decides whether people are offered it. `enabled` and `check` change the offered
/// set, so they are written only in a settings transaction that locks and bumps the generation, followed by a refresh
/// (ADR-0028), as [`record_check`] does: a plain update would stay invisible until some other settings write.
#[derive(Clone, Debug)]
pub(crate) struct StoredTemplate {
    pub(crate) template: Template,
    pub(crate) enabled: bool,
    pub(crate) check: CheckState,
    /// The refusal's reason code, when `check` is `Refused`.
    pub(crate) check_reason: Option<String>,
    /// Offered while unchecked ([`provided`]): the file's values (not an admin's re-creation), or the default built-in.
    pub(crate) from_seed: bool,
}

impl StoredTemplate {
    /// Why it is not offered, as a stable word or the refusal's code; `None` when it is.
    pub(crate) fn unavailable(&self) -> Option<&str> {
        if self.offered() {
            None
        } else if !self.enabled {
            Some("disabled")
        } else if self.check == CheckState::Refused {
            Some(self.check_reason.as_deref().unwrap_or("refused"))
        } else {
            Some("unverified")
        }
    }

    /// Offered to people (see [`offered`]).
    pub(crate) fn offered(&self) -> bool {
        offered(self.enabled, self.check, self.from_seed)
    }
}

/// The seed record of one template: what the seed-change rule compares (its keyed digest) and imports.
fn record(template: &Template) -> Value {
    let environment = template.environment();
    described(
        sized(
            json!({
                "name": template.name(),
                "display_name": template.display_name(),
                "image": environment.digest,
                "capabilities": environment.capabilities.iter().map(|capability| capability.as_str()).collect::<Vec<_>>(),
                "default": template.is_default(),
            }),
            &template.size(),
            false,
        ),
        template.description(),
    )
}

/// `record` with its description, only when there is one: a seed without one keeps the digest it had before
/// descriptions existed (GW-I-132), so no seeded template looks changed on upgrade.
fn described(mut record: Value, description: Option<&str>) -> Value {
    if let (Some(text), Value::Object(fields)) = (description, &mut record) {
        fields.insert("description".into(), Value::String(text.to_owned()));
    }
    record
}

/// Applies the templates seed (the file's `[[templates]]`) in the caller's settings transaction; returns whether it
/// wrote. A template dropped from the file is kept and marked, never deleted: conversations may still use it.
///
/// # Errors
/// Storage failure.
pub(crate) async fn seed(
    connection: &mut PgConnection,
    (templates, ceiling): (&[Template], &crate::config::Ceiling),
    digest: &KeyedDigest,
) -> Result<bool, GatewayError> {
    let mut wrote = false;
    for template in templates {
        wrote |= seed_template(connection, template, digest).await?;
    }
    let names: Vec<String> = templates
        .iter()
        .map(|template| template.name().to_owned())
        .collect();
    tombstones::forget_unlisted(connection, "templates", &names).await?;
    let removed = sqlx::query!(
        "UPDATE templates SET seed_state='removed' WHERE seed_state='current' AND NOT (name = ANY($1))",
        &names
    )
    .execute(&mut *connection)
    .await?;
    tracing::info!(templates = templates.len(), "templates seed applied");
    let lowered = sizes::clamp(connection, ceiling).await?;
    Ok(wrote || removed.rows_affected() > 0 || lowered)
}

/// Imports one seed template when it is new or its seed record changed; returns whether it wrote anything.
async fn seed_template(
    connection: &mut PgConnection,
    template: &Template,
    digest: &KeyedDigest,
) -> Result<bool, GatewayError> {
    let document = record(template);
    let seed = digest.of(
        &format!("templates/{}", template.name()),
        &serde_json::to_vec(&document)?,
    );
    let current = sqlx::query!(
        "SELECT version,last_set_by,seed_digest,seed_state,display_name,description,image,capabilities,cpu_millicores,memory_bytes,pids,workspace_bytes,is_default FROM templates WHERE name=$1 FOR UPDATE",
        template.name()
    )
    .fetch_optional(&mut *connection)
    .await?;
    let Some(current) = current else {
        return seed_new(connection, template, &seed).await;
    };
    let stored = described(
        sized(
            json!({
                "name": template.name(),
                "display_name": current.display_name,
                "image": current.image,
                "capabilities": current.capabilities,
                "default": current.is_default,
            }),
            &stored_size(
                current.cpu_millicores,
                current.memory_bytes,
                current.pids,
                current.workspace_bytes,
            ),
            false,
        ),
        current.description.as_deref(),
    );
    if current
        .seed_digest
        .as_deref()
        .is_some_and(|stored| bool::from(stored.ct_eq(&seed[..])))
    {
        if current.seed_state.as_deref() == Some("current") {
            return Ok(false);
        }
        sqlx::query!(
            "UPDATE templates SET seed_state='current' WHERE name=$1",
            template.name()
        )
        .execute(&mut *connection)
        .await?;
        return Ok(true);
    }
    // A row from before the seed-change rule holding exactly the seed's values: adopt it, which changes nothing.
    if current.seed_digest.is_none() && current.last_set_by == "seed" && stored == document {
        sqlx::query!(
            "UPDATE templates SET seed_digest=$2,seed_state='current' WHERE name=$1",
            template.name(),
            &seed[..]
        )
        .execute(&mut *connection)
        .await?;
        return Ok(true);
    }
    let version = current.version + 1;
    store_seed(connection, template, &seed, version).await?;
    audit(
        connection,
        &Change {
            area: "templates",
            actor: None,
            item: template.name(),
            action: "replace",
            before: Some(current.version),
            after: Some(version),
            value_changed: stored != document,
        },
    )
    .await?;
    Ok(true)
}

/// Imports a seed template no row holds yet, unless an administrator deleted it and its seed is unchanged.
async fn seed_new(
    connection: &mut PgConnection,
    template: &Template,
    seed: &[u8; 32],
) -> Result<bool, GatewayError> {
    if tombstones::buried(connection, "templates", template.name(), seed).await? {
        return Ok(false);
    }
    store_seed(connection, template, seed, 1).await?;
    audit(
        connection,
        &Change {
            area: "templates",
            actor: None,
            item: template.name(),
            action: "create",
            before: None,
            after: Some(1),
            value_changed: true,
        },
    )
    .await?;
    Ok(true)
}

/// Stores the seed's values at `version`. A new default first takes the default from any other template, since one
/// template at most may hold it; the new values need checking again. A size the upgrade folded in from the Limits page
/// stays unless the file's entry names that size (the environment sizes design).
async fn store_seed(
    connection: &mut PgConnection,
    template: &Template,
    seed: &[u8; 32],
    version: i64,
) -> Result<(), GatewayError> {
    // The implicit Minimal (no `[[templates]]`) never takes the default back from another environment, such as the
    // Workstation built-in, when `[runtime] image` changes: nobody chose it.
    let other_default = sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM templates WHERE is_default AND name<>$1) AS \"exists!\"",
        template.name()
    )
    .fetch_one(&mut *connection)
    .await?;
    let is_default = template.is_default() && !(template.implicit() && other_default);
    if is_default {
        sqlx::query!(
            "UPDATE templates SET is_default=false,updated_at=CURRENT_TIMESTAMP WHERE is_default AND name<>$1",
            template.name()
        )
        .execute(&mut *connection)
        .await?;
    }
    let environment = template.environment();
    let Columns {
        capabilities,
        cpu,
        memory,
        pids,
        workspace,
    } = columns(template)?;
    sqlx::query!(
        "INSERT INTO templates (name,display_name,image,capabilities,cpu_millicores,memory_bytes,pids,workspace_bytes,is_default,version,last_set_by,seed_digest,seed_state,description) \
         VALUES ($1,$2,$3,$4,$5,$6,$11,$12,$7,$8,'seed',$9,'current',$10) \
         ON CONFLICT (name) DO UPDATE SET display_name=EXCLUDED.display_name,description=COALESCE(EXCLUDED.description,templates.description),image=EXCLUDED.image,capabilities=EXCLUDED.capabilities,\
         cpu_millicores=CASE WHEN templates.sizes_from_limits THEN COALESCE(EXCLUDED.cpu_millicores,templates.cpu_millicores) ELSE EXCLUDED.cpu_millicores END,\
         memory_bytes=CASE WHEN templates.sizes_from_limits THEN COALESCE(EXCLUDED.memory_bytes,templates.memory_bytes) ELSE EXCLUDED.memory_bytes END,\
         pids=CASE WHEN templates.sizes_from_limits THEN COALESCE(EXCLUDED.pids,templates.pids) ELSE EXCLUDED.pids END,\
         workspace_bytes=CASE WHEN templates.sizes_from_limits THEN COALESCE(EXCLUDED.workspace_bytes,templates.workspace_bytes) ELSE EXCLUDED.workspace_bytes END,\
         clamped_from=CASE WHEN templates.sizes_from_limits THEN NULLIF(templates.clamped_from - $13::text[],'{}'::jsonb) END,\
         is_default=EXCLUDED.is_default,version=EXCLUDED.version,\
         last_set_by='seed',seed_digest=EXCLUDED.seed_digest,seed_state='current',check_state='unchecked',check_reason=NULL,\
         checked_version=NULL,builtin_image_created=NULL,builtin_updated_at=NULL,customised_builtin=false,builtin_base_image=NULL,updated_at=CURRENT_TIMESTAMP",
        template.name(),
        template.display_name(),
        environment.digest,
        &capabilities,
        cpu,
        memory,
        is_default,
        version,
        &seed[..],
        template.description(),
        pids,
        workspace,
        &named_sizes(&template.size())
    )
    .execute(&mut *connection)
    .await?;
    Ok(())
}

/// Every stored template, rebuilt through the file's validator.
///
/// # Errors
/// Storage failure, or a stored template the validator refuses.
pub(crate) async fn read(
    connection: &mut PgConnection,
) -> Result<Vec<StoredTemplate>, GatewayError> {
    let rows = sqlx::query!(
        "SELECT name,display_name,description,image,capabilities,cpu_millicores,memory_bytes,pids,workspace_bytes,is_default,enabled,check_state,check_reason,last_set_by FROM templates ORDER BY name"
    )
    .fetch_all(connection)
    .await?;
    rows.into_iter()
        .map(|row| {
            let template = if row.image.is_empty() && row.name == "minimal" {
                Template::legacy_minimal()
            } else {
                // The same definition the file gives, through the same validator (GW-U-041).
                let input: TemplateInput = serde_json::from_value(sized(
                    json!({
                        "name": row.name,
                        "display_name": row.display_name,
                        "description": row.description,
                        "image": row.image,
                        "capabilities": row.capabilities,
                        "default": row.is_default,
                    }),
                    &stored_size(
                        row.cpu_millicores,
                        row.memory_bytes,
                        row.pids,
                        row.workspace_bytes,
                    ),
                    false,
                ))
                .map_err(|_| GatewayError::Configuration("a stored template is invalid"))?;
                validate(input).map_err(|(field, reason)| {
                    tracing::error!(
                        template = row.name,
                        field,
                        reason = reason.as_str(),
                        "a stored template is invalid"
                    );
                    GatewayError::Configuration("a stored template is invalid")
                })?
            };
            let check = CheckState::stored(&row.check_state);
            Ok(StoredTemplate {
                template,
                enabled: row.enabled,
                check,
                check_reason: row.check_reason,
                from_seed: provided(&row.last_set_by, row.is_default),
            })
        })
        .collect()
}

/// One template as the admin API shows it: its definition, its state, its provenance and what binds it.
#[derive(Clone, Debug)]
pub(crate) struct AdminTemplate {
    pub(crate) name: String,
    pub(crate) display_name: String,
    pub(crate) description: Option<String>,
    pub(crate) image: String,
    pub(crate) capabilities: Vec<String>,
    pub(crate) cpu_millicores: Option<i32>,
    pub(crate) memory_bytes: Option<i64>,
    pub(crate) pids: Option<i32>,
    pub(crate) workspace_bytes: Option<i64>,
    pub(crate) clamped_from: Option<Value>,
    pub(crate) is_default: bool,
    pub(crate) enabled: bool,
    pub(crate) version: i64,
    pub(crate) last_set_by: String,
    pub(crate) seed_state: Option<String>,
    pub(crate) check_state: String,
    pub(crate) check_reason: Option<String>,
    pub(crate) checked_version: Option<i64>,
    pub(crate) check_secret_files: Option<Value>,
    pub(crate) check_scan_limit: Option<Value>,
    pub(crate) base: Option<String>,
    pub(crate) image_id: Option<String>,
    pub(crate) updated_at: time::OffsetDateTime,
    /// A built-in's image build time, and when a newer build last replaced its image.
    pub(crate) builtin_image_created: Option<time::OffsetDateTime>,
    pub(crate) builtin_updated_at: Option<time::OffsetDateTime>,
    /// A built-in an administrator edited: rebuilds no longer update it until it is reset.
    pub(crate) customised_builtin: bool,
    pub(crate) bound_conversations: i64,
}

impl AdminTemplate {
    /// Its own size, as stored.
    pub(crate) fn size(&self) -> crate::config::Size {
        stored_size(
            self.cpu_millicores,
            self.memory_bytes,
            self.pids,
            self.workspace_bytes,
        )
    }

    /// What the current check recorded, as stored: `null` while the template waits for a check, so an earlier
    /// version's findings never show.
    fn checked(&self, recorded: Option<&Value>) -> Value {
        if self.check_state == "unchecked" {
            return Value::Null;
        }
        recorded.cloned().unwrap_or(Value::Null)
    }

    /// The API form (no secret can be part of a template). `clamped` names each size lowered to the installation's
    /// ceiling: at start (`clamped_from` keeps what it was) or, until the next start, by a ceiling lowered since.
    pub(crate) fn to_json(&self, ceiling: &crate::config::Ceiling) -> Result<Value, GatewayError> {
        let shown = |at: &time::OffsetDateTime| {
            at.format(&time::format_description::well_known::Rfc3339)
                .map_err(|_| GatewayError::Io)
        };
        let updated_at = shown(&self.updated_at)?;
        // A built-in says when its image was built and when a newer build replaced it: never a silent update.
        let builtin = match &self.builtin_image_created {
            Some(created) => json!({
                "image_created": shown(created)?,
                "updated_at": self.builtin_updated_at.as_ref().map(shown).transpose()?,
            }),
            None => Value::Null,
        };
        let size = self.size();
        Ok(sized(
            json!({
                "name": self.name,
                "display_name": self.display_name,
                "description": self.description,
                "image": self.image,
                "capabilities": self.capabilities,
                "is_default": self.is_default,
                "enabled": self.enabled,
                "version": self.version,
                "last_set_by": self.last_set_by,
                "seed": self.seed_state,
                "check": {"state": self.check_state, "reason": self.check_reason, "checked_version": self.checked_version,
                          "secret_files": self.checked(self.check_secret_files.as_ref()),
                          "scan_limit": self.checked(self.check_scan_limit.as_ref())},
                "base": self.base,
                "image_id": self.image_id,
                "bound_conversations": self.bound_conversations,
                "updated_at": updated_at,
                "builtin": builtin,
                "customised": self.customised_builtin,
                "clamped": clamped(self.clamped_from.as_ref(), &ceiling.exceeded(&size)),
                "clamped_from": self.clamped_from.as_ref().map(sizes::shown_note),
            }),
            &size,
            true,
        ))
    }
}

/// The size fields lowered to the ceiling, in field order.
fn clamped(noted: Option<&Value>, exceeded: &[&'static str]) -> Vec<&'static str> {
    ["cpus", "memory_mib", "pids", "workspace_mib"]
        .into_iter()
        .filter(|field| {
            exceeded.contains(field)
                || noted.is_some_and(|noted| sizes::shown_note(noted).get(field).is_some())
        })
        .collect()
}

/// Every template, by name, as the admin API shows them.
///
/// # Errors
/// Storage failure.
pub(crate) async fn entries(
    connection: &mut PgConnection,
) -> Result<Vec<AdminTemplate>, GatewayError> {
    Ok(sqlx::query_as!(
        AdminTemplate,
        "SELECT t.name,t.display_name,t.description,t.image,t.capabilities,t.cpu_millicores,t.memory_bytes,t.pids,t.workspace_bytes,t.clamped_from,t.is_default,t.enabled,t.version,t.last_set_by,t.seed_state,t.check_state,t.check_reason,t.checked_version,t.check_secret_files,t.check_scan_limit,t.base,t.image_id,t.updated_at,t.builtin_image_created,t.builtin_updated_at,t.customised_builtin,\
         (SELECT count(*) FROM conversations c WHERE c.template=t.name AND c.state='active') AS \"bound_conversations!\" \
         FROM templates t ORDER BY t.name"
    )
    .fetch_all(connection)
    .await?)
}

pub(crate) async fn entry(
    connection: &mut PgConnection,
    name: &str,
) -> Result<AdminTemplate, GatewayError> {
    entries(connection)
        .await?
        .into_iter()
        .find(|stored| stored.name == name)
        .ok_or(GatewayError::NotFound)
}

/// An administrator's write of one template (ADR-0032): its definition; `default` and `enabled` when given.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TemplateWrite {
    pub(crate) display_name: String,
    /// Any JSON, so a non-string is refused as `description_invalid`, not as an unreadable body.
    #[serde(default)]
    pub(crate) description: Option<Value>,
    pub(crate) image: String,
    pub(crate) capabilities: Vec<String>,
    pub(crate) cpus: Option<f64>,
    pub(crate) memory_mib: Option<u64>,
    #[serde(default)]
    pub(crate) pids: Option<u32>,
    #[serde(default)]
    pub(crate) workspace_mib: Option<u64>,
    pub(crate) enabled: Option<bool>,
    pub(crate) default: Option<bool>,
    pub(crate) version: Option<i64>,
    /// File-only (ADR-0032): present in a write, it is refused with `400 file_only`.
    pub(crate) acknowledged_secret_files: Option<serde_json::Value>,
}

/// A stored template's values as a write compares them.
struct Current {
    version: i64,
    is_default: bool,
    enabled: bool,
    display_name: String,
    description: Option<String>,
    image: String,
    capabilities: Vec<String>,
    cpu_millicores: Option<i32>,
    memory_bytes: Option<i64>,
    pids: Option<i32>,
    workspace_bytes: Option<i64>,
    check_state: String,
}

impl Current {
    /// Its seed-comparable record, as `record` gives one for a template.
    fn record(&self, name: &str) -> Value {
        described(
            sized(
                json!({
                    "name": name, "display_name": self.display_name, "image": self.image,
                    "capabilities": self.capabilities, "default": self.is_default,
                }),
                &stored_size(
                    self.cpu_millicores,
                    self.memory_bytes,
                    self.pids,
                    self.workspace_bytes,
                ),
                false,
            ),
            self.description.as_deref(),
        )
    }
}

/// Whether a replacement needs checking again: a checked input changed. The default must stay offered, or
/// every create without a template name is refused, so a write that would leave it unoffered is refused.
///
/// # Errors
/// `409 template_is_default`; a template whose columns cannot be stored.
fn recheck(current: &Current, template: &Template, enabled: bool) -> Result<bool, GatewayError> {
    let stored = columns(template)?;
    // The image, capabilities, CPU and memory are checked inputs; processes and disk are not.
    let changed = current.image != template.environment().digest
        || current.capabilities != stored.capabilities
        || current.cpu_millicores != stored.cpu
        || current.memory_bytes != stored.memory;
    let check = if changed {
        CheckState::Unchecked
    } else {
        CheckState::stored(&current.check_state)
    };
    // An administrator's write is never the file's.
    if current.is_default && !offered(enabled, check, false) {
        return Err(GatewayError::Conflict("template_is_default"));
    }
    Ok(changed)
}

/// The set rules for a write: at most 32 templates, and exactly one default (a second one, or none left, is refused;
/// the default moves through its own endpoint).
async fn set_rules(
    connection: &mut PgConnection,
    name: &str,
    created: bool,
    is_default: bool,
) -> Result<(), GatewayError> {
    let others = sqlx::query!(
        "SELECT count(*) AS \"count!\", count(*) FILTER (WHERE is_default) AS \"defaults!\" FROM templates WHERE name<>$1",
        name
    )
    .fetch_one(&mut *connection)
    .await?;
    if created && others.count >= i64::try_from(MAX_STORED_TEMPLATES).unwrap_or(i64::MAX) {
        return Err(GatewayError::Conflict("template_count"));
    }
    if (is_default && others.defaults > 0) || (!is_default && others.defaults == 0) {
        return Err(GatewayError::Conflict("default_count"));
    }
    Ok(())
}

/// Creates (no `version`, the name free) or replaces (the current `version`) one template in the caller's settings
/// transaction, through the file's validator (GW-U-041); the set rules answer 409s with their codes. A change to a
/// checked input needs checking again. Returns whether it created, and the stored template.
///
/// # Errors
/// `400 invalid_request`, `409` `conflict`, `name_taken`, `default_count`, `template_count` or `template_is_default` (the
/// default disabled, or a checked input of it changed), storage failure.
pub(crate) async fn write(
    connection: &mut PgConnection,
    name: &str,
    write: TemplateWrite,
    (actor, ceiling): (uuid::Uuid, &crate::config::Ceiling),
) -> Result<(bool, AdminTemplate), GatewayError> {
    let current = sqlx::query_as!(
        Current,
        "SELECT version,is_default,enabled,display_name,description,image,capabilities,cpu_millicores,memory_bytes,pids,workspace_bytes,check_state FROM templates WHERE name=$1 FOR UPDATE",
        name
    )
    .fetch_optional(&mut *connection)
    .await?;
    let created = match (&current, write.version) {
        (None, None) => true,
        (Some(_), None) => return Err(GatewayError::Conflict("name_taken")),
        (Some(current), Some(expected)) if current.version == expected => false,
        _ => return Err(GatewayError::Conflict("conflict")),
    };
    let is_default = write
        .default
        .unwrap_or_else(|| current.as_ref().is_some_and(|current| current.is_default));
    let template = definition(name, &write, (is_default, ceiling))?;
    set_rules(connection, name, created, is_default).await?;
    let stored = record(&template);
    let enabled = write
        .enabled
        .unwrap_or_else(|| current.as_ref().is_none_or(|current| current.enabled));
    let checked_changed = match &current {
        Some(current) => recheck(current, &template, enabled)?,
        None => false,
    };
    let (before, after, value_changed) = if let Some(current) = current {
        let previous = current.record(name);
        replace(
            connection,
            &template,
            enabled,
            current.version + 1,
            checked_changed,
        )
        .await?;
        (
            Some(current.version),
            current.version + 1,
            previous != stored || current.enabled != enabled,
        )
    } else {
        // A seeded template deleted earlier keeps its seed digest, so an unchanged seed does not overwrite this one.
        let digest = tombstones::unearth(connection, "templates", name).await?;
        create(connection, &template, enabled, digest.as_deref()).await?;
        (None, 1, true)
    };
    audit(
        connection,
        &Change {
            area: "templates",
            actor: Some(actor),
            item: name,
            action: if created { "create" } else { "replace" },
            before,
            after: Some(after),
            value_changed,
        },
    )
    .await?;
    tracing::info!(
        template = name,
        version = after,
        "template updated by an administrator"
    );
    Ok((created, entry(connection, name).await?))
}

/// The written definition, through the file's validator (GW-U-041), and never sized above the installation's ceiling
/// (the environment sizes design).
fn definition(
    name: &str,
    write: &TemplateWrite,
    (is_default, ceiling): (bool, &crate::config::Ceiling),
) -> Result<Template, GatewayError> {
    let description = crate::description::from_json(write.description.clone())?;
    let input: TemplateInput = serde_json::from_value(json!({
        "name": name, "display_name": write.display_name, "description": description,
        "image": write.image, "capabilities": write.capabilities,
        "default": is_default, "cpus": write.cpus, "memory_mib": write.memory_mib,
        "pids": write.pids, "workspace_mib": write.workspace_mib,
    }))
    // Every other field has the same type in `TemplateWrite`; only a capability name can fail to convert.
    .map_err(|_| GatewayError::Invalid {
        field: "capabilities",
        reason: crate::InvalidReason::CapabilitiesInvalid,
    })?;
    let template =
        validate(input).map_err(|(field, reason)| GatewayError::Invalid { field, reason })?;
    if let Some(field) = ceiling.exceeded_by(&template.size()) {
        return Err(GatewayError::Invalid {
            field,
            reason: crate::InvalidReason::LimitOutOfRange,
        });
    }
    Ok(template)
}

/// Templates an install may hold, from the file and the API together.
pub(crate) const MAX_STORED_TEMPLATES: usize = 32;

async fn create(
    connection: &mut PgConnection,
    template: &Template,
    enabled: bool,
    digest: Option<&[u8]>,
) -> Result<(), GatewayError> {
    let stored = columns(template)?;
    let state = digest.map(|_| "current");
    sqlx::query!(
        "INSERT INTO templates (name,display_name,image,capabilities,cpu_millicores,memory_bytes,pids,workspace_bytes,is_default,enabled,version,last_set_by,seed_digest,seed_state,description) \
         VALUES ($1,$2,$3,$4,$5,$6,$12,$13,$7,$8,1,'admin',$9,$10,$11)",
        template.name(),
        template.display_name(),
        template.environment().digest,
        &stored.capabilities,
        stored.cpu,
        stored.memory,
        template.is_default(),
        enabled,
        digest,
        state,
        template.description(),
        stored.pids,
        stored.workspace
    )
    .execute(&mut *connection)
    .await?;
    Ok(())
}

/// Stores an administrator's replacement. The check is reset only when `recheck` (a checked input changed); otherwise a
/// check of the previous version carries over, as the definition the checker read did not change.
async fn replace(
    connection: &mut PgConnection,
    template: &Template,
    enabled: bool,
    version: i64,
    recheck: bool,
) -> Result<(), GatewayError> {
    let stored = columns(template)?;
    sqlx::query!(
        "UPDATE templates SET display_name=$2,image=$3,capabilities=$4,cpu_millicores=$5,memory_bytes=$6,pids=$12,workspace_bytes=$13,is_default=$7,enabled=$8,version=$9,description=$11,\
         last_set_by='admin',clamped_from=NULL,sizes_from_limits=false,builtin_image_created=NULL,builtin_updated_at=NULL,\
         customised_builtin=(templates.last_set_by='builtin' OR templates.customised_builtin),builtin_base_image=CASE WHEN templates.last_set_by='builtin' THEN templates.image ELSE templates.builtin_base_image END,check_state=CASE WHEN $10 THEN 'unchecked' ELSE check_state END,\
         check_reason=CASE WHEN $10 THEN NULL ELSE check_reason END,\
         checked_version=CASE WHEN $10 THEN NULL WHEN checked_version=version THEN $9 ELSE checked_version END,\
         base=CASE WHEN $10 THEN NULL ELSE base END,image_id=CASE WHEN $10 THEN NULL ELSE image_id END,\
         updated_at=CURRENT_TIMESTAMP WHERE name=$1",
        template.name(),
        template.display_name(),
        template.environment().digest,
        &stored.capabilities,
        stored.cpu,
        stored.memory,
        template.is_default(),
        enabled,
        version,
        recheck,
        template.description(),
        stored.pids,
        stored.workspace
    )
    .execute(&mut *connection)
    .await?;
    Ok(())
}

/// What still uses a template, which refuses its deletion (`409 template_in_use`, ADR-0032).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InUse {
    /// It is the default template.
    pub default: bool,
    /// Active conversations bound to it.
    pub conversations: i64,
    /// The `[[mcp.servers]]` entries that list it.
    pub mcp_servers: Vec<String>,
    /// The shared profiles that name it (ADR-0012).
    pub profiles: Vec<String>,
    /// Personal profiles that name it: counted, never named, since only their owners may see them.
    pub personal_profiles: i64,
}

/// Deletes the template at version `expected` unless it is the default, a conversation binds it, an
/// `[[mcp.servers]]` entry lists it (`mcp_servers`: those that do), or a profile names it.
///
/// # Errors
/// `404`, `409 conflict`, `409 template_in_use` with what uses it, storage failure.
pub(crate) async fn remove(
    connection: &mut PgConnection,
    name: &str,
    expected: i64,
    (actor, mcp_servers): (uuid::Uuid, Vec<String>),
) -> Result<(), GatewayError> {
    let current = sqlx::query!(
        "SELECT version,is_default,seed_digest,last_set_by,customised_builtin,COALESCE(builtin_base_image,image) AS \"build!\" FROM templates WHERE name=$1 FOR UPDATE",
        name
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(GatewayError::NotFound)?;
    if current.version != expected {
        return Err(GatewayError::Conflict("conflict"));
    }
    let conversations = sqlx::query_scalar!(
        "SELECT count(*) AS \"count!\" FROM conversations WHERE template=$1 AND state='active'",
        name
    )
    .fetch_one(&mut *connection)
    .await?;
    let profiles = sqlx::query_scalar!(
        "SELECT name FROM agent_profiles WHERE owner_id IS NULL AND document->>'template'=$1 ORDER BY name",
        name
    )
    .fetch_all(&mut *connection)
    .await?;
    let personal_profiles = sqlx::query_scalar!(
        "SELECT count(*) AS \"count!\" FROM agent_profiles WHERE owner_id IS NOT NULL AND document->>'template'=$1",
        name
    )
    .fetch_one(&mut *connection)
    .await?;
    if current.is_default
        || conversations > 0
        || !mcp_servers.is_empty()
        || !profiles.is_empty()
        || personal_profiles > 0
    {
        return Err(GatewayError::TemplateInUse(InUse {
            default: current.is_default,
            conversations,
            mcp_servers,
            profiles,
            personal_profiles,
        }));
    }
    sqlx::query!("DELETE FROM templates WHERE name=$1", name)
        .execute(&mut *connection)
        .await?;
    tombstones::bury(
        connection,
        "templates",
        name,
        current.seed_digest.as_deref(),
    )
    .await?;
    // A built-in, customised or not, stays deleted by name: a rebuild of its image doesn't bring it back (the
    // built-in environments design).
    if current.last_set_by == "builtin" || current.customised_builtin {
        super::builtins::bury(connection, name, &current.build).await?;
    }
    audit(
        connection,
        &Change {
            area: "templates",
            actor: Some(actor),
            item: name,
            action: "delete",
            before: Some(expected),
            after: None,
            value_changed: true,
        },
    )
    .await?;
    tracing::info!(template = name, "template deleted by an administrator");
    Ok(())
}

/// Makes the template at version `expected` the default, taking it from the current default in the same transaction.
/// The definition is unchanged, so provenance stays and a check result carries over to the new version.
///
/// # Errors
/// `404`, `409 conflict`, `409 template_unavailable` for a template that is not offered, storage failure.
pub(crate) async fn make_default(
    connection: &mut PgConnection,
    name: &str,
    expected: i64,
    actor: uuid::Uuid,
) -> Result<AdminTemplate, GatewayError> {
    let current = sqlx::query!(
        "SELECT version,is_default,enabled,check_state,last_set_by FROM templates WHERE name=$1 FOR UPDATE",
        name
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(GatewayError::NotFound)?;
    if current.version != expected {
        return Err(GatewayError::Conflict("conflict"));
    }
    if current.is_default {
        return entry(connection, name).await;
    }
    // Only an offered template can be the default: a create without a template name gets the default.
    if !offered(
        current.enabled,
        CheckState::stored(&current.check_state),
        provided(&current.last_set_by, current.is_default),
    ) {
        return Err(GatewayError::TemplateUnavailable);
    }
    // Clear the old default first: the one-default index is checked row by row, not per statement.
    let mut moved: Vec<(String, i64)> = sqlx::query!(
        "UPDATE templates SET is_default=false,version=version+1,\
         checked_version=CASE WHEN checked_version=version THEN version+1 ELSE checked_version END,updated_at=CURRENT_TIMESTAMP \
         WHERE is_default AND name<>$1 RETURNING name,version",
        name
    )
    .fetch_all(&mut *connection)
    .await?
    .into_iter()
    .map(|row| (row.name, row.version))
    .collect();
    let made = sqlx::query!(
        "UPDATE templates SET is_default=true,version=version+1,\
         checked_version=CASE WHEN checked_version=version THEN version+1 ELSE checked_version END,updated_at=CURRENT_TIMESTAMP \
         WHERE name=$1 RETURNING name,version",
        name
    )
    .fetch_one(&mut *connection)
    .await?;
    moved.push((made.name, made.version));
    for (item, version) in moved {
        audit(
            connection,
            &Change {
                area: "templates",
                actor: Some(actor),
                item: &item,
                action: "replace",
                before: Some(version - 1),
                after: Some(version),
                value_changed: true,
            },
        )
        .await?;
    }
    tracing::info!(template = name, "the default template moved");
    entry(connection, name).await
}

/// Queues the template's checks again (after its image or rootfs was fixed); the stored state stays until the new
/// result.
///
/// # Errors
/// `404`, storage failure.
pub(crate) async fn requeue(connection: &mut PgConnection, name: &str) -> Result<(), GatewayError> {
    let queued = sqlx::query!(
        "UPDATE templates SET checked_version=NULL WHERE name=$1",
        name
    )
    .execute(&mut *connection)
    .await?;
    if queued.rows_affected() == 0 {
        return Err(GatewayError::NotFound);
    }
    Ok(())
}
