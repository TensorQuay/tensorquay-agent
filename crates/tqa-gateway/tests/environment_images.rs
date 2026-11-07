//! GW-I-146 (the built-in environments design): `GET /api/v1/admin/environment-images`, the images on the engine host
//! an administrator can pick an environment from, so nobody types an image ID. Each says what it is (from its labels),
//! whether this version can run it and why not in plain words, what uses it, and whether it is a built-in and added. An
//! engine that doesn't answer is a plain, retryable 503; the gateway never pulls.
#[path = "support/fixture.rs"]
mod support;

use std::{collections::BTreeMap, sync::Mutex};

use async_trait::async_trait;
use reqwest::Method;
use serde_json::{Value, json};
use support::{Fixture, TestResult, call};
use tqa_gateway::{
    CheckOutcome, CheckRequest, EngineUnavailable, EnvironmentImageSource, TemplateCheck,
};
use tqa_sandbox::docker::ImageRecord;
use tqa_sandbox::template::secret_files::SecretFiles;
use tqa_types::{MIN_RUNTIME_REVISION, RUNTIME_REVISION_LABEL};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const WORKSTATION: &str = "sha256:3333333333333333333333333333333333333333333333333333333333333333";
const DERIVED: &str = "sha256:5555555555555555555555555555555555555555555555555555555555555555";
const OLD_DATA: &str = "sha256:6666666666666666666666666666666666666666666666666666666666666666";
const BAD_LABEL: &str = "sha256:8888888888888888888888888888888888888888888888888888888888888888";
const PLAIN: &str = "sha256:9999999999999999999999999999999999999999999999999999999999999999";
const ADMIN_PIN: &str = "sha256:7777777777777777777777777777777777777777777777777777777777777777";
/// 2026-09-01T00:00:00Z.
const BUILT: i64 = 1_788_220_800;

/// The engine host's images; `None` is an engine that doesn't answer.
struct Engine(Mutex<Option<Vec<ImageRecord>>>);

#[async_trait]
impl EnvironmentImageSource for Engine {
    async fn list(&self) -> std::result::Result<Vec<ImageRecord>, EngineUnavailable> {
        self.0
            .lock()
            .ok()
            .and_then(|held| held.clone())
            .ok_or(EngineUnavailable)
    }
}

fn engine(images: Option<Vec<ImageRecord>>) -> std::sync::Arc<Engine> {
    std::sync::Arc::new(Engine(Mutex::new(images)))
}

fn image(id: &str, tags: &[&str], created: i64, labels: &[(&str, &str)]) -> ImageRecord {
    ImageRecord {
        id: id.to_owned(),
        tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
        created,
        size_bytes: 2 << 30,
        labels: labels
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect::<BTreeMap<_, _>>(),
    }
}

fn labelled(
    id: &str,
    tags: &[&str],
    created: i64,
    environment: (&str, &str, &str),
    revision: &str,
) -> ImageRecord {
    let (name, display, capabilities) = environment;
    image(
        id,
        tags,
        created,
        &[
            (RUNTIME_REVISION_LABEL, revision),
            ("org.tensorquay.environment", name),
            ("org.tensorquay.environment.name", display),
            ("org.tensorquay.environment.capabilities", capabilities),
        ],
    )
}

/// Workstation as the install built it; a person's image built FROM it; a Data build too old for this version; an
/// image whose capability label names an unknown capability; and a runtime image without an environment label.
fn host() -> Vec<ImageRecord> {
    let current = MIN_RUNTIME_REVISION.to_string();
    let old = (MIN_RUNTIME_REVISION - 1).to_string();
    let workstation = (
        "workstation",
        "Workstation",
        "shell,python,node,build,browser",
    );
    vec![
        labelled(
            WORKSTATION,
            &["tqa-runtime:workstation"],
            BUILT,
            workstation,
            &current,
        ),
        labelled(
            DERIVED,
            &["team/data-science:1"],
            BUILT + 50,
            workstation,
            &current,
        ),
        labelled(
            OLD_DATA,
            &["tqa-runtime:data"],
            BUILT - 50,
            ("data", "Data", "shell,python"),
            &old,
        ),
        labelled(
            BAD_LABEL,
            &["team/gpu:1"],
            BUILT - 100,
            ("gpu", "GPU", "shell,gpu"),
            &current,
        ),
        image(
            PLAIN,
            &[],
            BUILT - 200,
            &[(RUNTIME_REVISION_LABEL, &current)],
        ),
    ]
}

