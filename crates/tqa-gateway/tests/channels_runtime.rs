//! Channels from a run (ADR-0014 points 4 and 6; G4): `/runtime/channels…` with the run's token, bound to the
//! sandbox's address.
//! - GW-I-103: a member conversation lists its channels, posts and reads; a conversation that isn't a member gets
//!   `not_member`; a removed conversation loses access on its next call; sessions and other credentials get `401`.
//! - SEC-015: the author is the run's conversation, whatever the body says; a foreign token is `401`.
#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

async fn runtime_call(
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

// TEST: GW-I-103
// TEST: SEC-015
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_103_a_member_conversation_lists_posts_and_reads(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let member = f.conversation(owner.id).await?;
        let outsider = f.conversation(owner.id).await?;
        let _member_runtime = f.runtime(&member).await?;
        let _outsider_runtime = f.runtime(&outsider).await?;
        let token = f.backend.token(&member.id).ok_or("member token")?.expose_secret().to_owned();
        let foreign = f.backend.token(&outsider.id).ok_or("outsider token")?.expose_secret().to_owned();
        let (status, created) = call(f, Method::POST, "/api/v1/channels", &cookie, (Some(json!({"name": "build", "purpose": "ship it", "rule": "human"})), None)).await?;
        assert_eq!(status, 201, "{created}");
        let id = created["id"].as_str().ok_or("id")?.to_owned();
        let (status, _) = call(f, Method::PUT, &format!("/api/v1/channels/{id}/members/conversation/{}", member.id), &cookie, (Some(json!({"role": "member"})), None)).await?;
        assert_eq!(status, 200);
        // The run lists its channels; another conversation's run lists none.
        let (status, listed) = runtime_call(f, Method::GET, "/runtime/channels", &token, None).await?;
        assert_eq!(status, 200, "{listed}");
        assert_eq!(listed["channels"], json!([{"id": id, "name": "build", "purpose": "ship it", "role": "member", "rule": "human", "state": "open"}]));
        let (_, none) = runtime_call(f, Method::GET, "/runtime/channels", &foreign, None).await?;
        assert_eq!(none["channels"], json!([]));
        // A post: the author is the run's conversation, whatever the body says (SEC-015).
        let forged = json!({"body": {"type": "text", "text": "hi"}, "author": {"kind": "user", "id": owner.id.to_string()}});
        let (status, posted) = runtime_call(f, Method::POST, &format!("/runtime/channels/{id}/messages"), &token, Some(forged)).await?;
        assert_eq!(status, 201, "{posted}");
        assert_eq!((&posted["author"]["kind"], &posted["author"]["id"], &posted["seq"]), (&json!("conversation"), &json!(member.id.as_str()), &json!(1)));
        let (status, page) = runtime_call(f, Method::GET, &format!("/runtime/channels/{id}/messages?since=0&types=text"), &token, None).await?;
        assert_eq!((status, &page["messages"][0]["seq"], &page["more"]), (200, &json!(1), &json!(false)), "{page}");
        // Not a member: 403 not_member; an unknown channel: 404.
        for (method, path) in [(Method::GET, format!("/runtime/channels/{id}/messages")), (Method::POST, format!("/runtime/channels/{id}/messages"))] {
            let body = (method == Method::POST).then(|| json!({"body": {"type": "text", "text": "x"}}));
            let (status, refused) = runtime_call(f, method, &path, &foreign, body).await?;
            assert_eq!((status, &refused["error"]["code"]), (403, &json!("not_member")), "{path}");
        }
        let (status, _) = runtime_call(f, Method::GET, &format!("/runtime/channels/{}/messages", uuid::Uuid::now_v7()), &token, None).await?;
        assert_eq!(status, 404);
        // Only a runtime token, from its sandbox: a session, a garbage token and none are refused.
        let (status, _) = call(f, Method::GET, "/runtime/channels", &cookie, (None, None)).await?;
        assert_eq!(status, 401);
        let (status, _) = runtime_call(f, Method::GET, "/runtime/channels", "tqa_not_a_runtime_token", None).await?;
        assert_eq!(status, 401);
        // Removed mid-run: the next call is refused.
        let (status, _) = call(f, Method::DELETE, &format!("/api/v1/channels/{id}/members/conversation/{}", member.id), &cookie, (None, None)).await?;
        assert_eq!(status, 204);
        let (status, refused) = runtime_call(f, Method::POST, &format!("/runtime/channels/{id}/messages"), &token, Some(json!({"body": {"type": "text", "text": "still here?"}}))).await?;
        assert_eq!((status, &refused["error"]["code"]), (403, &json!("not_member")));
        Ok(())
    })
    .await
}

