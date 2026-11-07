//! GW-I-159 (candidate 13's first-hour check, P0): Workstation registers from the labels its image really carries. Its
//! six capabilities once passed the protocol's bound but not the database's, so the start-up refresh failed as
//! `store_unavailable` and only Minimal was offered. The label is read from `images/runtime/Dockerfile`, so a fixture
//! can't drift from the image again. A built-in the database refuses is logged as what it is, naming the environment,
//! and the other built-ins still register.
//! SBX-C-020 (ADR-0039 section 10): the Desktop's own sizes (2 CPUs, 4 GiB, 1,024 processes) above a lower
//! `[sandbox.ceiling]` are lowered to it at the next start and recorded (`clamped_from`), and the Desktop is still
//! offered.
#[path = "support/log_capture.rs"]
mod log_capture;
#[path = "support/fixture.rs"]
mod support;

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{TestResult, call};
use tqa_gateway::{EngineUnavailable, EnvironmentImageSource};
use tqa_sandbox::docker::ImageRecord;
use tqa_types::{MIN_RUNTIME_REVISION, RUNTIME_REVISION_LABEL};

const WORKSTATION: &str = "sha256:3333333333333333333333333333333333333333333333333333333333333333";
const DATA: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const DESKTOP: &str = "sha256:4444444444444444444444444444444444444444444444444444444444444444";
/// 2026-09-01T00:00:00Z.
const BUILT: i64 = 1_788_220_800;

struct Engine(Vec<ImageRecord>);

#[async_trait]
impl EnvironmentImageSource for Engine {
    async fn list(&self) -> Result<Vec<ImageRecord>, EngineUnavailable> {
        Ok(self.0.clone())
    }
}

/// The environment labels a stage of the runtime Dockerfile sets, as the build gives them to its image.
fn stage_labels(stage: &str) -> Result<BTreeMap<String, String>, Box<dyn std::error::Error>> {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../images/runtime/Dockerfile"
    ))?;
    let start = text
        .find(&format!(" AS {stage}\n"))
        .ok_or(format!("no {stage} stage"))?;
    let body = &text[start..];
    let body = &body[..body[1..].find("\nFROM ").map_or(body.len(), |end| end + 1)];
    Ok(body
        .split(|c: char| c.is_whitespace() || c == '\\')
        .filter_map(|word| word.split_once('='))
        .filter(|(key, _)| key.starts_with("org.tensorquay.environment"))
        .map(|(key, value)| (key.to_owned(), value.trim_matches('"').to_owned()))
        .collect())
}

fn image(id: &str, tag: &str, stage: &str) -> Result<ImageRecord, Box<dyn std::error::Error>> {
    let mut labels = stage_labels(stage)?;
    labels.insert(
        RUNTIME_REVISION_LABEL.to_owned(),
        MIN_RUNTIME_REVISION.to_string(),
    );
    Ok(ImageRecord {
        id: id.to_owned(),
        tags: vec![tag.to_owned()],
        created: BUILT,
        size_bytes: 2 << 30,
        labels,
    })
}

/// The Data built-in, which the file doesn't name, with two capabilities.
fn data() -> ImageRecord {
    ImageRecord {
        id: DATA.to_owned(),
        tags: vec!["tqa-runtime:data".to_owned()],
        created: BUILT,
        size_bytes: 1 << 30,
        labels: BTreeMap::from(
            [
                (RUNTIME_REVISION_LABEL, MIN_RUNTIME_REVISION.to_string()),
                ("org.tensorquay.environment", "data".to_owned()),
                ("org.tensorquay.environment.name", "Data".to_owned()),
                (
                    "org.tensorquay.environment.capabilities",
                    "shell,python".to_owned(),
                ),
            ]
            .map(|(key, value)| (key.to_owned(), value)),
        ),
    }
}

async fn capabilities(pool: &PgPool, name: &str) -> Result<Option<Vec<String>>, sqlx::Error> {
    sqlx::query_scalar("SELECT capabilities FROM templates WHERE name=$1")
        .bind(name)
        .fetch_optional(pool)
        .await
}