async fn listed(f: &Fixture, cookie: &str) -> Result<(u16, Value)> {
    call(
        f,
        Method::GET,
        "/api/v1/admin/environment-images",
        cookie,
        (None, None),
    )
    .await
}

fn item<'a>(listing: &'a Value, id: &str) -> Result<&'a Value> {
    Ok(listing["items"]
        .as_array()
        .ok_or("items")?
        .iter()
        .find(|item| item["id"] == id)
        .ok_or("no such image listed")?)
}

async fn put(f: &Fixture, cookie: &str, name: &str, image: &str) -> Result<u16> {
    let body = json!({"display_name": "Ours", "image": image, "capabilities": ["shell"]});
    Ok(call(
        f,
        Method::PUT,
        &format!("/api/v1/admin/templates/{name}"),
        cookie,
        (Some(body), None),
    )
    .await?
    .0)
}

/// Template checks that pass: a built-in other than the default is offered once verified (GW-I-174).
struct Verifies;

#[async_trait]
impl TemplateCheck for Verifies {
    async fn check(&self, request: &CheckRequest<'_>) -> CheckOutcome {
        CheckOutcome::Verified {
            warnings: SecretFiles::default(),
            base: request.name.to_owned(),
            image_id: request.image.to_owned(),
        }
    }
}

/// Waits (bounded) until the administrators' list shows `name` verified.
async fn verified(f: &Fixture, cookie: &str, name: &str) -> TestResult {
    for _ in 0..100 {
        let (_, list) = call(
            f,
            Method::GET,
            "/api/v1/admin/templates",
            cookie,
            (None, None),
        )
        .await?;
        if list["items"].as_array().is_some_and(|items| {
            items
                .iter()
                .any(|item| item["name"] == name && item["check"]["state"] == "verified")
        }) {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    Err(format!("{name} was never verified").into())
}

// TEST: GW-I-146
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_146_the_image_list_says_what_each_image_is_and_what_uses_it(
    pool: sqlx::PgPool,
) -> TestResult {
    Box::pin(support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        f.gateway.start_template_checks(std::sync::Arc::new(Verifies));
        f.gateway.use_environment_images(engine(Some(host())));
        f.gateway.built_in_environments_now().await?;
        verified(f, &cookie, "workstation").await?;
        // A person picked the derived image for an environment of their own, and a task runs in Workstation.
        assert_eq!(put(f, &cookie, "science", DERIVED).await?, 201);
        let response = f
            .client
            .post(f.url("/api/v1/conversations"))
            .header("origin", f.origin())
            .header("cookie", &cookie)
            .header("idempotency-key", "in-workstation")
            .json(&json!({"title": "In it", "model_route": "local", "template": "workstation"}))
            .send()
            .await?;
        assert_eq!(response.status(), 201);
        let task: tqa_types::ConversationId = response.json::<Value>().await?["id"].as_str().ok_or("id")?.parse()?;
        f.gateway.sandboxes().ensure(&task).await?;

        let (status, listing) = listed(f, &cookie).await?;
        assert_eq!(status, 200, "{listing}");
        assert_eq!(listing["limit"], json!(200));
        let order: Vec<&str> = listing["items"].as_array().ok_or("items")?.iter().filter_map(|item| item["id"].as_str()).collect();
        assert_eq!(order, [DERIVED, WORKSTATION, OLD_DATA, BAD_LABEL, PLAIN], "newest first");
        assert_eq!(
            item(&listing, WORKSTATION)?,
            &json!({
                "id": WORKSTATION, "tags": ["tqa-runtime:workstation"], "created": "2026-09-01T00:00:00Z",
                "size_bytes": 2_u64 << 30, "runtime": "current", "environment": "workstation",
                "display_name": "Workstation", "capabilities": ["shell", "python", "node", "build", "browser"],
                "capabilities_valid": true, "usable": true, "reason": null, "based_on": null,
                "used_by": ["workstation"], "sandboxes": 1, "built_in": "registered", "sizes": null,
            })
        );
        let derived = item(&listing, DERIVED)?;
        assert_eq!(
            (&derived["based_on"], &derived["built_in"], &derived["usable"], &derived["used_by"], &derived["sandboxes"]),
            (&json!("Workstation"), &Value::Null, &json!(true), &json!(["science"]), &json!(0)),
            "F10: derived from Workstation, pickable, never a built-in"
        );
        let old = item(&listing, OLD_DATA)?;
        assert_eq!(
            (&old["runtime"], &old["usable"], &old["reason"], &old["built_in"]),
            (&json!("too_old"), &json!(false), &json!("too old for this version: rebuild it"), &Value::Null)
        );
        let bad = item(&listing, BAD_LABEL)?;
        assert_eq!(
            (&bad["capabilities"], &bad["capabilities_valid"], &bad["usable"], &bad["reason"]),
            (&json!(["shell", "gpu"]), &json!(false), &json!(false), &json!("its capability label names an unknown capability: gpu"))
        );
        let plain = item(&listing, PLAIN)?;
        assert_eq!(
            (&plain["environment"], &plain["display_name"], &plain["capabilities"], &plain["usable"], &plain["tags"]),
            (&Value::Null, &Value::Null, &json!([]), &json!(true), &json!([])),
            "a runtime image without environment labels can still be picked"
        );
        // Only administrators see the host's images.
        let (_, member) = f.member("member@example.invalid").await?;
        assert_eq!(listed(f, &member).await?.0, 403);
        Ok(())
    }))
    .await
}

