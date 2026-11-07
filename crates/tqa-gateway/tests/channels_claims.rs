//! GW-I-033 (claims; ADR-0014 point 8, G6): `task_claim` is atomic up to the task's limit (1 by default, a number, or
//! every voting member); a claimant claims once, and a retry returns its claim; only claimants report; the task board
//! shows claims and reports. People claim from the page, runs through the runtime route.
#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// A channel led by `cookie`'s user with `members` people as voting members; their cookies.
async fn channel(f: &Fixture, cookie: &str, members: usize) -> Result<(String, Vec<String>)> {
    let (_, created) = call(
        f,
        Method::POST,
        "/api/v1/channels",
        cookie,
        (Some(json!({"name": "work", "rule": "majority"})), None),
    )
    .await?;
    let id = created["id"].as_str().ok_or("id")?.to_owned();
    let mut cookies = Vec::new();
    for index in 0..members {
        let (user, member_cookie) = f.member(&format!("m{index}@example.invalid")).await?;
        let (status, _) = call(
            f,
            Method::PUT,
            &format!("/api/v1/channels/{id}/members/user/{}", user.id),
            cookie,
            (Some(json!({"role": "member"})), None),
        )
        .await?;
        assert_eq!(status, 200);
        cookies.push(member_cookie);
    }
    Ok((id, cookies))
}

async fn task(f: &Fixture, cookie: &str, id: &str, claims: Value, key: &str) -> Result<u64> {
    let mut body = json!({"type": "task", "title": "parse", "spec": "the file"});
    if !claims.is_null() {
        body["claims"] = claims;
    }
    let (status, posted) = call(
        f,
        Method::POST,
        &format!("/api/v1/channels/{id}/messages"),
        cookie,
        (Some(json!({"body": body})), Some(key)),
    )
    .await?;
    assert_eq!(status, 201, "{posted}");
    posted["seq"].as_u64().ok_or_else(|| "seq".into())
}

async fn claim(f: &Fixture, cookie: &str, id: &str, seq: u64) -> Result<(u16, Value)> {
    call(
        f,
        Method::POST,
        &format!("/api/v1/channels/{id}/tasks/{seq}/claim"),
        cookie,
        (None, None),
    )
    .await
}

/// Eight concurrent claims; how many won and the losers' codes.
async fn race(f: &Fixture, cookies: &[String], id: &str, seq: u64) -> Result<(usize, Vec<Value>)> {
    let results =
        futures::future::join_all(cookies.iter().map(|cookie| claim(f, cookie, id, seq))).await;
    let mut won = 0;
    let mut lost = Vec::new();
    for result in results {
        let (status, body) = result?;
        match status {
            201 => won += 1,
            _ => lost.push(json!([status, body["error"]["code"]])),
        }
    }
    Ok((won, lost))
}

// TEST: GW-I-033
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_033_claims_are_atomic_up_to_the_tasks_limit(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let (id, cookies) = channel(f, &cookie, 8).await?;
        let mut last = 0;
        for (claims, winners) in [(Value::Null, 1), (json!(3), 3), (json!("all"), 8)] {
            let seq = task(f, &cookie, &id, claims.clone(), &format!("task-{claims}")).await?;
            let (won, lost) = race(f, &cookies, &id, seq).await?;
            last = seq;
            assert_eq!(won, winners, "{claims}");
            assert!(
                lost.iter()
                    .all(|loss| *loss == json!([409, "already_claimed"])),
                "{claims}: {lost:?}"
            );
        }
        // "all" counts every voting member, the lead included: the ninth claim is the lead's, and it wins.
        let (status, _) = claim(f, &cookie, &id, last).await?;
        assert_eq!(status, 201);
        Ok(())
    })
    .await
}

