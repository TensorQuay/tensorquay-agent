//! GW-I-145 (the built-in environments design): the ready environments register themselves. At start the gateway asks
//! the engine for the images the install built and adds each built-in (the image its canonical tag names, its label
//! agreeing) as an environment, with its capabilities, never as the default. An upgrade adds them the same way; a moved
//! tag is an audited update the list shows; an administrator's environment, the file's entry, a deletion and an engine
//! that doesn't answer are each left alone.
#[path = "support/builtin_host.rs"]
mod builtin_host;
#[path = "support/fixture.rs"]
mod support;

use std::sync::Mutex;

use async_trait::async_trait;
use builtin_host::{
    ADMIN_PIN, BUILT, DERIVED, Engine, LATER, OLD_DATA, REBUILT, Result, WORKSTATION,
    WORKSTATION_LABELS, admin_item, admin_items, checked, chosen_minimal, host, image, offered,
    offered_once, reset,
};
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};
use tqa_gateway::{CheckOutcome, CheckReason, CheckRequest, TemplateCheck};
use tqa_sandbox::docker::ImageRecord;
use tqa_sandbox::template::secret_files::SecretFiles;
use tqa_types::{ConversationId, MIN_RUNTIME_REVISION};

const NEWEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaab";

async fn audit(
    pool: &PgPool,
    item: &str,
) -> Result<Vec<(String, String, Option<i64>, Option<i64>)>> {
    Ok(sqlx::query_as(
        "SELECT actor_kind,action,before_version,after_version FROM admin_audit WHERE area='templates' AND item=$1 ORDER BY at, id",
    )
    .bind(item)
    .fetch_all(pool)
    .await?)
}

/// The image the task is pinned to.
async fn pinned(f: &Fixture, id: &ConversationId) -> Result<String> {
    Ok(
        sqlx::query_scalar("SELECT template_digest FROM conversations WHERE id=$1")
            .bind(uuid::Uuid::parse_str(id.as_str())?)
            .fetch_one(&f.pool)
            .await?,
    )
}

/// A conversation in `template`, made through the API as a person would.
async fn conversation_in(f: &Fixture, cookie: &str, template: &str) -> Result<ConversationId> {
    let response = f
        .client
        .post(f.url("/api/v1/conversations"))
        .header("origin", f.origin())
        .header("cookie", cookie)
        .header("idempotency-key", format!("in-{template}"))
        .json(&json!({"title": "In it", "model_route": "local", "template": template}))
        .send()
        .await?;
    assert_eq!(response.status(), 201);
    Ok(response.json::<Value>().await?["id"]
        .as_str()
        .ok_or("id")?
        .parse()?)
}

