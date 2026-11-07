//! GW-I-137 (the environment sizes design): a sandbox's CPU (in halves), memory and processes, and a new workspace's
//! disk, are its environment's, or the installation's defaults, never above the file-only `[sandbox.ceiling]`; an
//! administrator's environment above the ceiling is refused on save; the admin lists show the defaults and the ceiling;
//! and the upgrade folds the Limits page's size overrides into the environments.
#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const MIB: u64 = 1024 * 1024;
const PIN: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const OTHER: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";

/// A ceiling of 4 CPUs, 4 GiB, 1024 processes and 8 GiB of disk; defaults of 1 CPU, 512 MiB, 256 processes and 1 GiB;
/// a sized environment (the default) and one without a size.
fn config() -> String {
    format!(
        "[sandbox.ceiling]\ncpus=4\nmemory_mib=4096\npids=1024\nworkspace_mib=8192\n\
         [workspaces]\nquota_bytes={}\n\
         [[templates]]\nname='sized'\ndisplay_name='Sized'\nimage='{PIN}'\ncapabilities=['shell']\ndefault=true\n\
         cpus=1.5\nmemory_mib=2048\npids=512\nworkspace_mib=4096\n\
         [[templates]]\nname='plain'\ndisplay_name='Plain'\nimage='{OTHER}'\ncapabilities=['shell']\n",
        1024 * MIB
    )
}

async fn task(f: &Fixture, cookie: &str, template: &str) -> Result<tqa_types::ConversationId> {
    let (status, created) = call(
        f,
        Method::POST,
        "/api/v1/conversations",
        cookie,
        (
            Some(json!({"title": "Task", "model_route": "local", "template": template})),
            Some(&format!("task-{template}")),
        ),
    )
    .await?;
    assert_eq!(status, 201, "{created}");
    Ok(created["id"].as_str().ok_or("id")?.parse()?)
}

async fn workspace(f: &Fixture, cookie: &str, body: Value) -> Result<Value> {
    let (status, created) = call(
        f,
        Method::POST,
        "/api/v1/workspaces",
        cookie,
        (Some(body), None),
    )
    .await?;
    assert_eq!(status, 201, "{created}");
    Ok(created)
}

// TEST: GW-I-137
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_137_a_sandbox_and_a_new_workspace_are_sized_by_their_environment(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, &config(), async |f| {
        let (_, cookie) = f.admin().await?;
        // The sized environment's own values; the other gets the defaults.
        for (template, size) in [("sized", (1500, 2048 * MIB, 512)), ("plain", (1000, 512 * MIB, 256))] {
            let id = task(f, &cookie, template).await?;
            let _runtime = f.runtime_for(&id).await?;
            assert_eq!(f.backend.limits(&id), Some(size), "{template}");
            let recorded: (Option<i32>, Option<i64>, Option<i32>) = sqlx::query_as(
                "SELECT cpu_millicores, memory_bytes, pids FROM sandbox_instances WHERE conversation_id=$1",
            )
            .bind(uuid::Uuid::parse_str(id.as_str())?)
            .fetch_one(&f.pool)
            .await?;
            let expected = (
                i32::try_from(size.0)?,
                i64::try_from(size.1)?,
                i32::try_from(size.2)?,
            );
            assert_eq!(recorded, (Some(expected.0), Some(expected.1), Some(expected.2)), "{template}");
            // A task's own workspace gets its environment's disk.
            let (_, read) = call(f, Method::GET, &format!("/api/v1/conversations/{id}"), &cookie, (None, None)).await?;
            let bound = read["workspace_id"].as_str().ok_or("workspace")?;
            let (_, view) = call(f, Method::GET, &format!("/api/v1/workspaces/{bound}"), &cookie, (None, None)).await?;
            let disk = if template == "sized" { 4096 * MIB } else { 1024 * MIB };
            assert_eq!(view["quota_bytes"], json!(disk), "{template}");
        }
        // A workspace made on the Workspaces page gets the default environment's disk, or the named one's; a copy keeps
        // its source's.
        let made = workspace(f, &cookie, json!({"name": "Notes"})).await?;
        assert_eq!(made["quota_bytes"], json!(4096 * MIB));
        let plain = workspace(f, &cookie, json!({"name": "Plain", "template": "plain"})).await?;
        assert_eq!(plain["quota_bytes"], json!(1024 * MIB));
        let (status, fork) = call(
            f,
            Method::POST,
            &format!("/api/v1/workspaces/{}/fork", plain["id"].as_str().ok_or("id")?),
            &cookie,
            (Some(json!({"name": "Copy"})), None),
        )
        .await?;
        assert_eq!((status, &fork["quota_bytes"]), (202, &json!(1024 * MIB)), "{fork}");
        // The people's list shows CPU and memory.
        let (_, listed) = call(f, Method::GET, "/api/v1/templates", &cookie, (None, None)).await?;
        let sized = listed["items"].as_array().ok_or("items")?.iter().find(|item| item["name"] == "sized").ok_or("sized")?;
        assert_eq!((&sized["cpus"], &sized["memory_mib"]), (&json!(1.5), &json!(2048)));
        Ok(())
    })
    .await
}