// TEST: GW-I-159
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_159_workstation_registers_with_every_capability_its_image_declares(
    pool: PgPool,
) -> TestResult {
    let declared: Vec<String> = stage_labels("workstation")?
        .get("org.tensorquay.environment.capabilities")
        .ok_or("Workstation declares its capabilities")?
        .split(',')
        .map(str::to_owned)
        .collect();
    assert!(declared.contains(&"data".to_owned()), "{declared:?}");
    // The Desktop declares all seven (revision 14, ADR-0039).
    let desktop: Vec<String> = stage_labels("desktop")?
        .get("org.tensorquay.environment.capabilities")
        .ok_or("the Desktop declares its capabilities")?
        .split(',')
        .map(str::to_owned)
        .collect();
    assert_eq!(desktop.len(), 7, "{desktop:?}");
    support::run(pool, async |f| {
        let engine = Engine(vec![
            image(WORKSTATION, "tqa-runtime:workstation", "workstation")?,
            image(DESKTOP, "tqa-runtime:desktop", "desktop")?,
        ]);
        f.gateway.use_environment_images(Arc::new(engine));
        f.gateway.built_in_environments_now().await?;
        assert_eq!(capabilities(&f.pool, "workstation").await?, Some(declared));
        assert_eq!(capabilities(&f.pool, "desktop").await?, Some(desktop));
        Ok(())
    })
    .await
}

// TEST: GW-I-159
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_159_a_built_in_the_database_refuses_is_named_and_the_others_still_register(
    pool: PgPool,
) -> TestResult {
    let capture = log_capture::Capture::default();
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_ansi(false)
            .with_max_level(tracing::Level::INFO)
            .finish(),
    )?;
    support::run(pool, async |f| {
        // A database stricter than this version: it refuses Workstation's six capabilities, not another built-in's two.
        sqlx::query(
            "ALTER TABLE templates ADD CONSTRAINT test_few_capabilities CHECK (cardinality(capabilities) <= 2) NOT VALID",
        )
        .execute(&f.pool)
        .await?;
        let engine = Engine(vec![
            image(WORKSTATION, "tqa-runtime:workstation", "workstation")?,
            data(),
        ]);
        f.gateway.use_environment_images(Arc::new(engine));
        f.gateway.built_in_environments_now().await?;
        assert_eq!(capabilities(&f.pool, "workstation").await?, None);
        assert_eq!(
            capabilities(&f.pool, "data").await?,
            Some(vec!["shell".to_owned(), "python".to_owned()])
        );
        let logs = capture.text()?;
        let refused: Vec<&str> = logs
            .lines()
            .filter(|line| line.contains("built-in environment not registered"))
            .collect();
        assert_eq!(refused.len(), 1, "{logs}");
        assert!(
            refused[0].contains("ERROR")
                && refused[0].contains("workstation")
                && refused[0].contains("test_few_capabilities"),
            "{}",
            refused[0]
        );
        assert!(!logs.contains("store_unavailable"), "{logs}");
        Ok(())
    })
    .await
}

// TEST: SBX-C-020
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sbx_c_020_a_ceiling_below_the_desktop_lowers_it_at_start_and_it_is_still_offered(
    pool: PgPool,
) -> TestResult {
    support::run_restarting_with(
        pool,
        None,
        "[sandbox.ceiling]\ncpus=1\nmemory_mib=2048\npids=512\nworkspace_mib=8192\n[workspaces]\nquota_bytes=1073741824\n",
        async |f| {
            let (_, cookie) = f.admin().await?;
            f.gateway.use_environment_images(Arc::new(Engine(vec![image(
                DESKTOP,
                "tqa-runtime:desktop",
                "desktop",
            )?])));
            f.gateway.built_in_environments_now().await?;
            // Registered with its own sizes; its check passed, as one must before people are offered it.
            sqlx::query("UPDATE templates SET check_state='verified', checked_version=version WHERE name='desktop'")
                .execute(&f.pool)
                .await?;
            f.restart().await?;
            let (_, listed) = call(f, Method::GET, "/api/v1/admin/templates", &cookie, (None, None)).await?;
            let desktop = listed["items"]
                .as_array()
                .ok_or("items")?
                .iter()
                .find(|item| item["name"] == "desktop")
                .ok_or("the Desktop is listed")?;
            assert_eq!(
                (&desktop["cpus"], &desktop["memory_mib"], &desktop["pids"]),
                (&json!(1), &json!(2048), &json!(512)),
                "{desktop}"
            );
            assert_eq!(desktop["clamped"], json!(["cpus", "memory_mib", "pids"]));
            assert_eq!(desktop["clamped_from"], json!({"cpus": 2, "memory_mib": 4096, "pids": 1024}));
            // Lowered by the ceiling, not changed: still checked, and still offered at the lowered size.
            assert_eq!(desktop["check"]["state"], json!("verified"), "{desktop}");
            let offered: Value = f.get("/api/v1/templates", &cookie).await?.error_for_status()?.json().await?;
            let item = offered["items"]
                .as_array()
                .ok_or("items")?
                .iter()
                .find(|item| item["name"] == "desktop")
                .ok_or("the Desktop is still offered")?;
            assert_eq!((&item["cpus"], &item["memory_mib"]), (&json!(1.0), &json!(2048)), "{item}");
            Ok(())
        },
    )
    .await
}