// TEST: GW-I-145
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_145_an_install_gains_its_built_ins_and_a_rebuild_is_an_audited_update(
    pool: PgPool,
) -> TestResult {
    Box::pin(support::run_restarting_with(pool, None, "", async |f| {
        let (_, cookie) = f.admin().await?;
        // Before this version (or before the images were built) only the file's Minimal is there.
        assert_eq!(offered(f, &cookie).await?.iter().map(|item| item.0.as_str()).collect::<Vec<_>>(), ["minimal"]);
        // The upgrade: the next start finds Workstation on the host. Nobody adds it.
        f.restart().await?;
        let engine = Engine::new(host(WORKSTATION, BUILT));
        f.gateway.use_environment_images(engine.clone());
        f.gateway.built_in_environments_now().await?;
        // Registered at once, and offered with the default once its checks pass (GW-I-172, GW-I-174).
        assert_eq!(admin_item(f, &cookie, "workstation").await?.ok_or("workstation")?["check"]["state"], json!("unchecked"));
        assert_eq!(
            offered(f, &cookie).await?.iter().map(|item| item.0.as_str()).collect::<Vec<_>>(),
            ["minimal"],
            "Workstation is not offered while Minimal is still the default"
        );
        f.gateway.start_template_checks(std::sync::Arc::new(MicroVmChecks { built: Mutex::new(true) }));
        assert_eq!(checked(f, &cookie, "workstation").await?, "verified");
        assert_eq!(
            offered_once(f, &cookie, "workstation").await?,
            [
                ("workstation".to_owned(), json!(["shell", "python", "node", "build", "browser"]), json!(true)),
                ("minimal".to_owned(), json!(["shell", "python"]), json!(false)),
            ],
            "Workstation, verified, becomes the default over the file's implicit Minimal (GW-I-154); the \
             derived and the too-old image are not added"
        );
        let item = admin_item(f, &cookie, "workstation").await?.ok_or("workstation")?;
        assert_eq!(
            (&item["last_set_by"], &item["image"], &item["display_name"], &item["version"]),
            (&json!("builtin"), &json!(WORKSTATION), &json!("Workstation"), &json!(2))
        );
        assert_eq!(item["builtin"], json!({"image_created": "2026-09-01T00:00:00Z", "updated_at": null}));
        assert_eq!(admin_item(f, &cookie, "minimal").await?.ok_or("minimal")?["builtin"], Value::Null);
        // Registered at version 1, then made the default once verified (an audited move to version 2).
        let registered: [(String, String, Option<i64>, Option<i64>); 2] = [("builtin".into(), "create".into(), None, Some(1)), ("builtin".into(), "replace".into(), Some(1), Some(2))];
        assert_eq!(audit(&f.pool, "workstation").await?, registered);
        // The next start with the same images changes nothing.
        f.gateway.built_in_environments_now().await?;
        assert_eq!(audit(&f.pool, "workstation").await?, registered);

        // A task in Workstation, pinned to its image, waiting for its next session.
        let task = conversation_in(f, &cookie, "workstation").await?;
        assert_eq!(pinned(f, &task).await?, WORKSTATION);

        // A rebuild moves the tag: an audited update, shown on the list, never silent.
        engine.set(Some(host(REBUILT, LATER)));
        f.gateway.built_in_environments_now().await?;
        let item = admin_item(f, &cookie, "workstation").await?.ok_or("workstation")?;
        assert_eq!((&item["image"], &item["version"], &item["last_set_by"]), (&json!(REBUILT), &json!(3), &json!("builtin")));
        assert_eq!(item["builtin"]["image_created"], json!("2026-10-01T00:00:00Z"));
        assert!(item["builtin"]["updated_at"].is_string(), "{item}");
        assert_eq!(
            audit(&f.pool, "workstation").await?,
            [registered[0].clone(), registered[1].clone(), ("builtin".into(), "replace".into(), Some(2), Some(3))]
        );
        // The task moves at its next session (ADR-0032 decision 7).
        f.gateway.sandboxes().ensure(&task).await?;
        assert_eq!((f.backend.created(&task).ok_or("no sandbox")?.0, pinned(f, &task).await?), (REBUILT.to_owned(), REBUILT.to_owned()));
        let values: String = sqlx::query_scalar(
            "SELECT coalesce(string_agg(row_to_json(a)::text, ''), '') FROM admin_audit a WHERE area='templates'",
        )
        .fetch_one(&f.pool)
        .await?;
        assert!(!values.contains(REBUILT), "no values in the audit");
        Ok(())
    }))
    .await
}