// TEST: GW-I-033
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_033_a_claimant_claims_once_and_only_claimants_report(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let (id, cookies) = channel(f, &cookie, 2).await?;
        let seq = task(f, &cookie, &id, json!(2), "t").await?;
        let (status, first) = claim(f, &cookies[0], &id, seq).await?;
        assert_eq!((status, &first["body"]), (201, &json!({"type": "claim", "task_seq": seq})), "{first}");
        let (status, again) = claim(f, &cookies[0], &id, seq).await?;
        assert_eq!((status, &again), (200, &first), "a retry returns the claim");
        // Not a task, an unknown seq, an observer.
        let (_, text) = call(f, Method::POST, &format!("/api/v1/channels/{id}/messages"), &cookie, (Some(json!({"body": {"type": "text", "text": "hi"}})), Some("x"))).await?;
        let (status, body) = claim(f, &cookie, &id, text["seq"].as_u64().ok_or("seq")?).await?;
        assert_eq!((status, &body["error"]["code"]), (409, &json!("not_a_task")));
        let (status, _) = claim(f, &cookie, &id, 999).await?;
        assert_eq!(status, 404);
        let (watcher, watcher_cookie) = f.member("watch@example.invalid").await?;
        call(f, Method::PUT, &format!("/api/v1/channels/{id}/members/user/{}", watcher.id), &cookie, (Some(json!({"role": "observer"})), None)).await?;
        let (status, body) = claim(f, &watcher_cookie, &id, seq).await?;
        assert_eq!((status, &body["error"]["code"]), (403, &json!("observer")));
        // Reports: the claimant's is accepted, anyone else's refused.
        let report = |key: &str| (Some(json!({"body": {"type": "report", "task_seq": seq, "status": "completed", "summary": "done"}})), Some(key.to_owned()));
        let (body, key) = report("r1");
        let (status, _) = call(f, Method::POST, &format!("/api/v1/channels/{id}/messages"), &cookies[0], (body, key.as_deref())).await?;
        assert_eq!(status, 201);
        let (body, key) = report("r2");
        let (status, refused) = call(f, Method::POST, &format!("/api/v1/channels/{id}/messages"), &cookies[1], (body, key.as_deref())).await?;
        assert_eq!((status, &refused["error"]["code"]), (403, &json!("not_claimant")));
        // The task board: the task, its limit, its claim and its report.
        let (status, board) = call(f, Method::GET, &format!("/api/v1/channels/{id}/tasks"), &cookies[1], (None, None)).await?;
        assert_eq!(status, 200, "{board}");
        let item = &board["items"][0];
        assert_eq!((&item["task_seq"], &item["title"], &item["limit"]), (&json!(seq), &json!("parse"), &json!(2)));
        assert_eq!(item["claims"].as_array().map(Vec::len), Some(1));
        assert_eq!(item["reports"][0]["status"], "completed");
        Ok(())
    })
    .await
}

// TEST: GW-I-033
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_033_a_run_claims_through_the_runtime_route(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let agent = f.conversation(owner.id).await?;
        let _runtime = f.runtime(&agent).await?;
        let token = f
            .backend
            .token(&agent.id)
            .ok_or("token")?
            .expose_secret()
            .to_owned();
        let (id, _) = channel(f, &cookie, 0).await?;
        call(
            f,
            Method::PUT,
            &format!("/api/v1/channels/{id}/members/conversation/{}", agent.id),
            &cookie,
            (Some(json!({"role": "member"})), None),
        )
        .await?;
        let seq = task(f, &cookie, &id, Value::Null, "t").await?;
        let response = f
            .client
            .post(f.url(&format!("/runtime/channels/{id}/tasks/{seq}/claim")))
            .bearer_auth(&token)
            .send()
            .await?;
        assert_eq!(response.status().as_u16(), 201);
        let claimed: Value = response.json().await?;
        assert_eq!(
            (&claimed["author"]["kind"], &claimed["author"]["id"]),
            (&json!("conversation"), &json!(agent.id.as_str()))
        );
        // The person comes second: the one claim is taken.
        let (status, body) = claim(f, &cookie, &id, seq).await?;
        assert_eq!(
            (status, &body["error"]["code"]),
            (409, &json!("already_claimed"))
        );
        Ok(())
    })
    .await
}
