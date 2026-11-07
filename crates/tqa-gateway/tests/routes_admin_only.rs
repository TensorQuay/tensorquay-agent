//! GW-I-038 (PRD §12 item 14, ADR-0028 amendment): routes are the administrators'. A member, by session or by API
//! token, is refused (`403`) on every route change (create, edit, delete) and on a probe, and nothing changes; a
//! member's task lists, binds and switches to administrators' routes only, and its runtime gets the administrator's
//! route.
#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn route(model: &str) -> Value {
    json!({"protocol": "openai_chat", "base_url": "https://models.example/v1", "model": model, "auth": {"kind": "none"}})
}

/// The administrator's view of every route: name, model and version.
async fn routes(f: &Fixture, admin: &str) -> Result<Vec<(String, String, i64)>> {
    let (status, body) = call(f, Method::GET, "/api/v1/admin/routes", admin, (None, None)).await?;
    assert_eq!(status, 200, "{body}");
    Ok(body["items"]
        .as_array()
        .ok_or("items")?
        .iter()
        .map(|item| {
            (
                item["name"].as_str().unwrap_or_default().to_owned(),
                item["model"].as_str().unwrap_or_default().to_owned(),
                item["version"].as_i64().unwrap_or_default(),
            )
        })
        .collect())
}

/// Every route change and the probe, as `(method, path, body)`, against the administrator's route `shared`.
fn changes() -> Vec<(Method, &'static str, Option<Value>)> {
    let mut edit = route("member-edit");
    edit["version"] = json!(1);
    vec![
        (
            Method::PUT,
            "/api/v1/admin/routes/mine",
            Some(route("member-own")),
        ),
        (Method::PUT, "/api/v1/admin/routes/shared", Some(edit)),
        (
            Method::DELETE,
            "/api/v1/admin/routes/shared?version=1",
            None,
        ),
        (Method::POST, "/api/v1/admin/routes/shared/probe", None),
    ]
}

async fn token(f: &Fixture, cookie: &str) -> Result<String> {
    let (status, body) = call(
        f,
        Method::POST,
        "/api/v1/tokens",
        cookie,
        (Some(json!({"name": "laptop", "expires_in_days": 1})), None),
    )
    .await?;
    assert_eq!(status, 201, "{body}");
    Ok(body["token"].as_str().ok_or("token")?.to_owned())
}

/// The routes a person may choose, sorted.
async fn offered(f: &Fixture, cookie: &str) -> Result<Vec<String>> {
    let models: Value = f.get("/api/v1/models", cookie).await?.json().await?;
    let mut names: Vec<String> = models["items"]
        .as_array()
        .ok_or("items")?
        .iter()
        .filter_map(|item| item["name"].as_str().map(str::to_owned))
        .collect();
    names.sort_unstable();
    Ok(names)
}

// TEST: GW-I-038
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_038_a_member_cannot_create_edit_delete_or_probe_a_route(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, admin) = f.admin().await?;
        let (status, body) = call(
            f,
            Method::PUT,
            "/api/v1/admin/routes/shared",
            &admin,
            (Some(route("shared-model")), None),
        )
        .await?;
        assert_eq!(status, 201, "{body}");
        let before = routes(f, &admin).await?;
        let (_, member) = f.member("member@example.invalid").await?;
        let bearer = token(f, &member).await?;
        for (method, path, body) in changes() {
            let (status, answer) = call(f, method.clone(), path, &member, (body.clone(), None)).await?;
            assert_eq!(status, 403, "session {method} {path}: {answer}");
            let mut request = f.client.request(method.clone(), f.url(path)).bearer_auth(&bearer);
            if let Some(body) = body {
                request = request.json(&body);
            }
            assert_eq!(request.send().await?.status(), 403, "token {method} {path}");
        }
        assert_eq!(routes(f, &admin).await?, before, "nothing changed");
        let audited: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM admin_audit a JOIN users u ON u.id=a.actor_user_id WHERE u.email='member@example.invalid'",
        )
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(audited, 0, "a refused change records nothing");
        // The same edit is well formed: the administrator's goes through.
        let (method, path, body) = changes().swap_remove(1);
        let (status, body) = call(f, method, path, &admin, (body, None)).await?;
        assert_eq!(status, 200, "{body}");
        Ok(())
    })
    .await
}

// TEST: GW-I-038
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_038_a_members_task_uses_only_administrators_routes(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, admin) = f.admin().await?;
        let names: Vec<String> = routes(f, &admin)
            .await?
            .into_iter()
            .map(|(name, ..)| name)
            .collect();
        let (_, member) = f.member("member@example.invalid").await?;
        // The member's list is the administrators' routes.
        let listed = offered(f, &member).await?;
        let mut expected = names.clone();
        expected.sort_unstable();
        assert_eq!(listed, expected);
        // A name no administrator made can't be bound at creation.
        let refused = f
            .action(
                "/api/v1/conversations",
                &member,
                Some("own"),
                &json!({"title": "Own", "model_route": "mine"}),
            )
            .await?;
        assert_eq!(refused.status(), 403);
        let chosen = names.first().ok_or("an administrator's route")?;
        let created = f
            .action(
                "/api/v1/conversations",
                &member,
                Some("task"),
                &json!({"title": "Task", "model_route": chosen}),
            )
            .await?;
        assert_eq!(created.status(), 201);
        let task: Value = created.json().await?;
        let id: tqa_types::ConversationId = task["id"].as_str().ok_or("id")?.parse()?;
        // Nor switched to later.
        let switched = f
            .client
            .patch(f.url(&format!("/api/v1/conversations/{id}")))
            .header("origin", f.origin())
            .header("cookie", &member)
            .header("idempotency-key", "switch")
            .json(&json!({"route": "mine"}))
            .send()
            .await?;
        assert_eq!(switched.status(), 403);
        // Its runtime gets the administrator's route.
        let runtime = f.runtime_for(&id).await?;
        assert_eq!(runtime.config["route"]["name"], json!(chosen));
        Ok(())
    })
    .await
}