// TEST: GW-I-137
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_137_the_ceiling_refuses_an_environment_on_save_and_both_lists_show_it(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, &config(), async |f| {
        let (_, cookie) = f.admin().await?;
        let body = |extra: Value| {
            let mut body =
                json!({"display_name": "Big", "image": OTHER, "capabilities": ["shell"]});
            body.as_object_mut()
                .into_iter()
                .for_each(|fields| fields.extend(extra.as_object().cloned().unwrap_or_default()));
            body
        };
        for (field, value) in [
            ("cpus", json!(4.5)),
            ("memory_mib", json!(4097)),
            ("pids", json!(1025)),
            ("workspace_mib", json!(8193)),
        ] {
            let (status, refused) = call(
                f,
                Method::PUT,
                "/api/v1/admin/templates/big",
                &cookie,
                (Some(body(json!({field: value}))), None),
            )
            .await?;
            let detail = &refused["error"]["details"][0];
            assert_eq!(
                (status, &detail["field"], &detail["reason"]),
                (400, &json!(field), &json!("limit_out_of_range")),
                "{field}: {refused}"
            );
        }
        let at =
            body(json!({"cpus": 3.5, "memory_mib": 4096, "pids": 1024, "workspace_mib": 8192}));
        let (status, created) = call(
            f,
            Method::PUT,
            "/api/v1/admin/templates/big",
            &cookie,
            (Some(at), None),
        )
        .await?;
        assert_eq!((status, &created["cpus"]), (201, &json!(3.5)), "{created}");
        let expected = json!({
            "defaults": {"cpus": 1.0, "memory_mib": 512, "pids": 256, "workspace_mib": 1024},
            "ceiling": {"cpus": 4.0, "memory_mib": 4096, "pids": 1024, "workspace_mib": 8192},
        });
        for path in ["/api/v1/admin/templates", "/api/v1/admin/limits"] {
            let (_, listed) = call(f, Method::GET, path, &cookie, (None, None)).await?;
            assert_eq!(
                (&listed["defaults"], &listed["ceiling"]),
                (&expected["defaults"], &expected["ceiling"]),
                "{path}"
            );
        }
        // An environment above a ceiling the file lowered since is marked, and new sandboxes get the ceiling.
        sqlx::query("UPDATE templates SET cpu_millicores=6000 WHERE name='big'")
            .execute(&f.pool)
            .await?;
        let (_, listed) = call(
            f,
            Method::GET,
            "/api/v1/admin/templates",
            &cookie,
            (None, None),
        )
        .await?;
        let big = listed["items"]
            .as_array()
            .ok_or("items")?
            .iter()
            .find(|item| item["name"] == "big")
            .ok_or("big")?;
        assert_eq!(big["clamped"], json!(["cpus"]));
        // The sizes are no longer Limits keys.
        let (_, limits) = call(
            f,
            Method::GET,
            "/api/v1/admin/limits",
            &cookie,
            (None, None),
        )
        .await?;
        let keys: Vec<&str> = limits["items"]
            .as_array()
            .ok_or("items")?
            .iter()
            .filter_map(|item| item["key"].as_str())
            .collect();
        assert!(
            !keys.iter().any(|key| [
                "sandbox.cpu_millicores",
                "sandbox.memory_bytes",
                "sandbox.pids"
            ]
            .contains(key)),
            "{keys:?}"
        );
        Ok(())
    })
    .await
}

/// The admin list's entry for `name`.
async fn admin_item(f: &Fixture, cookie: &str, name: &str) -> Result<Value> {
    let (_, listed) = call(
        f,
        Method::GET,
        "/api/v1/admin/templates",
        cookie,
        (None, None),
    )
    .await?;
    Ok(listed["items"]
        .as_array()
        .ok_or("items")?
        .iter()
        .find(|item| item["name"] == name)
        .ok_or("no such environment")?
        .clone())
}

