//! GW-I-119: one search for a channel's new members, over people and the caller's own recent tasks. Tasks are the
//! caller's active conversations, newest activity first, matched on the title, each with its run status. People are,
//! for a member, only those they already work with (a shared workspace either way, a channel in common) by email
//! prefix, plus an exact email match of any active account; an administrator matches every account by prefix. The
//! caller is never listed. The exact lookup that can find a stranger is rate-limited per caller; nothing else is.
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
        .header("idempotency-key", uuid::Uuid::now_v7().to_string())
}

async fn search(f: &Fixture, cookie: &str, query: &str) -> Result<(u16, Value)> {
    let response = call(
        f,
        cookie,
        Method::GET,
        &format!("/api/v1/channels/candidates?{query}"),
    )
    .send()
    .await?;
    Ok((
        response.status().as_u16(),
        response.json().await.unwrap_or(Value::Null),
    ))
}

fn emails(body: &Value) -> Vec<String> {
    body["people"]
        .as_array()
        .map(|people| {
            people
                .iter()
                .filter_map(|p| p["email"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn titles(body: &Value) -> Vec<String> {
    body["tasks"]
        .as_array()
        .map(|tasks| {
            tasks
                .iter()
                .filter_map(|t| t["title"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

async fn task(f: &Fixture, cookie: &str, title: &str) -> Result<String> {
    let created: Value = call(f, cookie, Method::POST, "/api/v1/conversations")
        .json(&json!({"title": title, "model_route": "local"}))
        .send()
        .await?
        .json()
        .await?;
    Ok(created["id"].as_str().ok_or("task id")?.to_owned())
}

// TEST: GW-I-119
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_119_one_search_over_known_people_and_the_callers_recent_tasks(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, admin_cookie) = f.admin().await?;
        let (_, cookie) = f.member("me@example.invalid").await?;
        let (known, known_cookie) = f.member("known@example.invalid").await?;
        f.member("stranger@example.invalid").await?;
        let (_, other_cookie) = f.member("other@example.invalid").await?;
        // `known` shares a workspace with the caller.
        let created: Value = call(f, &known_cookie, Method::POST, "/api/v1/workspaces")
            .json(&json!({"name": "together"}))
            .send()
            .await?
            .json()
            .await?;
        let workspace = created["id"].as_str().ok_or("workspace")?.to_owned();
        let status = call(f, &known_cookie, Method::PUT, &format!("/api/v1/workspaces/{workspace}/shares"))
            .json(&json!({"email": "me@example.invalid", "access": "read"}))
            .send()
            .await?
            .status();
        assert!(status.is_success(), "share: {status}");
        let _ = known;
        // The caller's tasks, the second the most recently active; someone else's task never shows.
        let parse = task(f, &cookie, "Parse the release notes").await?;
        let build = task(f, &cookie, "Build the site").await?;
        task(f, &other_cookie, "Parse someone else's notes").await?;
        sqlx::query("UPDATE conversations SET last_activity_at = CURRENT_TIMESTAMP - INTERVAL '1 hour' WHERE id = $1::uuid")
            .bind(&parse)
            .execute(&f.pool)
            .await?;

        // No query: suggestions, the newest task first, and only the people the caller works with.
        let (status, body) = search(f, &cookie, "").await?;
        assert_eq!(status, 200, "{body}");
        assert_eq!(titles(&body), ["Build the site", "Parse the release notes"]);
        assert_eq!(emails(&body), ["known@example.invalid"]);
        assert_eq!(body["tasks"][0]["run_status"], "idle");
        assert_eq!(body["tasks"][0]["conversation_id"], build.as_str());
        // A title word; an email prefix of someone known; a stranger only by the exact address.
        assert_eq!(titles(&search(f, &cookie, "q=parse").await?.1), ["Parse the release notes"]);
        assert_eq!(emails(&search(f, &cookie, "q=kno").await?.1), ["known@example.invalid"]);
        assert!(emails(&search(f, &cookie, "q=stra").await?.1).is_empty(), "no directory for members");
        assert_eq!(emails(&search(f, &cookie, "q=Stranger@Example.invalid").await?.1), ["stranger@example.invalid"]);
        // The caller is never a candidate.
        assert!(emails(&search(f, &cookie, "q=me@example.invalid").await?.1).is_empty());
        // An administrator matches every account by prefix.
        assert_eq!(emails(&search(f, &admin_cookie, "q=stra").await?.1), ["stranger@example.invalid"]);

        // With a channel: who is already in it.
        let channel: Value = call(f, &cookie, Method::POST, "/api/v1/channels")
            .json(&json!({"name": "notes", "rule": "majority", "purpose": "parse them"}))
            .send()
            .await?
            .json()
            .await?;
        let channel = channel["id"].as_str().ok_or("channel")?.to_owned();
        let added = call(f, &cookie, Method::PUT, &format!("/api/v1/channels/{channel}/members/conversation/{parse}"))
            .json(&json!({"role": "member"}))
            .send()
            .await?
            .status();
        assert!(added.is_success(), "{added}");
        let (_, body) = search(f, &cookie, &format!("channel={channel}")).await?;
        let flags: Vec<(String, bool)> = body["tasks"]
            .as_array()
            .ok_or("tasks")?
            .iter()
            .map(|t| (t["title"].as_str().unwrap_or_default().to_owned(), t["already_member"] == true))
            .collect();
        assert_eq!(flags, [("Build the site".to_owned(), false), ("Parse the release notes".to_owned(), true)]);
        // Someone else's channel is not found.
        let (status, _) = search(f, &other_cookie, &format!("channel={channel}")).await?;
        assert_eq!(status, 404);

        // Refusals, and the rate limit on exact lookups only: searching by title or known prefix stays free.
        for query in ["q=".to_owned() + &"x".repeat(101), "limit=0".into(), "limit=51".into(), "channel=nope".into()] {
            let (status, _) = search(f, &cookie, &query).await?;
            assert!(status == 400 || status == 404, "{query}: {status}");
        }
        for _ in 0..15 {
            assert_eq!(search(f, &cookie, "q=parse").await?.0, 200);
        }
        let mut refused = false;
        for attempt in 0..12 {
            let (status, _) = search(f, &cookie, &format!("q=nobody{attempt}@example.invalid")).await?;
            refused |= status == 429;
        }
        assert!(refused, "exact lookups are rate-limited");
        Ok(())
    })
    .await
}