// TEST: GW-I-146
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_146_a_built_in_not_added_says_why(pool: sqlx::PgPool) -> TestResult {
    // The file's Minimal is the chosen default, so Workstation isn't the default and can be deleted (GW-I-154).
    let file = format!(
        "[[templates]]\nname='minimal'\ndisplay_name='Minimal'\nimage='{ADMIN_PIN}'\ncapabilities=['shell']\ndefault=true\n"
    );
    Box::pin(support::run_configured(pool, None, &file, async |f| {
        let (_, cookie) = f.admin().await?;
        f.gateway.use_environment_images(engine(Some(host())));
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
        let (_, listing) = listed(f, &cookie).await?;
        let workstation = item(&listing, WORKSTATION)?;
        assert_eq!(
            (
                &workstation["built_in"],
                &workstation["reason"],
                &workstation["usable"],
                &workstation["used_by"]
            ),
            (
                &json!("not_added"),
                &json!("deleted by an administrator"),
                &json!(true),
                &json!([])
            )
        );
        // An administrator's own environment took the name.
        assert_eq!(put(f, &cookie, "workstation", ADMIN_PIN).await?, 201);
        let (_, listing) = listed(f, &cookie).await?;
        let workstation = item(&listing, WORKSTATION)?;
        assert_eq!(
            (&workstation["built_in"], &workstation["reason"]),
            (
                &json!("not_added"),
                &json!("not added: an environment named Workstation already exists")
            )
        );
        Ok(())
    }))
    .await
}

// TEST: GW-I-146
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_146_an_empty_host_and_an_engine_that_does_not_answer(
    pool: sqlx::PgPool,
) -> TestResult {
    Box::pin(support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        // No engine at all (its address was unusable at start): the same plain answer as one that doesn't answer.
        for source in [None, Some(engine(None))] {
            if let Some(source) = source {
                f.gateway.use_environment_images(source);
            }
            let (status, body) = listed(f, &cookie).await?;
            assert_eq!(status, 503, "{body}");
            assert_eq!(
                (&body["error"]["code"], &body["error"]["retryable"]),
                (&json!("engine_unavailable"), &json!(true))
            );
        }
        f.gateway.use_environment_images(engine(Some(Vec::new())));
        let (status, body) = listed(f, &cookie).await?;
        assert_eq!((status, &body["items"]), (200, &json!([])));
        Ok(())
    }))
    .await
}
