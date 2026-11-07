//! Agent profiles (ADR-0012). GW-I-025 (CRUD part): a user's profiles are created, listed, read, replaced and deleted,
//! with field-level refusals, versions and retried keys. SEC-028: a personal profile is its owner's alone, and only
//! administrators manage shared ones.
#[path = "support/fixture.rs"]
mod support;

use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn profile(name: &str) -> Value {
    json!({
        "name": name,
        "display_name": "Reviewer",
        "instructions": "Review; never edit.",
        "model_route": "local",
        "tools": ["read_file", "grep"],
        "limits": {"max_steps": 20},
    })
}

async fn create(f: &Fixture, cookie: &str, body: Value, key: Option<&str>) -> Result<(u16, Value)> {
    call(
        f,
        Method::POST,
        "/api/v1/profiles",
        cookie,
        (Some(body), key),
    )
    .await
}

fn code(answer: &(u16, Value)) -> (u16, Value) {
    (answer.0, answer.1["error"]["code"].clone())
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_025_a_users_profiles_are_managed_with_versions_and_field_errors(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        // Members are made with the administrator's password hash, so an administrator comes first.
        f.admin().await?;
        let (_, cookie) = f.member("owner@example.invalid").await?;
        let (status, created) =
            create(f, &cookie, profile("reviewer"), Some("create-reviewer")).await?;
        assert_eq!(status, 201, "{created}");
        assert_eq!(
            (&created["shared"], &created["version"], &created["name"]),
            (&json!(false), &json!(1), &json!("reviewer"))
        );
        assert_eq!(created["tools"], json!(["read_file", "grep"]));
        let id = created["id"].as_str().ok_or("id")?.to_owned();
        // A retry replays; the same key with another body is refused.
        assert_eq!(
            create(f, &cookie, profile("reviewer"), Some("create-reviewer")).await?,
            (201, created.clone())
        );
        assert_eq!(
            create(f, &cookie, profile("other"), Some("create-reviewer"))
                .await?
                .0,
            422
        );
        // Listed and read back.
        let (_, listed) = call(f, Method::GET, "/api/v1/profiles", &cookie, (None, None)).await?;
        let names: Vec<&str> = listed["items"]
            .as_array()
            .ok_or("items")?
            .iter()
            .filter_map(|item| item["name"].as_str())
            .collect();
        assert_eq!(names, ["reviewer"]);
        let path = format!("/api/v1/profiles/{id}");
        assert_eq!(
            call(f, Method::GET, &path, &cookie, (None, None)).await?.1,
            created
        );
        // Field errors carry the field and reason; a name that exists nowhere is its own refusal.
        let mut blank = profile("blank");
        blank["display_name"] = json!("");
        let (status, refused) = create(f, &cookie, blank, None).await?;
        assert_eq!(
            (status, &refused["error"]["details"]),
            (
                400,
                &json!([{"field": "display_name", "reason": "display_name_invalid"}])
            )
        );
        let mut nowhere = profile("nowhere");
        nowhere["model_route"] = json!("no-such-route");
        let (status, refused) = create(f, &cookie, nowhere, None).await?;
        assert_eq!(
            (status, &refused["error"]["details"]),
            (
                422,
                &json!([{"field": "model_route", "reason": "unknown_reference"}])
            )
        );
        // Every other name must exist too; bodies that are not a profile are refused before anything is stored.
        for (field, value) in [
            ("template", json!("nowhere")),
            ("mcp_servers", json!(["nowhere"])),
            ("connectors", json!(["nowhere"])),
            ("decision_engine", json!("nowhere")),
        ] {
            let mut body = profile("refs");
            body[field] = value;
            let (status, refused) = create(f, &cookie, body, None).await?;
            let named = &refused["error"]["details"][0]["field"];
            assert_eq!((status, named), (422, &json!(field)), "{field}");
        }
        let mut versioned = profile("versioned");
        versioned["version"] = json!(1);
        assert_eq!(
            create(f, &cookie, versioned, None).await?.0,
            400,
            "a version on create"
        );
        assert_eq!(
            create(f, &cookie, json!(["not", "a", "profile"]), None)
                .await?
                .0,
            400
        );
        let raw = f
            .client
            .post(f.url("/api/v1/profiles"))
            .header("origin", f.origin())
            .header("cookie", &cookie)
            .header("content-type", "application/json")
            .body("{not json")
            .send()
            .await?;
        assert_eq!(raw.status(), 400);
        // Versions: replace the version read; a stale one is a conflict; names are unique per owner.
        let mut replaced = profile("reviewer");
        replaced["display_name"] = json!("Careful reviewer");
        replaced["version"] = json!(1);
        let mut sharing = replaced.clone();
        sharing["shared"] = json!(true);
        let put = |body: Value, key: Option<&'static str>| {
            call(f, Method::PUT, &path, &cookie, (Some(body), key))
        };
        assert_eq!(put(sharing, None).await?.0, 400, "sharing never changes");
        assert_eq!(
            put(profile("reviewer"), None).await?.0,
            400,
            "a replace needs its version"
        );
        let (status, updated) = put(replaced.clone(), Some("replace-reviewer")).await?;
        assert_eq!(
            (status, &updated["version"], &updated["display_name"]),
            (200, &json!(2), &json!("Careful reviewer"))
        );
        // A retried replace replays its answer instead of meeting its own new version.
        let retried = put(replaced.clone(), Some("replace-reviewer")).await?;
        assert_eq!(retried, (200, updated.clone()));
        assert_eq!(code(&put(replaced, None).await?), (409, json!("conflict")));
        assert_eq!(
            code(&create(f, &cookie, profile("reviewer"), None).await?),
            (409, json!("name_taken"))
        );
        // Deleted with its version.
        let stale = format!("{path}?version=1");
        assert_eq!(
            code(&call(f, Method::DELETE, &stale, &cookie, (None, None)).await?),
            (409, json!("conflict"))
        );
        let current = format!("{path}?version=2");
        assert_eq!(
            call(f, Method::DELETE, &current, &cookie, (None, None))
                .await?
                .0,
            204
        );
        assert_eq!(
            call(f, Method::GET, &path, &cookie, (None, None)).await?.0,
            404
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_028_a_personal_profile_is_its_owners_and_shared_ones_are_administered(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, admin) = f.admin().await?;
        let (_, owner) = f.member("owner@example.invalid").await?;
        let (_, other) = f.member("other@example.invalid").await?;
        let (_, mine) = create(f, &owner, profile("mine"), None).await?;
        let path = format!("/api/v1/profiles/{}", mine["id"].as_str().ok_or("id")?);
        // Nobody else sees or touches it, administrators included.
        for cookie in [&other, &admin] {
            assert_eq!(
                call(f, Method::GET, &path, cookie, (None, None)).await?.0,
                404
            );
            let mut change = profile("mine");
            change["version"] = json!(1);
            assert_eq!(
                call(f, Method::PUT, &path, cookie, (Some(change), None))
                    .await?
                    .0,
                404
            );
            assert_eq!(
                call(
                    f,
                    Method::DELETE,
                    &format!("{path}?version=1"),
                    cookie,
                    (None, None)
                )
                .await?
                .0,
                404
            );
            let (_, listed) =
                call(f, Method::GET, "/api/v1/profiles", cookie, (None, None)).await?;
            assert!(!listed.to_string().contains("\"mine\""), "{listed}");
        }
        // Only an administrator creates a shared profile; every member sees it; members cannot change it.
        let mut shared = profile("team");
        shared["shared"] = json!(true);
        assert_eq!(code(&create(f, &other, shared.clone(), None).await?).0, 403);
        let (status, team) = create(f, &admin, shared, None).await?;
        assert_eq!((status, &team["shared"]), (201, &json!(true)));
        let team_path = format!("/api/v1/profiles/{}", team["id"].as_str().ok_or("id")?);
        assert_eq!(
            call(f, Method::GET, &team_path, &other, (None, None))
                .await?
                .0,
            200
        );
        let mut change = profile("team");
        change["version"] = json!(1);
        assert_eq!(
            call(f, Method::PUT, &team_path, &other, (Some(change), None))
                .await?
                .0,
            403
        );
        assert_eq!(
            call(
                f,
                Method::DELETE,
                &format!("{team_path}?version=1"),
                &other,
                (None, None)
            )
            .await?
            .0,
            403
        );
        let (_, listed) = call(f, Method::GET, "/api/v1/profiles", &other, (None, None)).await?;
        assert!(listed.to_string().contains("\"team\""));
        // The owner's profile is untouched by all of it.
        assert_eq!(
            call(f, Method::GET, &path, &owner, (None, None)).await?.1["version"],
            1
        );
        Ok(())
    })
    .await
}

