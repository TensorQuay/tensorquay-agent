//! `GET /api/v1/admin/environment-images` (the built-in environments design): the images on the engine host an
//! administrator can make an environment from, so nobody types an image ID. Read-only, and it never pulls: only what is
//! on the host is listed. Each image says what it is (its labels), whether this version can run it (and in plain words
//! why not), what uses it, and whether it is a built-in and added.
use super::require_admin;
use crate::{
    GatewayError,
    app::State,
    auth::Authz,
    environments::builtin::{BuiltIn, EngineImage, OwnSize, built_ins, classify, own_size},
};
use axum::{Json, extract::State as AppState};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

/// A stored template as the list compares it: its name, its pin and the image ID a check resolved that pin to.
struct Pinned {
    name: String,
    image: String,
    image_id: Option<String>,
    last_set_by: String,
    customised_builtin: bool,
}

/// What the install says about the images: the templates, the file's template names, the deleted built-ins and the
/// live sandboxes per image.
struct Uses {
    templates: Vec<Pinned>,
    file: Vec<String>,
    deleted: Vec<String>,
    sandboxes: BTreeMap<String, i64>,
}

/// `GET /api/v1/admin/environment-images` (administrator, session or token).
///
/// # Errors
/// Not an administrator; `503 engine_unavailable` when the engine didn't answer; a store failure.
pub(crate) async fn list(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
) -> Result<Json<Value>, GatewayError> {
    require_admin(&user)?;
    let source = state
        .environment_images
        .get()
        .ok_or(GatewayError::EngineUnavailable)?;
    let mut images = source
        .list()
        .await
        .map_err(|_| GatewayError::EngineUnavailable)?;
    // Newest first (ties by ID) and bounded, whatever order the source gave.
    images.sort_by(|left, right| {
        right
            .created
            .cmp(&left.created)
            .then_with(|| left.id.cmp(&right.id))
    });
    images.truncate(tqa_sandbox::docker::MAX_LISTED);
    let install = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        let templates = sqlx::query_as!(
            Pinned,
            "SELECT name,image,image_id,last_set_by,customised_builtin FROM templates ORDER BY name"
        )
        .fetch_all(&mut *transaction)
        .await?;
        let deleted = sqlx::query_scalar!("SELECT name FROM builtin_deletions ORDER BY name")
            .fetch_all(&mut *transaction)
            .await?;
        let sandboxes = sqlx::query!(
            "SELECT c.template_digest AS image,count(*) AS \"count!\" FROM sandbox_instances s \
             JOIN conversations c ON c.id=s.conversation_id WHERE s.phase<>'destroying' GROUP BY c.template_digest"
        )
        .fetch_all(&mut *transaction)
        .await?
        .into_iter()
        .map(|row| (row.image, row.count))
        .collect();
        transaction.commit().await?;
        Ok(Uses {
            templates,
            file: state
                .config
                .templates()
                .iter()
                .map(|template| template.name().to_owned())
                .collect(),
            deleted,
            sandboxes,
        })
    })
    .await?;
    let pinned = state.config.builtin_images();
    let found = built_ins(&images, pinned);
    let items: Vec<Value> = images
        .iter()
        .map(|image| item(image, &found, &install, pinned))
        .collect::<Result<_, _>>()?;
    Ok(Json(json!({
        "items": items,
        "limit": tqa_sandbox::docker::MAX_LISTED,
    })))
}

fn item(
    image: &EngineImage,
    found: &[BuiltIn],
    install: &Uses,
    pinned: &crate::config::BuiltinImages,
) -> Result<Value, GatewayError> {
    let classified = classify(image, pinned);
    let pins = |template: &Pinned| {
        template.image == image.id || template.image_id.as_deref() == Some(image.id.as_str())
    };
    let used_by: Vec<&str> = install
        .templates
        .iter()
        .filter(|template| pins(template))
        .map(|template| template.name.as_str())
        .collect();
    let built = found.iter().find(|built| built.image == image.id);
    let (built_in, not_added) = match built {
        None => (None, None),
        Some(built) => {
            let (state, reason) = added(built, install, &pins);
            (Some(state), reason)
        }
    };
    // A built-in's own size, so an environment started from its card takes it (GW-U-197).
    let sizes = built
        .and_then(|built| own_size(built.name))
        .map(OwnSize::listed);
    let created = time::OffsetDateTime::from_unix_timestamp(image.created)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH)
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|_| GatewayError::Io)?;
    Ok(json!({
        "id": image.id,
        "tags": image.tags,
        "created": created,
        "size_bytes": image.size_bytes,
        "runtime": classified.runtime,
        "environment": classified.environment,
        "display_name": classified.display_name,
        "capabilities": classified.capabilities,
        "capabilities_valid": classified.capabilities_valid,
        "usable": classified.usable,
        // A built-in is always usable, so its reason is why it is not added.
        "reason": classified.reason.or(not_added),
        "based_on": classified.based_on,
        "used_by": used_by,
        "sandboxes": install.sandboxes.get(&image.id).copied().unwrap_or(0),
        "built_in": built_in,
        "sizes": sizes,
    }))
}