// TEST: GW-I-145
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_145_an_administrators_environment_and_the_files_entry_keep_their_name(
    pool: PgPool,
) -> TestResult {
    let file = format!(
        "[[templates]]\nname='minimal'\ndisplay_name='Minimal'\nimage='{ADMIN_PIN}'\ncapabilities=['shell']\ndefault=true\n\n\
         [[templates]]\nname='data'\ndisplay_name='Our data'\nimage='{ADMIN_PIN}'\ncapabilities=['shell']\n"
    );
    Box::pin(support::run_restarting_with(pool, None, &file, async |f| {
        let (_, cookie) = f.admin().await?;
        // An administrator made their own "workstation" before this version.
        let body = json!({"display_name": "Ours", "image": ADMIN_PIN, "capabilities": ["shell"]});
        let (status, _) = call(
            f,
            Method::PUT,
            "/api/v1/admin/templates/workstation",
            &cookie,
            (Some(body), None),
        )
        .await?;
        assert_eq!(status, 201);
        // This host's Data is current (no older build tagged tqa-runtime:data), so it is a built-in the file must win over.
        let mut images: Vec<ImageRecord> = host(WORKSTATION, BUILT)
            .into_iter()
            .filter(|image| image.id != OLD_DATA)
            .collect();
        images.push(image(
            REBUILT,
            &["tqa-runtime:data"],
            LATER,
            ("data", "Data", "shell,python"),
            MIN_RUNTIME_REVISION,
        ));
        let engine = Engine::new(images);
        f.gateway.use_environment_images(engine.clone());
        f.gateway.built_in_environments_now().await?;
        let workstation = admin_item(f, &cookie, "workstation")
            .await?
            .ok_or("workstation")?;
        assert_eq!(
            (
                &workstation["last_set_by"],
                &workstation["image"],
                &workstation["display_name"]
            ),
            (&json!("admin"), &json!(ADMIN_PIN), &json!("Ours"))
        );
        let data = admin_item(f, &cookie, "data").await?.ok_or("data")?;
        assert_eq!(
            (&data["last_set_by"], &data["image"]),
            (&json!("seed"), &json!(ADMIN_PIN)),
            "the file wins"
        );
        // The file's entry wins even after an administrator deleted it.
        let (status, _) = call(
            f,
            Method::DELETE,
            "/api/v1/admin/templates/data?version=1",
            &cookie,
            (None, None),
        )
        .await?;
        assert_eq!(status, 204);
        f.gateway.built_in_environments_now().await?;
        assert!(admin_item(f, &cookie, "data").await?.is_none());
        for name in ["workstation", "data"] {
            assert!(
                audit(&f.pool, name)
                    .await?
                    .iter()
                    .all(|row| row.0 != "builtin"),
                "{name}: nothing registered over it"
            );
        }
        Ok(())
    }))
    .await
}

// TEST: GW-I-145
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_145_a_deleted_built_in_stays_deleted_by_name_even_after_a_rebuild(
    pool: PgPool,
) -> TestResult {
    Box::pin(support::run_restarting_with(
        pool,
        None,
        &chosen_minimal(),
        async |f| {
            let (_, cookie) = f.admin().await?;
            let engine = Engine::new(host(WORKSTATION, BUILT));
            f.gateway.use_environment_images(engine.clone());
            f.gateway.built_in_environments_now().await?;
            let (status, _) = call(
                f,
                Method::DELETE,
                "/api/v1/admin/templates/workstation?version=1",
                &cookie,
                (None, None),
            )
            .await?;
            assert_eq!(status, 204);
            // A rebuild, then a restart: still deleted.
            engine.set(Some(host(REBUILT, LATER)));
            f.gateway.built_in_environments_now().await?;
            f.restart().await?;
            f.gateway.use_environment_images(engine.clone());
            f.gateway.built_in_environments_now().await?;
            assert!(admin_item(f, &cookie, "workstation").await?.is_none());
            assert!(
                offered(f, &cookie)
                    .await?
                    .iter()
                    .all(|item| item.0 != "workstation")
            );
            Ok(())
        },
    ))
    .await
}

// TEST: GW-I-145
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_145_an_engine_that_does_not_answer_at_start_keeps_the_built_ins_it_had(
    pool: PgPool,
) -> TestResult {
    Box::pin(support::run_restarting_with(pool, None, "", async |f| {
        let (_, cookie) = f.admin().await?;
        f.gateway
            .start_template_checks(std::sync::Arc::new(MicroVmChecks {
                built: Mutex::new(true),
            }));
        let engine = Engine::new(host(WORKSTATION, BUILT));
        f.gateway.use_environment_images(engine.clone());
        f.gateway.built_in_environments_now().await?;
        assert_eq!(checked(f, &cookie, "workstation").await?, "verified");
        engine.set(None);
        f.restart().await?;
        f.gateway.use_environment_images(engine.clone());
        f.gateway.built_in_environments_now().await?;
        let item = admin_item(f, &cookie, "workstation")
            .await?
            .ok_or("workstation")?;
        assert_eq!(
            (&item["image"], &item["version"]),
            (&json!(WORKSTATION), &json!(2))
        );
        assert!(
            offered(f, &cookie)
                .await?
                .iter()
                .any(|item| item.0 == "workstation")
        );
        // Created, then made the default once verified; the start without an engine changed nothing.
        assert_eq!(audit(&f.pool, "workstation").await?.len(), 2);
        Ok(())
    }))
    .await
}

