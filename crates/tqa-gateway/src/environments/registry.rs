//! The template registry: the settings snapshot holds the templates (ADR-0028, ADR-0032); conversations bind to one
//! template and its pin.
use crate::{GatewayError, config::Template, settings::Snapshot};
use tqa_types::{Capability, Environment};
use uuid::Uuid;

/// A conversation's environment binding with its template's declared capabilities and resource overrides.
pub(crate) struct Binding {
    pub template: String,
    pub digest: String,
    pub capabilities: Vec<Capability>,
    pub cpu_millicores: Option<u32>,
    pub memory_bytes: Option<u64>,
    pub pids: Option<u32>,
    /// The template version whose image the conversation was last moved to; absent until it moves (ADR-0032).
    pub version: Option<u64>,
}

impl Binding {
    /// The environment the runtime receives in `session.config`, validated by the shared protocol rules.
    ///
    /// A task bound before templates had pins (Minimal, no digest) follows the runtime image and gets the legacy
    /// environment, the only form the wire allows without a pin, whatever Minimal declares now (GW-U-108: Minimal became
    /// Shell and Python, and such tasks got `store_unavailable`).
    ///
    /// # Errors
    /// Fails when the stored binding no longer forms a valid environment.
    pub(crate) fn environment(&self) -> Result<Environment, GatewayError> {
        let legacy = Environment::default();
        if self.digest.is_empty() && self.template == legacy.template {
            return Ok(legacy);
        }
        Environment::new(
            self.template.clone(),
            self.digest.clone(),
            self.capabilities.clone(),
        )
        .and_then(|environment| environment.with_version(self.version))
        .map_err(|_| GatewayError::StoreUnavailable)
    }
}

/// Templates that can be chosen: those offered (ADR-0032), default first, then by display name.
pub(crate) fn listed(snapshot: &Snapshot) -> Vec<&Template> {
    let mut templates: Vec<&Template> = snapshot
        .templates
        .iter()
        .filter(|stored| stored.offered())
        .map(|stored| &stored.template)
        .collect();
    templates.sort_by(|left, right| {
        right
            .is_default()
            .cmp(&left.is_default())
            .then_with(|| left.display_name().cmp(right.display_name()))
    });
    templates
}

/// An offered template by name, or the default when no name is given.
///
/// # Errors
/// `template_unavailable` for a template that exists but is not offered now; `unknown_template` for any other name.
pub(crate) fn choose<'a>(
    snapshot: &'a Snapshot,
    name: Option<&str>,
) -> Result<&'a Template, GatewayError> {
    let wanted =
        |template: &Template| name.map_or(template.is_default(), |name| template.name() == name);
    if let Some(template) = listed(snapshot)
        .into_iter()
        .find(|template| wanted(template))
    {
        return Ok(template);
    }
    if snapshot
        .templates
        .iter()
        .any(|stored| wanted(&stored.template))
    {
        return Err(GatewayError::TemplateUnavailable);
    }
    Err(GatewayError::UnknownTemplate)
}

/// Reads a conversation's binding joined with its template's declared capabilities and resources.
///
/// # Errors
/// Fails on a storage error; a template missing from the table is refused as unknown.
pub(crate) async fn binding(
    executor: impl sqlx::PgExecutor<'_>,
    conversation: Uuid,
) -> Result<Binding, GatewayError> {
    let row = sqlx::query!("SELECT c.template,c.template_digest,c.template_version,t.capabilities AS \"capabilities?\",t.cpu_millicores,t.memory_bytes,t.pids FROM conversations c LEFT JOIN templates t ON t.name=c.template WHERE c.id=$1", conversation)
        .fetch_optional(executor)
        .await?
        .ok_or(GatewayError::NotFound)?;
    let capabilities = row
        .capabilities
        .ok_or(GatewayError::UnknownTemplate)?
        .iter()
        .map(|name| {
            serde_json::from_value::<Capability>(serde_json::Value::String(name.clone()))
                .map_err(|_| GatewayError::StoreUnavailable)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Binding {
        template: row.template,
        digest: row.template_digest,
        capabilities,
        cpu_millicores: row
            .cpu_millicores
            .map(u32::try_from)
            .transpose()
            .map_err(|_| GatewayError::StoreUnavailable)?,
        memory_bytes: row
            .memory_bytes
            .map(u64::try_from)
            .transpose()
            .map_err(|_| GatewayError::StoreUnavailable)?,
        pids: row
            .pids
            .map(u32::try_from)
            .transpose()
            .map_err(|_| GatewayError::StoreUnavailable)?,
        version: row
            .template_version
            .map(u64::try_from)
            .transpose()
            .map_err(|_| GatewayError::StoreUnavailable)?,
    })
}

/// Moves an idle conversation to its template's current image before a new sandbox is made for it (ADR-0032
/// decision 7): a security fix, or an upgrade of the built-in templates, then reaches bound conversations without a
/// switch. A run in progress keeps the image it started with, and a template that is not offered
/// keeps the old pin. Legacy bindings without a pin follow the runtime image already. The template is read from its
/// table, not the settings snapshot, on purpose: in the same transaction as the conversation's lock and its run
/// activity, so the move and the facts it rests on cannot disagree (review of 41ddaef).
///
/// # Errors
/// Storage failures.
pub(crate) async fn refresh(pool: &sqlx::PgPool, conversation: Uuid) -> Result<(), GatewayError> {
    use crate::settings::templates::{CheckState, offered, provided};
    let mut transaction = pool.begin().await?;
    let row = sqlx::query!(
        "SELECT c.template_digest,t.name AS \"name?\",t.image AS \"image?\",t.version AS \"version?\",t.enabled AS \"enabled?\",t.check_state AS \"check_state?\",t.last_set_by AS \"last_set_by?\",t.is_default AS \"is_default?\" FROM conversations c LEFT JOIN templates t ON t.name=c.template WHERE c.id=$1 FOR UPDATE OF c",
        conversation
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(GatewayError::NotFound)?;
    let (
        Some(name),
        Some(image),
        Some(version),
        Some(enabled),
        Some(check),
        Some(set_by),
        Some(is_default),
    ) = (
        row.name,
        row.image,
        row.version,
        row.enabled,
        row.check_state,
        row.last_set_by,
        row.is_default,
    )
    else {
        return Ok(());
    };
    if row.template_digest.is_empty()
        || image == row.template_digest
        || !offered(
            enabled,
            CheckState::stored(&check),
            provided(&set_by, is_default),
        )
        || crate::events::run_activity(&mut *transaction, conversation)
            .await?
            .unfinished()
    {
        return Ok(());
    }
    sqlx::query!(
        "UPDATE conversations SET template_digest=$2,template_version=$3 WHERE id=$1",
        conversation,
        image,
        version
    )
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    tracing::info!(%conversation, template = name, version, "a conversation moved to its template's current image");
    Ok(())
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