/// Whether a built-in image is added, and why not when it isn't. A template of its name that pins this very image
/// (the file's Minimal, on the quick start) counts as added.
fn added(
    built: &BuiltIn,
    install: &Uses,
    pins: &dyn Fn(&Pinned) -> bool,
) -> (&'static str, Option<String>) {
    match install
        .templates
        .iter()
        .find(|template| template.name == built.name)
    {
        Some(template) if pins(template) => ("registered", None),
        Some(template) if template.customised_builtin => (
            "not_added",
            Some("customised by an administrator: rebuilds no longer update it".to_owned()),
        ),
        Some(template) if template.last_set_by == "builtin" => (
            "not_added",
            Some(format!(
                "{} moves to this build at the gateway's next start",
                built.display_name
            )),
        ),
        Some(_) => (
            "not_added",
            Some(format!(
                "not added: an environment named {} already exists",
                built.display_name
            )),
        ),
        None if install.file.iter().any(|name| name == built.name) => (
            "not_added",
            Some(format!(
                "not added: the gateway's configuration file has its own {} environment",
                built.display_name
            )),
        ),
        None if install.deleted.iter().any(|name| name == built.name) => {
            ("not_added", Some("deleted by an administrator".to_owned()))
        }
        None => (
            "not_added",
            Some("not added yet: it is added at the gateway's next start".to_owned()),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{Uses, built_ins, item};
    use crate::{config::BuiltinImages, environments::builtin::EngineImage};
    use std::collections::BTreeMap;
    use tqa_types::{MIN_RUNTIME_REVISION, RUNTIME_REVISION_LABEL};

    fn image(id: &str, tag: &str, name: &str, capabilities: &str) -> EngineImage {
        let revision = MIN_RUNTIME_REVISION.to_string();
        EngineImage {
            id: format!("sha256:{id:0>64}"),
            tags: vec![tag.to_owned()],
            created: 100,
            size_bytes: 1 << 30,
            labels: [
                (RUNTIME_REVISION_LABEL, revision.as_str()),
                ("org.tensorquay.environment", name),
                ("org.tensorquay.environment.capabilities", capabilities),
            ]
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect(),
        }
    }

    // TEST: GW-U-197
    #[test]
    fn gw_u_197_the_image_list_says_a_built_ins_own_size() -> Result<(), crate::GatewayError> {
        // A hand check before 17.0: Desktop added again from its card took the defaults (1 CPU, 512 MiB), since the
        // list never said Desktop's own size. A deleted Desktop's build says it; Workstation and own images say null.
        let images = [
            image(
                "d",
                "tqa-runtime:desktop",
                "desktop",
                "shell,python,node,build,browser,data,desktop",
            ),
            image(
                "w",
                "tqa-runtime:workstation",
                "workstation",
                "shell,python,node,build,browser,data",
            ),
            image(
                "e",
                "team/desktop-tools:1",
                "desktop",
                "shell,python,node,build,browser,data,desktop",
            ),
        ];
        let pinned = BuiltinImages::default();
        let found = built_ins(&images, &pinned);
        let install = Uses {
            templates: Vec::new(),
            file: Vec::new(),
            deleted: vec!["desktop".to_owned()],
            sandboxes: BTreeMap::new(),
        };
        let listed = images
            .iter()
            .map(|each| item(each, &found, &install, &pinned))
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(listed[0]["built_in"], "not_added");
        assert_eq!(
            listed[0]["sizes"],
            serde_json::json!({"cpus": 2.0, "memory_mib": 4096, "pids": 1024}),
            "{}",
            listed[0]
        );
        assert!(listed[1]["sizes"].is_null(), "{}", listed[1]);
        assert!(
            listed[2]["built_in"].is_null() && listed[2]["sizes"].is_null(),
            "{}",
            listed[2]
        );
        Ok(())
    }
}