// TEST: GW-I-145
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_145_an_edited_built_in_is_customised_until_it_is_reset(pool: PgPool) -> TestResult {
    Box::pin(support::run_restarting_with(pool, None, &chosen_minimal(), async |f| {
        let (_, cookie) = f.admin().await?;
        let engine = Engine::new(host(WORKSTATION, BUILT));
        f.gateway.use_environment_images(engine.clone());
        f.gateway.built_in_environments_now().await?;
        assert_eq!(admin_item(f, &cookie, "workstation").await?.ok_or("workstation")?["customised"], json!(false));
        // An administrator edits Workstation: it is theirs now, and says so.
        let edit = json!({"display_name": "Our workstation", "image": WORKSTATION, "capabilities": ["shell", "python"], "version": 1});
        let (status, edited) = call(f, Method::PUT, "/api/v1/admin/templates/workstation", &cookie, (Some(edit), None)).await?;
        assert_eq!(status, 200, "{edited}");
        assert_eq!(
            (&edited["last_set_by"], &edited["customised"], &edited["builtin"]),
            (&json!("admin"), &json!(true), &Value::Null)
        );
        // A rebuild no longer moves it, and the image list says why.
        engine.set(Some(host(REBUILT, LATER)));
        f.gateway.built_in_environments_now().await?;
        let item = admin_item(f, &cookie, "workstation").await?.ok_or("workstation")?;
        assert_eq!((&item["image"], &item["version"], &item["customised"]), (&json!(WORKSTATION), &json!(2), &json!(true)));
        let (_, images) = call(f, Method::GET, "/api/v1/admin/environment-images", &cookie, (None, None)).await?;
        let rebuilt = images["items"].as_array().ok_or("items")?.iter().find(|image| image["id"] == REBUILT).ok_or("rebuilt")?;
        assert_eq!(
            (&rebuilt["built_in"], &rebuilt["reason"]),
            (&json!("not_added"), &json!("customised by an administrator: rebuilds no longer update it"))
        );

        // Refusals, each with its own code.
        assert_eq!(reset(f, &cookie, "workstation", 1, "stale").await?.1["error"]["code"], "conflict");
        assert_eq!(reset(f, &cookie, "minimal", 1, "minimal").await?.1["error"]["code"], "not_customised");
        engine.set(None);
        let (status, body) = reset(f, &cookie, "workstation", 2, "engine-down").await?;
        assert_eq!((status, &body["error"]["code"]), (503, &json!("engine_unavailable")));
        engine.set(Some(host(REBUILT, LATER).into_iter().filter(|image| image.id != REBUILT).collect()));
        let (status, body) = reset(f, &cookie, "workstation", 2, "image-gone").await?;
        assert_eq!((status, &body["error"]["code"]), (409, &json!("builtin_image_missing")));

        // Reset to built-in: the current build, its name and tools, handed back to the rebuilds; audited.
        engine.set(Some(host(REBUILT, LATER)));
        let (status, back) = reset(f, &cookie, "workstation", 2, "reset").await?;
        assert_eq!(status, 200, "{back}");
        assert_eq!(
            (&back["last_set_by"], &back["customised"], &back["image"], &back["display_name"], &back["version"]),
            (&json!("builtin"), &json!(false), &json!(REBUILT), &json!("Workstation"), &json!(3))
        );
        assert_eq!(back["capabilities"], json!(["shell", "python", "node", "build", "browser"]));
        assert_eq!(back["builtin"]["image_created"], json!("2026-10-01T00:00:00Z"));
        assert_eq!(reset(f, &cookie, "workstation", 2, "reset").await?, (200, back.clone()), "a retry replays");
        let detail: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT actor_kind,detail FROM admin_audit WHERE area='templates' AND item='workstation' AND after_version=3",
        )
        .fetch_all(&f.pool)
        .await?;
        assert_eq!(detail, [("admin".to_owned(), Some("admin->builtin".to_owned()))]);
        // The next rebuild moves it again.
        engine.set(Some(host(NEWEST, LATER + 86_400)));
        f.gateway.built_in_environments_now().await?;
        let item = admin_item(f, &cookie, "workstation").await?.ok_or("workstation")?;
        assert_eq!((&item["image"], &item["version"]), (&json!(NEWEST), &json!(4)));

        // Deleting a customised built-in is final too.
        let edit = json!({"display_name": "Ours", "image": DERIVED, "capabilities": ["shell"], "version": 4});
        assert_eq!(call(f, Method::PUT, "/api/v1/admin/templates/workstation", &cookie, (Some(edit), None)).await?.0, 200);
        let (status, _) = call(f, Method::DELETE, "/api/v1/admin/templates/workstation?version=5", &cookie, (None, None)).await?;
        assert_eq!(status, 204);
        f.gateway.built_in_environments_now().await?;
        assert!(admin_item(f, &cookie, "workstation").await?.is_none());
        Ok(())
    }))
    .await
}

