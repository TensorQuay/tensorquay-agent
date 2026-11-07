//! GW-I-117: the workspaces list pages, sorts and filters. `limit` pages with an opaque `next_cursor` that stays exact
//! while workspaces are added; `sort` is `recent` (last used, else created), `name` or `created`; `filter` is `in_use`
//! (its write lease is held), `idle` or `shared_with_me`. Without `limit` the answer is the whole list, as before.
#[path = "support/fixture.rs"]
mod support;

use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn call(f: &Fixture, cookie: &str, method: Method, path: &str) -> reqwest::RequestBuilder {
    f.client
        .request(method, f.url(path))
        .header("origin", f.origin())
        .header("cookie", cookie)
}

async fn get(f: &Fixture, cookie: &str, path: &str) -> Result<(u16, Value)> {
    let response = call(f, cookie, Method::GET, path).send().await?;
    Ok((
        response.status().as_u16(),
        response.json().await.unwrap_or(Value::Null),
    ))
}

async fn create(f: &Fixture, cookie: &str, name: &str) -> Result<String> {
    let created: Value = call(f, cookie, Method::POST, "/api/v1/workspaces")
        .json(&json!({"name": name}))
        .send()
        .await?
        .json()
        .await?;
    Ok(created["id"].as_str().ok_or("id")?.to_owned())
}

fn names(page: &Value) -> Vec<String> {
    page["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item["name"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// Every name, page by page, following `next_cursor`.
async fn all_pages(f: &Fixture, cookie: &str, query: &str, limit: u32) -> Result<Vec<String>> {
    let mut seen = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..50 {
        let path = match &cursor {
            Some(cursor) => format!("/api/v1/workspaces?{query}&limit={limit}&cursor={cursor}"),
            None => format!("/api/v1/workspaces?{query}&limit={limit}"),
        };
        let (status, page) = get(f, cookie, &path).await?;
        assert_eq!(status, 200, "{path}: {page}");
        let items = names(&page);
        assert!(items.len() <= limit as usize);
        seen.extend(items);
        match page["next_cursor"].as_str() {
            Some(next) => cursor = Some(next.to_owned()),
            None => return Ok(seen),
        }
    }
    Err("the pages never ended".into())
}

async fn set_used(f: &Fixture, id: &str, minutes_ago: i32) -> TestResult {
    sqlx::query("UPDATE workspaces SET last_used_at = CURRENT_TIMESTAMP - make_interval(mins => $2) WHERE id = $1::uuid")
        .bind(id)
        .bind(minutes_ago)
        .execute(&f.pool)
        .await?;
    Ok(())
}

// TEST: GW-I-117
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_117_the_workspaces_list_pages_sorts_and_filters(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (admin, cookie) = f.admin().await?;
        let (_, other_cookie) = f.member("other@example.invalid").await?;
        let mut ids = std::collections::HashMap::new();
        for name in ["delta", "alpha", "echo", "charlie", "bravo"] {
            ids.insert(name, create(f, &cookie, name).await?);
        }
        // Someone else's workspace, shared with the caller.
        let theirs = create(f, &other_cookie, "shared-in").await?;
        let status = call(f, &other_cookie, Method::PUT, &format!("/api/v1/workspaces/{theirs}/shares/{}", admin.id))
            .json(&json!({"access": "read"}))
            .send()
            .await?
            .status();
        assert!(status.is_success(), "share: {status}");
        // One in use: its write lease is held.
        let conversation = f.conversation(admin.id).await?;
        sqlx::query("UPDATE workspaces SET lease_conversation_id = $2::uuid, lease_expires_at = CURRENT_TIMESTAMP + INTERVAL '1 minute' WHERE id = $1::uuid")
            .bind(&ids["charlie"])
            .bind(conversation.id.as_str())
            .execute(&f.pool)
            .await?;

        // Everything was created a day ago; then bravo was used most recently, and echo before it. The rest were never
        // used, so they sort by creation, after both.
        sqlx::query("UPDATE workspaces SET created_at = created_at - INTERVAL '1 day'").execute(&f.pool).await?;
        set_used(f, &ids["echo"], 30).await?;
        set_used(f, &ids["bravo"], 5).await?;
        // Without limit: the whole list, no cursor, as before.
        let (status, everything) = get(f, &cookie, "/api/v1/workspaces").await?;
        assert_eq!(status, 200);
        assert!(everything["next_cursor"].is_null());
        let total = names(&everything).len();
        assert!(total >= 6, "{everything}");

        // Sorts, paged two at a time, equal the unpaged sort.
        let recent = all_pages(f, &cookie, "sort=recent", 2).await?;
        assert_eq!(&recent[..2], ["bravo", "echo"], "last used first: {recent:?}");
        assert_eq!(recent.len(), total);
        let by_name = all_pages(f, &cookie, "sort=name", 2).await?;
        let mut sorted = by_name.clone();
        sorted.sort_by_key(|name| name.to_lowercase());
        assert_eq!(by_name, sorted);
        let created = all_pages(f, &cookie, "sort=created", 3).await?;
        assert_eq!(created.len(), total);
        assert_eq!(created.iter().position(|n| n == "bravo"), Some(created.iter().position(|n| n == "charlie").ok_or("c")? - 1));

        // Filters.
        assert_eq!(all_pages(f, &cookie, "filter=in_use", 10).await?, ["charlie"]);
        let idle = all_pages(f, &cookie, "filter=idle", 10).await?;
        assert!(!idle.contains(&"charlie".to_owned()) && idle.len() == total - 1, "{idle:?}");
        assert_eq!(all_pages(f, &cookie, "filter=shared_with_me", 10).await?, ["shared-in"]);

        // A cursor stays exact while a workspace is added: no repeat, no gap.
        let (_, first) = get(f, &cookie, "/api/v1/workspaces?sort=created&limit=2").await?;
        let cursor = first["next_cursor"].as_str().ok_or("cursor")?.to_owned();
        create(f, &cookie, "late").await?;
        let (_, second) = get(f, &cookie, &format!("/api/v1/workspaces?sort=created&limit=2&cursor={cursor}")).await?;
        let (first, second) = (names(&first), names(&second));
        assert!(first.iter().all(|name| !second.contains(name)) && !second.contains(&"late".to_owned()), "{first:?} {second:?}");

        // Refusals.
        for query in ["limit=0", "limit=101", "sort=size", "filter=mine", "cursor=not-a-cursor&limit=2"] {
            let (status, body) = get(f, &cookie, &format!("/api/v1/workspaces?{query}")).await?;
            assert_eq!(status, 400, "{query}: {body}");
        }
        // A cursor belongs to its sort and filter.
        let (status, _) = get(f, &cookie, &format!("/api/v1/workspaces?sort=name&limit=2&cursor={cursor}")).await?;
        assert_eq!(status, 400);
        Ok(())
    })
    .await
}