// TEST: GW-I-103
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_103_a_runtime_post_is_idempotent_by_its_key(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let member = f.conversation(owner.id).await?;
        let _runtime = f.runtime(&member).await?;
        let token = f
            .backend
            .token(&member.id)
            .ok_or("token")?
            .expose_secret()
            .to_owned();
        let (_, created) = call(
            f,
            Method::POST,
            "/api/v1/channels",
            &cookie,
            (Some(json!({"name": "b", "rule": "majority"})), None),
        )
        .await?;
        let id = created["id"].as_str().ok_or("id")?.to_owned();
        call(
            f,
            Method::PUT,
            &format!("/api/v1/channels/{id}/members/conversation/{}", member.id),
            &cookie,
            (Some(json!({"role": "member"})), None),
        )
        .await?;
        let post = || {
            f.client
                .post(f.url(&format!("/runtime/channels/{id}/messages")))
                .bearer_auth(&token)
                .header("idempotency-key", "run-1:call-7")
                .json(&json!({"body": {"type": "text", "text": "once"}}))
        };
        let first: Value = post().send().await?.json().await?;
        let again: Value = post().send().await?.json().await?;
        assert_eq!(first, again);
        let (_, page) = runtime_call(
            f,
            Method::GET,
            &format!("/runtime/channels/{id}/messages"),
            &token,
            None,
        )
        .await?;
        assert_eq!(page["messages"].as_array().map(Vec::len), Some(1));
        Ok(())
    })
    .await
}

