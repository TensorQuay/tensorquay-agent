//! GW-I-278, GW-I-279 (the c16i Desktop resize and delete, 14 Oct 2026): a customised built-in is still checked
//! against the build it was customised from. Desktop is built FROM Workstation and starts its own init, so an
//! administrator's size change that took it out of the check's bases had it refused for its entrypoint and gone from
//! the picker. A deleted Desktop stays deleted, as the other built-ins do.
#[path = "support/builtin_host.rs"]
mod builtin_host;
#[path = "support/fixture.rs"]
mod support;

use std::sync::Mutex;

use async_trait::async_trait;
use builtin_host::{
    BUILT, DERIVED, Engine, LATER, WORKSTATION, admin_item, checked, chosen_minimal, host, image,
    offered, offered_once, reset,
};
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{TestResult, call};
use tqa_gateway::{CheckOutcome, CheckReason, CheckRequest, TemplateCheck};
use tqa_sandbox::template::secret_files::SecretFiles;
use tqa_types::MIN_RUNTIME_REVISION;

const DESKTOP: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const DESKTOP_LABELS: (&str, &str, &str) = (
    "desktop",
    "Desktop",
    "shell,python,node,build,browser,data,desktop",
);

/// The engine side of the checks for Workstation and Desktop as `build-images.sh --desktop` builds them: Desktop is
/// built FROM Workstation and starts its own init (`tqa-desktop-init`), Workstation and an image built FROM it start
/// the runtime. As `check_image` decides (SBX-U-007, SBX-U-017): an image is checked against the most specific base
/// among the request's bases, and itself is the most specific; Desktop checked against Workstation is refused for its
/// entrypoint. The requests' bases are kept.
struct EntrypointChecks {
    asked: Mutex<Vec<Asked>>,
}

/// One check asked: the template and its bases (name, image).
type Asked = (String, Vec<(String, String)>);

#[async_trait]
impl TemplateCheck for EntrypointChecks {
    async fn check(&self, request: &CheckRequest<'_>) -> CheckOutcome {
        if let Ok(mut asked) = self.asked.lock() {
            asked.push((request.name.to_owned(), request.bases.to_vec()));
        }
        let base_named = |image: &str| {
            request
                .bases
                .iter()
                .find(|(_, base)| base == image)
                .map(|(name, _)| name.clone())
        };
        let verified = |base: String| CheckOutcome::Verified {
            warnings: SecretFiles::default(),
            base,
            image_id: request.image.to_owned(),
        };
        if let Some(itself) = base_named(request.image) {
            return verified(itself);
        }
        match (request.image, base_named(WORKSTATION)) {
            (DESKTOP, Some(_)) => CheckOutcome::Refused(CheckReason::ImageEntrypoint),
            (DERIVED, Some(workstation)) => verified(workstation),
            _ => CheckOutcome::Refused(CheckReason::NotDerived),
        }
    }
}