async fn defaults(f: &Fixture, cookie: &str) -> Result<Vec<(String, Value)>> {
    Ok(admin_items(f, cookie)
        .await?
        .into_iter()
        .map(|item| {
            (
                item["name"].as_str().unwrap_or_default().to_owned(),
                item["is_default"].clone(),
            )
        })
        .collect())
}

// TEST: GW-I-154
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_154_workstation_becomes_the_default_over_the_implicit_minimal_and_keeps_it(
    pool: PgPool,
) -> TestResult {
    // New task's own suggestions need Workstation's tools; a quick start's default was the file's implicit Minimal.
    Box::pin(support::run_restarting_with(pool, None, "", async |f| {
        let (_, cookie) = f.admin().await?;
        // Its checks pass on this backend (GW-I-172 covers one where they don't).
        f.gateway.start_template_checks(std::sync::Arc::new(MicroVmChecks { built: Mutex::new(true) }));
        let engine = Engine::new(host(WORKSTATION, BUILT));
        f.gateway.use_environment_images(engine.clone());
        f.gateway.built_in_environments_now().await?;
        assert_eq!(checked(f, &cookie, "workstation").await?, "verified");
        assert_eq!(defaults(f, &cookie).await?, [("minimal".to_owned(), json!(false)), ("workstation".to_owned(), json!(true))]);
        let moved: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT actor_kind,item,action FROM admin_audit WHERE area='templates' AND actor_kind='builtin' ORDER BY at, id",
        )
        .fetch_all(&f.pool)
        .await?;
        assert!(moved.contains(&("builtin".into(), "minimal".into(), "replace".into())), "{moved:?}: the move is audited");
        // An upgrade rebuilds the runtime: the file's Minimal takes its new image, never the default back.
        f.set_runtime_image("example/runtime@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")?;
        f.restart().await?;
        f.gateway.use_environment_images(engine.clone());
        f.gateway.built_in_environments_now().await?;
        let minimal = admin_item(f, &cookie, "minimal").await?.ok_or("minimal")?;
        assert_eq!(minimal["image"], json!("example/runtime@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"));
        assert_eq!(defaults(f, &cookie).await?, [("minimal".to_owned(), json!(false)), ("workstation".to_owned(), json!(true))]);
        Ok(())
    }))
    .await
}