// TEST: GW-I-137
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_137_a_stored_size_above_the_ceiling_is_lowered_at_start_and_the_default_still_saves(
    pool: PgPool,
) -> TestResult {
    support::run_restarting_with(pool, None, &config(), async |f| {
        let (_, cookie) = f.admin().await?;
        // The default holds a size the ceiling doesn't allow, as an upgrade's fold or a ceiling lowered since leaves
        // it: 100 CPUs (above even an environment's own bound) and 32 GiB.
        // It passed its check, as a default must before an administrator saves it.
        sqlx::query("UPDATE templates SET cpu_millicores=100000, memory_bytes=$1, check_state='verified', checked_version=version WHERE name='sized'")
            .bind(i64::try_from(32_768 * MIB)?)
            .execute(&f.pool)
            .await?;
        let was = admin_item(f, &cookie, "sized").await?["version"].as_i64().ok_or("version")?;
        f.restart().await?;
        let lowered = admin_item(f, &cookie, "sized").await?;
        assert_eq!(
            (&lowered["cpus"], &lowered["memory_mib"], &lowered["pids"]),
            (&json!(4), &json!(4096), &json!(512)),
            "{lowered}"
        );
        assert_eq!(lowered["clamped"], json!(["cpus", "memory_mib"]));
        assert_eq!(lowered["clamped_from"], json!({"cpus": 100, "memory_mib": 32_768}));
        let version = lowered["version"].as_i64().ok_or("version")?;
        assert_eq!(version, was + 1);
        // Lowering is the ceiling's doing, not a change to check again: the default stays checked.
        assert_eq!(
            (&lowered["check"]["state"], &lowered["check"]["checked_version"]),
            (&json!("verified"), &json!(version))
        );
        let audited: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM admin_audit WHERE actor_kind='seed' AND item='sized' AND after_version=$1",
        )
        .bind(version)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(audited, 1);
        // Another start changes nothing.
        f.restart().await?;
        assert_eq!(admin_item(f, &cookie, "sized").await?["version"], json!(version));
        // A ceiling raised again gives the size back, up to the new ceiling; what is still above it stays noted.
        f.reconfigure(&config().replace("cpus=4\nmemory_mib=4096", "cpus=6\nmemory_mib=65536"))?;
        f.restart().await?;
        let raised = admin_item(f, &cookie, "sized").await?;
        assert_eq!(
            (&raised["cpus"], &raised["memory_mib"], &raised["clamped"], &raised["clamped_from"]),
            (&json!(6), &json!(32_768), &json!(["cpus"]), &json!({"cpus": 100})),
            "{raised}"
        );
        f.reconfigure(&config())?;
        f.restart().await?;
        let lowered = admin_item(f, &cookie, "sized").await?;
        assert_eq!(lowered["clamped_from"], json!({"cpus": 100, "memory_mib": 32_768}));
        let version = lowered["version"].as_i64().ok_or("version")?;
        // The default saves as the form sends it: stored sizes untouched, only its processes changed.
        let (status, saved) = call(
            f,
            Method::PUT,
            "/api/v1/admin/templates/sized",
            &cookie,
            (
                Some(json!({
                    "display_name": "Sized", "image": PIN, "capabilities": ["shell"], "cpus": 4,
                    "memory_mib": 4096, "pids": 600, "workspace_mib": 4096, "version": version,
                })),
                None,
            ),
        )
        .await?;
        assert_eq!((status, &saved["pids"]), (200, &json!(600)), "{saved}");
        assert_eq!((&saved["clamped"], &saved["clamped_from"]), (&json!([]), &Value::Null));
        Ok(())
    })
    .await
}

// TEST: GW-I-137
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_137_a_size_folded_into_a_seeded_environment_outlasts_a_file_change(
    pool: PgPool,
) -> TestResult {
    support::run_restarting_with(pool, None, &config(), async |f| {
        let (_, cookie) = f.admin().await?;
        // The upgrade folded a Limits override into the file's environment without a size.
        sqlx::query("UPDATE templates SET cpu_millicores=2500, pids=300, sizes_from_limits=true WHERE name='plain'")
            .execute(&f.pool)
            .await?;
        f.reconfigure(&config().replace("display_name='Plain'", "display_name='Plain 2'"))?;
        f.restart().await?;
        let plain = admin_item(f, &cookie, "plain").await?;
        assert_eq!(
            (&plain["display_name"], &plain["cpus"], &plain["pids"], &plain["memory_mib"]),
            (&json!("Plain 2"), &json!(2.5), &json!(300), &Value::Null),
            "{plain}"
        );
        // A size the file then names is the file's.
        f.reconfigure(&config().replace(
            "display_name='Plain'",
            "display_name='Plain 3'\ncpus=1",
        ))?;
        f.restart().await?;
        let plain = admin_item(f, &cookie, "plain").await?;
        assert_eq!((&plain["cpus"], &plain["pids"]), (&json!(1), &json!(300)), "{plain}");
        Ok(())
    })
    .await
}

