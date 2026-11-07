//! The sidebar's and the Workspaces page's lists: each task's run status, and a search on both lists.
//! - GW-I-120: each item in the task list carries its run status, from the channels' lifecycle projection.
//! - GW-I-121: `q` on the task list matches the title case-insensitively, as literal text, and pages.
//! - GW-I-122: `q` on the workspaces list matches the name as literal text, with sort, filter and paging; a cursor
//!   belongs to its `q`.
//! - SEC-044: `q` never matches a title or name the caller may not read, even for an administrator listing all.
#[path = "support/fixture.rs"]
mod support;

use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

async fn get(f: &Fixture, cookie: &str, path: &str) -> Result<(u16, Value)> {
    let response = f
        .client
        .request(Method::GET, f.url(path))
        .header("origin", f.origin())
        .header("cookie", cookie)
        .send()
        .await?;
    Ok((
        response.status().as_u16(),
        response.json().await.unwrap_or(Value::Null),
    ))
}

fn field(page: &Value, key: &str) -> Vec<String> {
    page["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item[key].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// `q` as a query-string value.
fn encoded(q: &str) -> String {
    url::form_urlencoded::byte_serialize(q.as_bytes()).collect()
}

/// A task of `owner` titled `title`, active `minute` minutes past a fixed instant.
async fn task(f: &Fixture, owner: uuid::Uuid, title: &str, minute: i32) -> Result<uuid::Uuid> {
    let id = uuid::Uuid::parse_str(f.conversation(owner).await?.id.as_str())?;
    sqlx::query("UPDATE conversations SET title = $2, last_activity_at = TIMESTAMPTZ '2026-01-01 00:00:00Z' + make_interval(mins => $3) WHERE id = $1")
        .bind(id)
        .bind(title)
        .bind(minute)
        .execute(&f.pool)
        .await?;
    Ok(id)
}

async fn workspace(f: &Fixture, cookie: &str, name: &str) -> Result<String> {
    let created: Value = f
        .client
        .request(Method::POST, f.url("/api/v1/workspaces"))
        .header("origin", f.origin())
        .header("cookie", cookie)
        .json(&json!({"name": name}))
        .send()
        .await?
        .json()
        .await?;
    Ok(created["id"].as_str().ok_or("id")?.to_owned())
}

/// Every value of `key`, page by page, following `next_cursor`.
async fn all_pages(f: &Fixture, cookie: &str, path: &str, key: &str) -> Result<Vec<String>> {
    let mut seen = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..50 {
        let url = match &cursor {
            Some(cursor) => format!("{path}&limit=1&cursor={cursor}"),
            None => format!("{path}&limit=1"),
        };
        let (status, page) = get(f, cookie, &url).await?;
        assert_eq!(status, 200, "{url}: {page}");
        seen.extend(field(&page, key));
        match page["next_cursor"].as_str() {
            Some(next) => cursor = Some(next.to_owned()),
            None => return Ok(seen),
        }
    }
    Err("the pages never ended".into())
}

/// A lifecycle event as the event store keeps it: the typed `lifecycle` column the projection reads.
async fn lifecycle(f: &Fixture, conversation: uuid::Uuid, seq: i64, kind: &str) -> TestResult {
    sqlx::query(
        "INSERT INTO events (conversation_id, seq, ts, frame, lifecycle) \
         VALUES ($1, $2, CURRENT_TIMESTAMP, '{}'::jsonb, $3)",
    )
    .bind(conversation)
    .bind(seq)
    .bind(kind)
    .execute(&f.pool)
    .await?;
    Ok(())
}

/// One task's lifecycle events and its expected status.
type Case<'a> = (&'a [&'a str], &'a str);

// TEST: GW-I-120
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_120_each_listed_task_carries_its_run_status(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        f.admin().await?;
        let (member, cookie) = f.member("member@example.invalid").await?;
        let cases: [Case<'_>; 7] = [
            (&[], "idle"),
            (&["run.started"], "running"),
            (&["run.started", "run.resumed"], "running"),
            // A run parked for an approval before ADR-0035 has ended.
            (&["run.started", "run.parked"], "failed"),
            (&["run.started", "run.completed"], "completed"),
            (&["run.started", "run.resumed", "run.failed"], "failed"),
            (&["run.started", "run.cancelled"], "cancelled"),
        ];
        let mut expected = std::collections::BTreeMap::new();
        for (minute, (events, status)) in (1..).zip(cases) {
            let id = task(f, member.id, &format!("task {minute}"), minute).await?;
            for (index, kind) in events.iter().enumerate() {
                lifecycle(f, id, i64::try_from(index)? + 1, kind).await?;
            }
            expected.insert(id.to_string(), status);
        }
        // The whole list, and page by page: the status comes with the item either way.
        let (status, listed) = get(f, &cookie, "/api/v1/conversations").await?;
        assert_eq!(status, 200, "{listed}");
        let items = listed["items"].as_array().ok_or("items")?;
        assert_eq!(items.len(), 7);
        for item in items {
            let id = item["id"].as_str().ok_or("id")?;
            assert_eq!(item["run_status"], json!(expected[id]), "{item}");
        }
        let (_, first) = get(f, &cookie, "/api/v1/conversations?limit=3").await?;
        let statuses: Vec<&str> = first["items"]
            .as_array()
            .ok_or("items")?
            .iter()
            .filter_map(|item| item["run_status"].as_str())
            .collect();
        assert_eq!(
            statuses,
            ["cancelled", "failed", "completed"],
            "newest first"
        );
        // One task's own read carries the same status, so a page opened on a task knows it before the history replays.
        for (id, status) in &expected {
            let (code, read) = get(f, &cookie, &format!("/api/v1/conversations/{id}")).await?;
            assert_eq!(code, 200, "{read}");
            assert_eq!(read["run_status"], json!(status), "{read}");
        }
        Ok(())
    })
    .await
}