// TEST: GW-I-154
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_154_a_chosen_default_is_kept_and_minimal_is_the_fallback(pool: PgPool) -> TestResult {
    // The file names its own default: Workstation is added but doesn't take it.
    let file = format!(
        "[[templates]]\nname='minimal'\ndisplay_name='Minimal'\nimage='{ADMIN_PIN}'\ncapabilities=['shell']\ndefault=true\n"
    );
    Box::pin(support::run_restarting_with(pool, None, &file, async |f| {
        let (_, cookie) = f.admin().await?;
        // No Workstation on the host yet: Minimal stays the default.
        let engine = Engine::new(Vec::new());
        f.gateway.use_environment_images(engine.clone());
        f.gateway.built_in_environments_now().await?;
        assert_eq!(
            defaults(f, &cookie).await?,
            [("minimal".to_owned(), json!(true))]
        );
        engine.set(Some(host(WORKSTATION, BUILT)));
        f.gateway.built_in_environments_now().await?;
        assert_eq!(
            defaults(f, &cookie).await?,
            [
                ("minimal".to_owned(), json!(true)),
                ("workstation".to_owned(), json!(false))
            ]
        );
        Ok(())
    }))
    .await
}

// TEST: GW-I-154
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_154_an_administrators_environments_keep_their_default(pool: PgPool) -> TestResult {
    Box::pin(support::run_restarting_with(pool, None, "", async |f| {
        let (_, cookie) = f.admin().await?;
        // An administrator already set environments up (their own one, not the default): their choices stand.
        let body = json!({"display_name": "Ours", "image": ADMIN_PIN, "capabilities": ["shell"]});
        assert_eq!(
            call(
                f,
                Method::PUT,
                "/api/v1/admin/templates/ours",
                &cookie,
                (Some(body), None)
            )
            .await?
            .0,
            201
        );
        f.gateway
            .use_environment_images(Engine::new(host(WORKSTATION, BUILT)));
        f.gateway.built_in_environments_now().await?;
        let defaults = defaults(f, &cookie).await?;
        assert!(
            defaults.contains(&("minimal".to_owned(), json!(true))),
            "{defaults:?}"
        );
        assert!(
            defaults.contains(&("workstation".to_owned(), json!(false))),
            "{defaults:?}"
        );
        Ok(())
    }))
    .await
}

/// The engine side of the template checks: Workstation is refused as on a microVM install without its rootfs
/// (`rootfs_missing`) until `built` says its image was built for microVMs; every other template verifies.
struct MicroVmChecks {
    built: Mutex<bool>,
}

#[async_trait]
impl TemplateCheck for MicroVmChecks {
    async fn check(&self, request: &CheckRequest<'_>) -> CheckOutcome {
        let built = self.built.lock().is_ok_and(|built| *built);
        if request.name == "workstation" && !built {
            return CheckOutcome::Refused(CheckReason::RootfsMissing);
        }
        CheckOutcome::Verified {
            warnings: SecretFiles::default(),
            base: request.name.to_owned(),
            image_id: request.image.to_owned(),
        }
    }
}

// TEST: GW-I-172
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_172_workstation_takes_the_default_only_once_its_checks_pass(
    pool: PgPool,
) -> TestResult {
    // E2E-VM-013's cause: on a microVM install whose Workstation rootfs isn't built, Workstation took the default
    // and every task created without a template was refused (409).
    Box::pin(support::run_restarting_with(pool, None, "", async |f| {
        let (_, cookie) = f.admin().await?;
        let checks = std::sync::Arc::new(MicroVmChecks { built: Mutex::new(false) });
        f.gateway.start_template_checks(checks.clone());
        let engine = Engine::new(host(WORKSTATION, BUILT));
        f.gateway.use_environment_images(engine.clone());
        f.gateway.built_in_environments_now().await?;
        assert_eq!(checked(f, &cookie, "workstation").await?, "refused");
        assert_eq!(defaults(f, &cookie).await?, [("minimal".to_owned(), json!(true)), ("workstation".to_owned(), json!(false))]);
        // A task without a template starts on Minimal.
        let body = json!({"title": "Files", "model_route": "local"});
        let (status, created) = call(f, Method::POST, "/api/v1/conversations", &cookie, (Some(body), Some("vm-013"))).await?;
        assert_eq!((status, &created["template"]), (201, &json!("minimal")), "{created}");
        // Its image is built for microVMs and the next build is checked: now it passes, and Workstation takes the
        // default, audited.
        *checks.built.lock().map_err(|_| "lock")? = true;
        engine.set(Some(host(REBUILT, LATER)));
        f.gateway.built_in_environments_now().await?;
        assert_eq!(checked(f, &cookie, "workstation").await?, "verified");
        assert_eq!(defaults(f, &cookie).await?, [("minimal".to_owned(), json!(false)), ("workstation".to_owned(), json!(true))]);
        let moved: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT actor_kind,item,action FROM admin_audit WHERE area='templates' AND actor_kind='builtin' ORDER BY at, id",
        )
        .fetch_all(&f.pool)
        .await?;
        assert!(moved.contains(&("builtin".into(), "minimal".into(), "replace".into())), "{moved:?}: the move is audited");
        Ok(())
    }))
    .await
}