/// Stores `count` profiles for `owner` (or shared ones) directly, as if made earlier.
async fn fill(f: &Fixture, owner: Option<uuid::Uuid>, count: usize) -> TestResult {
    for index in 0..count {
        let name = format!("filled-{index}");
        sqlx::query("INSERT INTO agent_profiles (id,owner_id,name,document,last_set_by) VALUES ($1,$2,$3,$4,$5)")
            .bind(uuid::Uuid::now_v7())
            .bind(owner)
            .bind(&name)
            .bind(json!({"name": name, "display_name": "Filled", "skills": "all"}))
            .bind(if owner.is_some() { "owner" } else { "admin" })
            .execute(&f.pool)
            .await?;
    }
    Ok(())
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_025_profiles_are_bounded_per_owner_and_per_install(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, admin) = f.admin().await?;
        let (owner, cookie) = f.member("owner@example.invalid").await?;
        let (_, other) = f.member("other@example.invalid").await?;
        // A user's hundredth profile is accepted; the next is refused.
        fill(f, Some(owner.id), 99).await?;
        assert_eq!(create(f, &cookie, profile("hundredth"), None).await?.0, 201);
        assert_eq!(
            code(&create(f, &cookie, profile("one-more"), None).await?),
            (409, json!("profile_count"))
        );
        // Another user's count is their own.
        assert_eq!(create(f, &other, profile("first"), None).await?.0, 201);
        // The install's sixty-fourth shared profile is accepted; the next is refused. Seeded ones are bounded by the
        // file and do not count.
        sqlx::query("INSERT INTO agent_profiles (id,owner_id,name,document,last_set_by,seed_digest,seed_state) VALUES ($1,NULL,'seeded',$2,'seed',$3,'current')")
            .bind(uuid::Uuid::now_v7())
            .bind(json!({"name": "seeded", "display_name": "Seeded", "skills": "all"}))
            .bind(vec![0_u8; 32])
            .execute(&f.pool)
            .await?;
        fill(f, None, 63).await?;
        let shared = |name: &str| {
            let mut body = profile(name);
            body["shared"] = json!(true);
            body
        };
        assert_eq!(
            create(f, &admin, shared("sixty-fourth"), None).await?.0,
            201
        );
        assert_eq!(
            code(&create(f, &admin, shared("one-more"), None).await?),
            (409, json!("profile_count"))
        );
        Ok(())
    })
    .await
}