// TEST: GW-I-032
// TEST: GW-I-103
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_032_a_wait_returns_the_matching_post_or_nothing_at_its_timeout(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let member = f.conversation(owner.id).await?;
        let _runtime = f.runtime(&member).await?;
        let token = f
            .backend
            .token(&member.id)
            .ok_or("token")?
            .expose_secret()
            .to_owned();
        let (_, created) = call(
            f,
            Method::POST,
            "/api/v1/channels",
            &cookie,
            (Some(json!({"name": "w", "rule": "majority"})), Some("c")),
        )
        .await?;
        let id = created["id"].as_str().ok_or("id")?.to_owned();
        call(
            f,
            Method::PUT,
            &format!("/api/v1/channels/{id}/members/conversation/{}", member.id),
            &cookie,
            (Some(json!({"role": "member"})), None),
        )
        .await?;
        // Waiting for a task: a text post doesn't end the wait, the task does, whichever way the requests race.
        let path = format!("/runtime/channels/{id}/messages?since=0&types=task&wait=30");
        let waiting = runtime_call(f, Method::GET, &path, &token, None);
        let posting = async {
            call(
                f,
                Method::POST,
                &format!("/api/v1/channels/{id}/messages"),
                &cookie,
                (
                    Some(json!({"body": {"type": "text", "text": "noise"}})),
                    Some("t1"),
                ),
            )
            .await?;
            call(
                f,
                Method::POST,
                &format!("/api/v1/channels/{id}/messages"),
                &cookie,
                (
                    Some(json!({"body": {"type": "task", "title": "t", "spec": "s"}})),
                    Some("t2"),
                ),
            )
            .await
        };
        let started = std::time::Instant::now();
        let (waited, posted) = tokio::join!(waiting, posting);
        let (status, page) = waited?;
        assert_eq!(posted?.0, 201);
        assert_eq!(
            (
                status,
                &page["messages"].as_array().map(Vec::len),
                &page["messages"][0]["body"]["type"]
            ),
            (200, &Some(1), &json!("task")),
            "{page}"
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(25),
            "woken by the post, not the timeout"
        );
        // A match already past `since` returns at once; nothing new returns empty when the wait runs out.
        let (_, at_once) = runtime_call(
            f,
            Method::GET,
            &format!("/runtime/channels/{id}/messages?since=0&wait=30"),
            &token,
            None,
        )
        .await?;
        assert_eq!(at_once["messages"].as_array().map(Vec::len), Some(2));
        let started = std::time::Instant::now();
        let (status, empty) = runtime_call(
            f,
            Method::GET,
            &format!("/runtime/channels/{id}/messages?since=2&wait=1"),
            &token,
            None,
        )
        .await?;
        assert_eq!(
            (
                status,
                &empty["messages"],
                &empty["next_since"],
                &empty["more"]
            ),
            (200, &json!([]), &json!(2), &json!(false))
        );
        assert!(
            started.elapsed() >= std::time::Duration::from_secs(1),
            "it waited"
        );
        // Past 300 s is refused, naming the field.
        let (status, refused) = runtime_call(
            f,
            Method::GET,
            &format!("/runtime/channels/{id}/messages?wait=301"),
            &token,
            None,
        )
        .await?;
        assert_eq!(
            (status, &refused["error"]["details"][0]["field"]),
            (400, &json!("wait"))
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-103
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_103_two_conversations_of_one_owner_may_use_the_same_key(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let (first, second) = (
            f.conversation(owner.id).await?,
            f.conversation(owner.id).await?,
        );
        let (_first_runtime, _second_runtime) =
            (f.runtime(&first).await?, f.runtime(&second).await?);
        let (_, created) = call(
            f,
            Method::POST,
            "/api/v1/channels",
            &cookie,
            (Some(json!({"name": "k", "rule": "majority"})), None),
        )
        .await?;
        let id = created["id"].as_str().ok_or("id")?.to_owned();
        let mut tokens = Vec::new();
        for conversation in [&first, &second] {
            call(
                f,
                Method::PUT,
                &format!(
                    "/api/v1/channels/{id}/members/conversation/{}",
                    conversation.id
                ),
                &cookie,
                (Some(json!({"role": "member"})), None),
            )
            .await?;
            tokens.push(
                f.backend
                    .token(&conversation.id)
                    .ok_or("token")?
                    .expose_secret()
                    .to_owned(),
            );
        }
        // Both runs are their conversation's first, and a model numbers its calls per turn: the same key, other bodies.
        let post = |token: &str, words: &str| {
            f.client
                .post(f.url(&format!("/runtime/channels/{id}/messages")))
                .bearer_auth(token)
                .header("idempotency-key", "run-1:call-1-0")
                .json(&json!({"body": {"type": "text", "text": words}}))
                .send()
        };
        let one = post(&tokens[0], "from the first").await?;
        assert_eq!(one.status().as_u16(), 201);
        let one: Value = one.json().await?;
        let two = post(&tokens[1], "from the second").await?;
        let status = two.status().as_u16();
        let two: Value = two.json().await?;
        assert_eq!(
            status, 201,
            "the second conversation's post is its own: {two}"
        );
        assert_ne!(one["seq"], two["seq"]);
        // The same conversation retrying the same post gets its first answer back.
        let again: Value = post(&tokens[0], "from the first").await?.json().await?;
        assert_eq!(again, one);
        let (_, page) = runtime_call(
            f,
            Method::GET,
            &format!("/runtime/channels/{id}/messages"),
            &tokens[0],
            None,
        )
        .await?;
        assert_eq!(page["messages"].as_array().map(Vec::len), Some(2));
        Ok(())
    })
    .await
}