// TEST: GW-I-278
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_278_a_customised_desktop_is_checked_against_its_built_in_and_stays_offered(
    pool: PgPool,
) -> TestResult {
    // The c16i repro (14 Oct): an administrator gave Desktop 6 GB; the edit took it out of the check's bases, so its
    // image was checked against Workstation, refused for its entrypoint (`image_entrypoint`), and gone from the picker.
    Box::pin(support::run_restarting_with(pool, None, &chosen_minimal(), async |f| {
        let (_, cookie) = f.admin().await?;
        let checks = std::sync::Arc::new(EntrypointChecks { asked: Mutex::new(Vec::new()) });
        f.gateway.start_template_checks(checks.clone());
        let mut images = host(WORKSTATION, BUILT);
        images.push(image(DESKTOP, &["tqa-runtime:desktop"], BUILT, DESKTOP_LABELS, MIN_RUNTIME_REVISION));
        let engine = Engine::new(images.clone());
        f.gateway.use_environment_images(engine.clone());
        f.gateway.built_in_environments_now().await?;
        assert_eq!(checked(f, &cookie, "desktop").await?, "verified");
        let fresh = admin_item(f, &cookie, "desktop").await?.ok_or("desktop")?;
        assert_eq!((&fresh["customised"], &fresh["memory_mib"]), (&json!(false), &json!(4096)), "{fresh}");

        // The administrator's resize, the same image: customised, checked against Desktop's own build, and offered.
        let mut edit = json!({
            "display_name": "Desktop", "image": DESKTOP, "capabilities": fresh["capabilities"].clone(),
            "cpus": 2, "memory_mib": 6144, "version": fresh["version"].clone(),
        });
        let (status, edited) = call(f, Method::PUT, "/api/v1/admin/templates/desktop", &cookie, (Some(edit.clone()), None)).await?;
        assert_eq!((status, &edited["customised"]), (200, &json!(true)), "{edited}");
        assert_eq!(checked(f, &cookie, "desktop").await?, "verified");
        let item = admin_item(f, &cookie, "desktop").await?.ok_or("desktop")?;
        assert_eq!((&item["memory_mib"], &item["check"]["reason"], &item["base"]), (&json!(6144), &Value::Null, &json!("desktop")));
        let asked = checks.asked.lock().map_err(|_| "lock")?.clone();
        let (_, bases) = asked.iter().rev().find(|(name, _)| name == "desktop").ok_or("desktop asked")?;
        assert!(bases.contains(&("desktop".to_owned(), DESKTOP.to_owned())), "{bases:?}: its built-in stays a base");
        assert!(offered_once(f, &cookie, "desktop").await?.iter().any(|item| item.0 == "desktop"));

        // A rebuild doesn't move the customised Desktop, and its base stays the build it was customised from.
        let rebuilt = "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
        let mut later = host(WORKSTATION, BUILT);
        later.push(image(rebuilt, &["tqa-runtime:desktop"], LATER, DESKTOP_LABELS, MIN_RUNTIME_REVISION));
        later.push(image(DESKTOP, &[], BUILT, DESKTOP_LABELS, MIN_RUNTIME_REVISION));
        engine.set(Some(later.clone()));
        f.gateway.built_in_environments_now().await?;
        let item = admin_item(f, &cookie, "desktop").await?.ok_or("desktop")?;
        assert_eq!((&item["image"], &item["check"]["state"]), (&json!(DESKTOP), &json!("verified")));

        // An image change is checked as before: an image built FROM Workstation derives from Workstation.
        edit["image"] = json!(DERIVED);
        edit["version"] = item["version"].clone();
        let (status, edited) = call(f, Method::PUT, "/api/v1/admin/templates/desktop", &cookie, (Some(edit.clone()), None)).await?;
        assert_eq!(status, 200, "{edited}");
        assert_eq!(checked(f, &cookie, "desktop").await?, "verified");
        assert_eq!(admin_item(f, &cookie, "desktop").await?.ok_or("desktop")?["base"], json!("workstation"));

        // Reset to built-in: the current build, as `builtin`, verified against itself and offered.
        let version = admin_item(f, &cookie, "desktop").await?.ok_or("desktop")?["version"].as_i64().ok_or("version")?;
        let (status, back) = reset(f, &cookie, "desktop", version, "desktop-reset").await?;
        assert_eq!(status, 200, "{back}");
        assert_eq!((&back["last_set_by"], &back["customised"], &back["image"]), (&json!("builtin"), &json!(false), &json!(rebuilt)));
        assert_eq!(checked(f, &cookie, "desktop").await?, "verified");
        assert!(offered_once(f, &cookie, "desktop").await?.iter().any(|item| item.0 == "desktop"));
        Ok(())
    }))
    .await
}

// TEST: GW-I-278
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_278_a_desktop_refused_before_the_fix_is_checked_again_at_the_next_start(
    pool: PgPool,
) -> TestResult {
    // An install that took 16i's refusal: a customised Desktop on the image its tag still names, with no recorded
    // built-in base. The next registration (a gateway start) records that build as its base and checks it again.
    Box::pin(support::run_restarting_with(pool, None, &chosen_minimal(), async |f| {
        let (_, cookie) = f.admin().await?;
        let checks = std::sync::Arc::new(EntrypointChecks { asked: Mutex::new(Vec::new()) });
        f.gateway.start_template_checks(checks.clone());
        let mut images = host(WORKSTATION, BUILT);
        images.push(image(DESKTOP, &["tqa-runtime:desktop"], BUILT, DESKTOP_LABELS, MIN_RUNTIME_REVISION));
        let engine = Engine::new(images);
        f.gateway.use_environment_images(engine.clone());
        f.gateway.built_in_environments_now().await?;
        assert_eq!(checked(f, &cookie, "desktop").await?, "verified");
        let fresh = admin_item(f, &cookie, "desktop").await?.ok_or("desktop")?;
        let edit = json!({
            "display_name": "Desktop", "image": DESKTOP, "capabilities": fresh["capabilities"].clone(),
            "cpus": 2, "memory_mib": 6144, "version": fresh["version"].clone(),
        });
        assert_eq!(call(f, Method::PUT, "/api/v1/admin/templates/desktop", &cookie, (Some(edit), None)).await?.0, 200);
        assert_eq!(checked(f, &cookie, "desktop").await?, "verified");
        // As 16i stored it: no built-in base, refused.
        sqlx::query("UPDATE templates SET builtin_base_image=NULL,check_state='refused',check_reason='image_entrypoint',checked_version=version WHERE name='desktop'")
            .execute(&f.pool)
            .await?;
        f.gateway.built_in_environments_now().await?;
        assert_eq!(checked(f, &cookie, "desktop").await?, "verified");
        let item = admin_item(f, &cookie, "desktop").await?.ok_or("desktop")?;
        assert_eq!((&item["memory_mib"], &item["customised"], &item["base"]), (&json!(6144), &json!(true), &json!("desktop")));
        assert!(offered_once(f, &cookie, "desktop").await?.iter().any(|item| item.0 == "desktop"));
        Ok(())
    }))
    .await
}