/// Checks that wait for `open` before answering, as a real engine takes seconds: the window a first task can land in.
struct HeldChecks {
    open: tokio::sync::Semaphore,
}

#[async_trait]
impl TemplateCheck for HeldChecks {
    async fn check(&self, request: &CheckRequest<'_>) -> CheckOutcome {
        if let Ok(permit) = self.open.acquire().await {
            permit.forget();
        }
        CheckOutcome::Verified {
            warnings: SecretFiles::default(),
            base: request.name.to_owned(),
            image_id: request.image.to_owned(),
        }
    }
}

// TEST: GW-I-174
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_174_a_first_task_made_once_workstation_is_offered_starts_in_it(
    pool: PgPool,
) -> TestResult {
    // The first-hour kit on a fresh Docker install: it waits until the public list offers Workstation, then makes the
    // first task without naming an environment, and expects Workstation's tools.
    Box::pin(support::run_restarting_with(pool, None, "", async |f| {
        let (_, cookie) = f.admin().await?;
        let checks = std::sync::Arc::new(HeldChecks {
            open: tokio::sync::Semaphore::new(0),
        });
        f.gateway.start_template_checks(checks.clone());
        f.gateway
            .use_environment_images(Engine::new(host(WORKSTATION, BUILT)));
        f.gateway.built_in_environments_now().await?;
        // While its check runs, it isn't offered: the picker and the default must agree (GW-I-172).
        assert_eq!(offered_names(f, &cookie).await?, ["minimal"]);
        // The engine answers; the kit waits until the list offers Workstation, then makes its first task.
        checks.open.add_permits(8);
        let mut offered = false;
        for _ in 0..100 {
            if offered_names(f, &cookie)
                .await?
                .contains(&"workstation".to_owned())
            {
                offered = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(offered, "Workstation is offered once its check passes");
        let body = json!({"title": "Fresh install check", "model_route": "local"});
        let (status, created) = call(
            f,
            Method::POST,
            "/api/v1/conversations",
            &cookie,
            (Some(body), Some("first")),
        )
        .await?;
        assert_eq!(
            (status, &created["template"]),
            (201, &json!("workstation")),
            "{created}"
        );
        Ok(())
    }))
    .await
}

/// The public list's environment names.
async fn offered_names(f: &Fixture, cookie: &str) -> Result<Vec<String>> {
    let (_, list) = call(f, Method::GET, "/api/v1/templates", cookie, (None, None)).await?;
    Ok(list["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item["name"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default())
}

// TEST: GW-I-174
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_174_the_default_workstation_stays_offered_while_a_rebuild_is_checked(
    pool: PgPool,
) -> TestResult {
    // A rebuild makes the default's new version unchecked; hiding it until the check would refuse every task made
    // without an environment meanwhile.
    Box::pin(support::run_restarting_with(pool, None, "", async |f| {
        let (_, cookie) = f.admin().await?;
        // The first round of checks (Minimal and Workstation) passes; the rebuild's check is held.
        let checks = std::sync::Arc::new(HeldChecks {
            open: tokio::sync::Semaphore::new(2),
        });
        f.gateway.start_template_checks(checks.clone());
        let engine = Engine::new(host(WORKSTATION, BUILT));
        f.gateway.use_environment_images(engine.clone());
        f.gateway.built_in_environments_now().await?;
        assert_eq!(checked(f, &cookie, "workstation").await?, "verified");
        assert_eq!(
            defaults(f, &cookie).await?,
            [
                ("minimal".to_owned(), json!(false)),
                ("workstation".to_owned(), json!(true))
            ]
        );
        engine.set(Some(host(REBUILT, LATER)));
        f.gateway.built_in_environments_now().await?;
        let item = admin_item(f, &cookie, "workstation")
            .await?
            .ok_or("workstation")?;
        assert_eq!(
            (&item["image"], &item["check"]["state"], &item["is_default"]),
            (&json!(REBUILT), &json!("unchecked"), &json!(true))
        );
        assert!(
            offered_names(f, &cookie)
                .await?
                .contains(&"workstation".to_owned())
        );
        let body = json!({"title": "During the rebuild's check", "model_route": "local"});
        let (status, created) = call(
            f,
            Method::POST,
            "/api/v1/conversations",
            &cookie,
            (Some(body), Some("rebuilt")),
        )
        .await?;
        assert_eq!(
            (status, &created["template"]),
            (201, &json!("workstation")),
            "{created}"
        );
        checks.open.add_permits(8);
        assert_eq!(checked(f, &cookie, "workstation").await?, "verified");
        Ok(())
    }))
    .await
}

const PINNED: &str = "sha256:8888888888888888888888888888888888888888888888888888888888888888";
const DATA: &str = "sha256:9999999999999999999999999999999999999999999999999999999999999999";

// TEST: GW-I-190
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_190_a_pinned_built_in_registers_its_image_not_the_one_the_shared_tag_names(
    pool: PgPool,
) -> TestResult {
    // The engine is shared: its `tqa-runtime:workstation` tag names another build; the image under test has no tag.
    let pins = format!("[runtime.builtin_images]\nworkstation='{PINNED}'\n");
    Box::pin(support::run_restarting_with(pool, None, &pins, async |f| {
        let (_, cookie) = f.admin().await?;
        let revision = MIN_RUNTIME_REVISION;
        let engine = Engine::new(vec![
            image(
                WORKSTATION,
                &["tqa-runtime:workstation"],
                LATER,
                WORKSTATION_LABELS,
                revision,
            ),
            image(PINNED, &[], BUILT, WORKSTATION_LABELS, revision),
            image(
                DATA,
                &["tqa-runtime:data"],
                BUILT,
                ("data", "Data", "shell,python"),
                revision,
            ),
        ]);
        f.gateway.use_environment_images(engine.clone());
        f.gateway.built_in_environments_now().await?;
        let workstation = admin_item(f, &cookie, "workstation")
            .await?
            .ok_or("workstation")?;
        assert_eq!(
            (&workstation["last_set_by"], &workstation["image"]),
            (&json!("builtin"), &json!(PINNED))
        );
        // An unpinned built-in still comes from its tag.
        assert_eq!(
            admin_item(f, &cookie, "data").await?.ok_or("data")?["image"],
            json!(DATA)
        );
        Ok(())
    }))
    .await
}

// TEST: GW-I-213
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_213_a_built_in_the_engine_couldnt_list_at_start_is_registered_once_it_answers(
    pool: PgPool,
) -> TestResult {
    Box::pin(support::run_restarting_with(pool, None, "", async |f| {
        let (_, cookie) = f.admin().await?;
        // The engine doesn't answer at start: nothing is registered, and nothing waits for it.
        let engine = Engine::new(host(WORKSTATION, BUILT));
        engine.set(None);
        f.gateway.use_environment_images(engine.clone());
        f.gateway.keep_registering_built_ins();
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        assert!(
            admin_item(f, &cookie, "workstation").await?.is_none(),
            "the engine hasn't answered"
        );
        // It answers later: the next retry (4 to 6 s after the first listing) registers Workstation, with no restart.
        engine.set(Some(host(WORKSTATION, BUILT)));
        for _ in 0..150 {
            if let Some(item) = admin_item(f, &cookie, "workstation").await? {
                assert_eq!(
                    (&item["image"], &item["last_set_by"]),
                    (&json!(WORKSTATION), &json!("builtin"))
                );
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        Err("Workstation was never registered after the engine answered".into())
    }))
    .await
}