/// A template after the fold: name, CPU, memory, processes, version, and whether the fold changed it.
type Folded = (String, Option<i32>, Option<i64>, Option<i32>, i64, bool);

/// The migrations before `version`.
fn before(version: i64) -> sqlx::migrate::Migrator {
    sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            tqa_gateway::MIGRATOR
                .migrations
                .iter()
                .filter(|migration| migration.version < version)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    }
}

// TEST: GW-I-137
#[sqlx::test(migrations = false)]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_137_the_upgrade_folds_the_limits_size_overrides_into_environments(
    pool: PgPool,
) -> TestResult {
    support::require_database()?;
    let sizes = tqa_gateway::MIGRATOR
        .migrations
        .iter()
        .find(|migration| migration.description.contains("environment sizes"))
        .map(|migration| migration.version)
        .ok_or("no environment sizes migration")?;
    before(sizes).run(&pool).await?;
    // An administrator's CPU override between halves, a memory and a processes override; a template with its own
    // CPU, one without, one with its own memory below 256 MiB, and one from the configuration file.
    for (key, value) in [
        ("sandbox.cpu_millicores", 1200_i64),
        ("sandbox.memory_bytes", 1024 * 1024 * 1024),
        ("sandbox.pids", 8192),
    ] {
        sqlx::query("INSERT INTO settings (id,key,value,last_set_by) VALUES ($1,$2,$3,'admin')")
            .bind(uuid::Uuid::now_v7())
            .bind(key)
            .bind(value)
            .execute(&pool)
            .await?;
    }
    let small = i64::try_from(128 * MIB)?;
    for (name, cpu, memory, set_by) in [
        ("own", Some(2000_i32), None, "admin"),
        ("bare", None, None, "admin"),
        ("small", Some(1000), Some(small), "admin"),
        ("seeded", None, None, "seed"),
    ] {
        sqlx::query("INSERT INTO templates (name,display_name,image,capabilities,cpu_millicores,memory_bytes,is_default,version,last_set_by) VALUES ($1,$1,$2,'{shell}',$3,$4,$5,1,$6)")
            .bind(name)
            .bind(PIN)
            .bind(cpu)
            .bind(memory)
            .bind(name == "own")
            .bind(set_by)
            .execute(&pool)
            .await?;
    }
    sqlx::query("UPDATE templates SET check_state='verified', checked_version=1 WHERE name='own'")
        .execute(&pool)
        .await?;
    tqa_gateway::MIGRATOR.run(&pool).await?;
    let checked: Option<i64> =
        sqlx::query_scalar("SELECT checked_version FROM templates WHERE name='own'")
            .fetch_one(&pool)
            .await?;
    assert_eq!(
        checked,
        Some(2),
        "a check of the version before still holds"
    );
    let sized: Vec<Folded> = sqlx::query_as(
        "SELECT name, cpu_millicores, memory_bytes, pids, version, sizes_from_limits FROM templates ORDER BY name",
    )
    .fetch_all(&pool)
    .await?;
    let (gib, least) = (i64::try_from(1024 * MIB)?, i64::try_from(256 * MIB)?);
    assert_eq!(
        sized,
        [
            (
                "bare".to_owned(),
                Some(1500),
                Some(gib),
                Some(8192),
                2,
                true
            ),
            ("own".to_owned(), Some(2000), Some(gib), Some(8192), 2, true),
            (
                "seeded".to_owned(),
                Some(1500),
                Some(gib),
                Some(8192),
                2,
                true
            ),
            (
                "small".to_owned(),
                Some(1000),
                Some(least),
                Some(8192),
                2,
                true
            ),
        ],
        "the CPU override rounds up to a half; an environment's own size stays, and memory rises to 256 MiB; the \
         processes override is kept as it was (the gateway lowers it to the ceiling at start)"
    );
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM settings WHERE key LIKE 'sandbox.%'")
        .fetch_one(&pool)
        .await?;
    let audited: i64 =
        sqlx::query_scalar("SELECT count(*) FROM admin_audit WHERE actor_kind='seed'")
            .fetch_one(&pool)
            .await?;
    assert_eq!(
        (left, audited),
        (0, 7),
        "four environments and three overrides audited"
    );
    Ok(())
}
