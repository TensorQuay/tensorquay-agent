//! GW-I-033 (claims; ADR-0014 point 8): `task_claim` is atomic up to the task's limit (1 by default, a number, or
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

/// A runtime call with the conversation's token: the status and the JSON answer (null when empty).
async fn as_run(
    f: &Fixture,
    method: Method,
    path: &str,
    token: &str,
    body: Option<Value>,
) -> Result<(u16, Value)> {
    let mut request = f.client.request(method, f.url(path)).bearer_auth(token);
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await?;
    let status = response.status().as_u16();
    let text = response.text().await?;
    Ok((
        status,
        if text.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text)?
        },
    ))
}

/// A run's claim through the runtime route.
async fn claim_as_run(f: &Fixture, token: &str, id: &str, seq: u64) -> Result<(u16, Value)> {
    as_run(
        f,
        Method::POST,
        &format!("/runtime/channels/{id}/tasks/{seq}/claim"),
        token,
        None,
    )
    .await
}

/// A run's report on `seq` with `status`, through the runtime route.
async fn report(
    f: &Fixture,
    token: &str,
    id: &str,
    seq: u64,
    status: &str,
) -> Result<(u16, Value)> {
    let body = json!({"body": {"type": "report", "task_seq": seq, "status": status, "summary": "where it stands"}});
    as_run(
        f,
        Method::POST,
        &format!("/runtime/channels/{id}/messages"),
        token,
        Some(body),
    )
    .await
}

// TEST: GW-I-384
// C-13 (21 Nov 2026): an agent holds one open to-do at a time in a channel. A claim by a conversation that holds an
// unreported claim on another to-do there is refused one_at_a_time, in words naming the to-do it holds; a retry of the
// held claim still answers 200; any report on the held to-do (here `blocked`, then `failed`) frees the agent to claim
// the next. People are not limited.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_384_an_agent_holds_one_open_to_do_at_a_time(pool: PgPool) -> TestResult {
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
        let (id, people) = channel(f, &cookie, 1).await?;
        call(
            f,
            Method::PUT,
            &format!("/api/v1/channels/{id}/members/conversation/{}", agent.id),
            &cookie,
            (Some(json!({"role": "member"})), None),
        )
        .await?;
        let mut todos = Vec::new();
        for key in ["t1", "t2", "t3"] {
            todos.push(task(f, &cookie, &id, Value::Null, key).await?);
        }
        let (one, two, three) = (todos[0], todos[1], todos[2]);
        let (status, first) = claim_as_run(f, &token, &id, one).await?;
        assert_eq!(status, 201, "{first}");
        // A second to-do while the first is open: refused, in words that say what to do.
        let (status, refused) = claim_as_run(f, &token, &id, two).await?;
        assert_eq!(
            (status, &refused["error"]["code"]),
            (409, &json!("one_at_a_time")),
            "{refused}"
        );
        let said = refused["error"]["message"].as_str().ok_or("message")?;
        assert!(said.contains(&format!("to-do {one}")), "{said}");
        assert!(
            said.contains("report") && said.contains("then claim"),
            "{said}"
        );
        // The held claim's retry is still the claim it holds.
        let (status, again) = claim_as_run(f, &token, &id, one).await?;
        assert_eq!((status, &again["seq"]), (200, &first["seq"]), "{again}");
        // Any report closes the hold: blocked frees the agent for the second, failed for the third.
        let (status, posted) = report(f, &token, &id, one, "blocked").await?;
        assert_eq!(status, 201, "{posted}");
        let (status, second) = claim_as_run(f, &token, &id, two).await?;
        assert_eq!(status, 201, "{second}");
        let (status, refused) = claim_as_run(f, &token, &id, three).await?;
        assert_eq!(status, 409, "{refused}");
        let said = refused["error"]["message"].as_str().ok_or("message")?;
        assert!(said.contains(&format!("to-do {two}")), "{said}");
        let (status, posted) = report(f, &token, &id, two, "failed").await?;
        assert_eq!(status, 201, "{posted}");
        let (status, third) = claim_as_run(f, &token, &id, three).await?;
        assert_eq!(status, 201, "{third}");
        // A person holds two open to-dos at once.
        let (four, five) = (
            task(f, &cookie, &id, Value::Null, "t4").await?,
            task(f, &cookie, &id, Value::Null, "t5").await?,
        );
        for seq in [four, five] {
            let (status, body) = claim(f, &people[0], &id, seq).await?;
            assert_eq!(status, 201, "{body}");
        }
        Ok(())
    })
    .await
}

/// The listing's counts for one channel: (open votes, open to-dos).
async fn work_left(f: &Fixture, cookie: &str, id: &str) -> Result<(Value, Value)> {
    let (status, listed) = call(f, Method::GET, "/api/v1/channels", cookie, (None, None)).await?;
    assert_eq!(status, 200, "{listed}");
    let item = listed["items"]
        .as_array()
        .ok_or("items")?
        .iter()
        .find(|item| item["id"] == id)
        .ok_or("the channel isn't listed")?;
    Ok((item["open_votes"].clone(), item["todos_open"].clone()))
}

async fn post_body(f: &Fixture, cookie: &str, id: &str, body: Value, key: &str) -> Result<u64> {
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

// TEST: GW-I-400
/// GW-I-400 (18.3.0; the product owner, 27 Nov 2026: "inside complete, outside open"): the channel list says how much
/// work is left: the proposals not yet decided, and the to-dos whose places aren't all taken by members whose latest
/// report is completed ("all" counts every voting member; failed or blocked is still open).
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_400_the_channel_list_counts_open_votes_and_open_todos(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let (id, cookies) = channel(f, &cookie, 2).await?;
        assert_eq!(work_left(f, &cookie, &id).await?, (json!(0), json!(0)));
        // A proposal is open until it is decided.
        let proposal = post_body(f, &cookie, &id, json!({"type": "proposal", "title": "Ship", "text": "now"}), "p").await?;
        assert_eq!(work_left(f, &cookie, &id).await?, (json!(1), json!(0)));
        // A to-do of two places: open while a place is free, or a taker's latest report isn't completed.
        let todo = task(f, &cookie, &id, json!(2), "t").await?;
        assert_eq!(work_left(f, &cookie, &id).await?.1, json!(1));
        let report = |status: &str| json!({"type": "report", "task_seq": todo, "status": status, "summary": "s"});
        claim(f, &cookies[0], &id, todo).await?;
        post_body(f, &cookies[0], &id, report("completed"), "r0").await?;
        assert_eq!(work_left(f, &cookie, &id).await?.1, json!(1), "a place is still free");
        claim(f, &cookies[1], &id, todo).await?;
        post_body(f, &cookies[1], &id, report("blocked"), "r1").await?;
        assert_eq!(work_left(f, &cookie, &id).await?.1, json!(1), "blocked is still open");
        post_body(f, &cookies[1], &id, report("completed"), "r2").await?;
        assert_eq!(work_left(f, &cookie, &id).await?.1, json!(0), "every place completed");
        // "all": every voting member, the lead included (three here).
        task(f, &cookie, &id, json!("all"), "t-all").await?;
        assert_eq!(work_left(f, &cookie, &id).await?.1, json!(1));
        // Two of three vote accept: decided, so no vote is open.
        for (index, member) in cookies.iter().enumerate() {
            post_body(f, member, &id, json!({"type": "vote", "proposal_seq": proposal, "choice": "accept"}), &format!("v{index}")).await?;
        }
        assert_eq!(work_left(f, &cookie, &id).await?.0, json!(0));
        Ok(())
    })
    .await
}