// TEST: GW-I-279
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_279_a_deleted_desktop_stays_deleted_and_can_be_added_again(
    pool: PgPool,
) -> TestResult {
    // As for the other built-ins (GW-I-145): deleting Desktop is final, through a rebuild and a restart, and an
    // administrator adds it again from its image. 16i's list of deletable built-ins lacked Desktop, so its delete failed.
    Box::pin(support::run_restarting_with(
        pool,
        None,
        &chosen_minimal(),
        async |f| {
            let (_, cookie) = f.admin().await?;
            let checks = std::sync::Arc::new(EntrypointChecks {
                asked: Mutex::new(Vec::new()),
            });
            f.gateway.start_template_checks(checks.clone());
            let mut images = host(WORKSTATION, BUILT);
            images.push(image(
                DESKTOP,
                &["tqa-runtime:desktop"],
                BUILT,
                DESKTOP_LABELS,
                MIN_RUNTIME_REVISION,
            ));
            let engine = Engine::new(images);
            f.gateway.use_environment_images(engine.clone());
            f.gateway.built_in_environments_now().await?;
            assert_eq!(checked(f, &cookie, "desktop").await?, "verified");
            let version =
                admin_item(f, &cookie, "desktop").await?.ok_or("desktop")?["version"].clone();
            let (status, body) = call(
                f,
                Method::DELETE,
                &format!("/api/v1/admin/templates/desktop?version={version}"),
                &cookie,
                (None, None),
            )
            .await?;
            assert_eq!(status, 204, "{body}");
            assert!(admin_item(f, &cookie, "desktop").await?.is_none());
            assert!(
                offered(f, &cookie)
                    .await?
                    .iter()
                    .all(|item| item.0 != "desktop")
            );

            // A rebuild, then a restart: still deleted.
            let rebuilt = "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
            let mut later = host(WORKSTATION, BUILT);
            later.push(image(
                rebuilt,
                &["tqa-runtime:desktop"],
                LATER,
                DESKTOP_LABELS,
                MIN_RUNTIME_REVISION,
            ));
            engine.set(Some(later));
            f.gateway.built_in_environments_now().await?;
            f.restart().await?;
            f.gateway.start_template_checks(checks.clone());
            f.gateway.use_environment_images(engine.clone());
            f.gateway.built_in_environments_now().await?;
            assert!(admin_item(f, &cookie, "desktop").await?.is_none());
            let (_, images) = call(
                f,
                Method::GET,
                "/api/v1/admin/environment-images",
                &cookie,
                (None, None),
            )
            .await?;
            let listed = images["items"]
                .as_array()
                .ok_or("items")?
                .iter()
                .find(|image| image["id"] == rebuilt)
                .ok_or("rebuilt")?;
            assert_eq!(
                listed["reason"],
                json!("deleted by an administrator"),
                "{listed}"
            );

            // Added again from its image, as the guide says: the administrator's own Desktop, verified and offered.
            let add = json!({
                "display_name": "Desktop", "image": rebuilt,
                "capabilities": ["shell", "python", "node", "build", "browser", "data", "desktop"],
            });
            let (status, added) = call(
                f,
                Method::PUT,
                "/api/v1/admin/templates/desktop",
                &cookie,
                (Some(add), None),
            )
            .await?;
            assert_eq!(
                (status, &added["last_set_by"]),
                (201, &json!("admin")),
                "{added}"
            );
            assert_eq!(checked(f, &cookie, "desktop").await?, "verified");
            assert!(
                offered_once(f, &cookie, "desktop")
                    .await?
                    .iter()
                    .any(|item| item.0 == "desktop")
            );
            Ok(())
        },
    ))
    .await
}