// TEST: GW-I-121
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_121_q_on_the_task_list_matches_the_title_as_literal_text(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        f.admin().await?;
        let (member, cookie) = f.member("member@example.invalid").await?;
        let titles = [
            "Fix the date range",
            "Add CSV export",
            "fix DATE parsing",
            "50% off",
            "500 items",
            "a_b",
            "axb",
            r"back\slash",
        ];
        for (minute, title) in (1..).zip(titles) {
            task(f, member.id, title, minute).await?;
        }
        let search = async |q: &str| -> Result<(u16, Value)> {
            get(
                f,
                &cookie,
                &format!("/api/v1/conversations?q={}", encoded(q)),
            )
            .await
        };
        let cases: [(&str, &[&str]); 7] = [
            ("date", &["fix DATE parsing", "Fix the date range"]),
            ("  csv ", &["Add CSV export"]),
            ("50%", &["50% off"]),
            ("a_b", &["a_b"]),
            ("%", &["50% off"]),
            (r"\", &[r"back\slash"]),
            ("nothing like it", &[]),
        ];
        for (q, expected) in cases {
            let (status, page) = search(q).await?;
            assert_eq!(status, 200, "{q:?}: {page}");
            assert_eq!(field(&page, "title"), expected, "{q:?}");
        }
        // Empty is no search; the list is whole.
        assert_eq!(field(&search("").await?.1, "title").len(), titles.len());
        // Paging keeps the search.
        assert_eq!(
            all_pages(f, &cookie, "/api/v1/conversations?q=fix", "title").await?,
            ["fix DATE parsing", "Fix the date range"]
        );
        // At most 100 characters.
        assert_eq!(search(&"a".repeat(100)).await?.0, 200);
        let (status, body) = search(&"a".repeat(101)).await?;
        assert_eq!(
            (status, &body["error"]["details"]),
            (400, &json!([{"field": "q", "reason": "value_too_long"}]))
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-122
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_122_q_on_the_workspaces_list_matches_the_name_as_literal_text(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        f.admin().await?;
        let (member, cookie) = f.member("member@example.invalid").await?;
        let (_, other_cookie) = f.member("other@example.invalid").await?;
        for name in ["alpha-api", "Alpha-web", "beta", "50% off", "500 items"] {
            workspace(f, &cookie, name).await?;
        }
        let theirs = workspace(f, &other_cookie, "shared-alpha").await?;
        let shared = f
            .client
            .request(
                Method::PUT,
                f.url(&format!("/api/v1/workspaces/{theirs}/shares/{}", member.id)),
            )
            .header("origin", f.origin())
            .header("cookie", &other_cookie)
            .json(&json!({"access": "read"}))
            .send()
            .await?
            .status();
        assert!(shared.is_success(), "share: {shared}");
        let names = async |query: &str| -> Result<Vec<String>> {
            let (status, page) = get(f, &cookie, &format!("/api/v1/workspaces?{query}")).await?;
            assert_eq!(status, 200, "{query}: {page}");
            Ok(field(&page, "name"))
        };
        assert_eq!(
            names("q=ALPHA&sort=name").await?,
            ["alpha-api", "Alpha-web", "shared-alpha"]
        );
        assert_eq!(
            names("q=alpha&filter=shared_with_me").await?,
            ["shared-alpha"]
        );
        assert_eq!(names(&format!("q={}", encoded("50%"))).await?, ["50% off"]);
        assert_eq!(names("q=").await?.len(), 6, "empty is no search");
        assert_eq!(
            all_pages(f, &cookie, "/api/v1/workspaces?q=alpha&sort=name", "name").await?,
            ["alpha-api", "Alpha-web", "shared-alpha"]
        );
        // A cursor belongs to its search, as to its sort and filter.
        let (_, first) = get(f, &cookie, "/api/v1/workspaces?q=alpha&sort=name&limit=1").await?;
        let cursor = first["next_cursor"].as_str().ok_or("cursor")?;
        let (status, body) = get(
            f,
            &cookie,
            &format!("/api/v1/workspaces?q=beta&sort=name&limit=1&cursor={cursor}"),
        )
        .await?;
        assert_eq!(
            (status, &body["error"]["details"]),
            (
                400,
                &json!([{"field": "cursor", "reason": "cursor_invalid"}])
            )
        );
        let (status, body) = get(
            f,
            &cookie,
            &format!("/api/v1/workspaces?q={}", "a".repeat(101)),
        )
        .await?;
        assert_eq!(
            (status, &body["error"]["details"]),
            (400, &json!([{"field": "q", "reason": "value_too_long"}]))
        );
        Ok(())
    })
    .await
}

// TEST: SEC-044
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_044_q_never_matches_what_the_caller_may_not_read(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (admin, admin_cookie) = f.admin().await?;
        let (alice, alice_cookie) = f.member("alice@example.invalid").await?;
        let (bob, bob_cookie) = f.member("bob@example.invalid").await?;
        let own = task(f, alice.id, "secret plan", 1).await?;
        let bobs = task(f, bob.id, "secret plan", 2).await?;
        let admins = task(f, admin.id, "secret admin note", 3).await?;
        let ids = |page: &Value| field(page, "id");
        // A member finds only their own tasks, a wildcard included.
        for q in ["secret", "%", "_"] {
            let (status, page) = get(
                f,
                &alice_cookie,
                &format!("/api/v1/conversations?q={}", encoded(q)),
            )
            .await?;
            assert_eq!(status, 200, "{q:?}: {page}");
            assert!(!ids(&page).contains(&bobs.to_string()), "{q:?}: {page}");
        }
        let (_, page) = get(f, &alice_cookie, "/api/v1/conversations?q=secret").await?;
        assert_eq!(ids(&page), [own.to_string()]);
        // An administrator listing all sees everyone's metadata but searches only titles they may read: their own.
        let (_, everyone) = get(f, &admin_cookie, "/api/v1/conversations?all=true").await?;
        assert_eq!(ids(&everyone).len(), 3, "all=true without q is unchanged");
        let (status, page) =
            get(f, &admin_cookie, "/api/v1/conversations?all=true&q=secret").await?;
        assert_eq!(status, 200, "{page}");
        assert_eq!(ids(&page), [admins.to_string()]);
        let (_, page) = get(f, &admin_cookie, "/api/v1/conversations?all=true&q=plan").await?;
        assert_eq!(
            ids(&page),
            Vec::<String>::new(),
            "no title the administrator may not read"
        );
        // Workspaces: an unshared name is never matched, for a member or an administrator listing all.
        workspace(f, &bob_cookie, "secret-bob").await?;
        workspace(f, &admin_cookie, "secret-admin").await?;
        let (_, page) = get(f, &alice_cookie, "/api/v1/workspaces?q=secret").await?;
        assert_eq!(field(&page, "id").len(), 0, "{page}");
        let (status, page) = get(f, &admin_cookie, "/api/v1/workspaces?all=true&q=secret").await?;
        assert_eq!(status, 200, "{page}");
        assert_eq!(field(&page, "name"), ["secret-admin"]);
        let (_, page) = get(f, &admin_cookie, "/api/v1/workspaces?all=true&q=bob").await?;
        assert_eq!(field(&page, "id").len(), 0, "{page}");
        Ok(())
    })
    .await
}
