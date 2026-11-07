//! GW-I-138: after a task's first run completes, the gateway names it in 3–6 words with one small call on the task's
//! own route, recorded as `title` usage and announced with `task.titled`; `title_source` keeps a person's rename from
//! ever being overwritten; nothing is asked twice; the file's `[tasks] auto_title = false` turns it off.
#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::time::Duration;
use support::{Fixture, TestResult, call};
use tqa_testkit::{FakeModelServer, Script, Scripted};
use tqa_types::{Event, ServerFrame};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const ASKED: &str = "Clean up the attached sales CSV: drop duplicate rows and fix the date column";

fn registry(server: &FakeModelServer) -> Result<tqa_llm::Registry> {
    registry_with(server, "")
}

fn registry_with(server: &FakeModelServer, extra: &str) -> Result<tqa_llm::Registry> {
    Ok(tqa_llm::Registry::from_toml(&format!(
        "[[routes]]\nname='local'\nprotocol='openai_chat'\nbase_url='{}'\nmodel='m'\nauth={{kind='none'}}\nstream_usage=true\n{extra}",
        server.base_url()
    ))?)
}

fn answers(texts: &[&str]) -> Vec<Script> {
    texts
        .iter()
        .map(|text| {
            Script::new(vec![
                Scripted::Text((*text).into()),
                Scripted::Usage(tqa_llm::Usage {
                    prompt_tokens: 40,
                    completion_tokens: 6,
                    cached_tokens: None,
                    reasoning_tokens: None,
                }),
            ])
        })
        .collect()
}

fn lifecycle(seq: u64, event: Event) -> ServerFrame {
    ServerFrame::Event {
        seq: seq.into(),
        run_id: None,
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event,
    }
}

async fn task(f: &Fixture, cookie: &str, key: &str) -> Result<tqa_types::ConversationId> {
    let (status, created) = call(
        f,
        Method::POST,
        "/api/v1/conversations",
        cookie,
        (
            Some(json!({"title": ASKED, "model_route": "local"})),
            Some(key),
        ),
    )
    .await?;
    assert_eq!(
        (status, &created["title_source"]),
        (201, &json!("message")),
        "{created}"
    );
    Ok(created["id"].as_str().ok_or("id")?.parse()?)
}

/// Runs `runs` complete runs on the task's runtime.
async fn complete(f: &Fixture, id: &tqa_types::ConversationId, runs: u64) -> TestResult {
    let mut runtime = f.runtime_for(id).await?;
    for run in 0..runs {
        let run_id: tqa_types::RunId = format!("run-{run}").parse()?;
        runtime
            .send(&lifecycle(
                2 * run + 1,
                Event::RunStarted {
                    run_id: run_id.clone(),
                },
            ))
            .await?;
        runtime
            .send(&lifecycle(2 * run + 2, Event::RunCompleted { run_id }))
            .await?;
    }
    f.until_sequence(id, 2 * runs).await?;
    Ok(())
}

async fn read(f: &Fixture, cookie: &str, id: &tqa_types::ConversationId) -> Result<Value> {
    let (_, body) = call(
        f,
        Method::GET,
        &format!("/api/v1/conversations/{id}"),
        cookie,
        (None, None),
    )
    .await?;
    Ok(body)
}

/// The task's title once its source is no longer `message`, or its read after `wait`.
async fn settled(
    f: &Fixture,
    cookie: &str,
    id: &tqa_types::ConversationId,
    wait: Duration,
) -> Result<Value> {
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        let body = read(f, cookie, id).await?;
        if body["title_source"] != "message" || tokio::time::Instant::now() >= deadline {
            return Ok(body);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

// TEST: GW-I-138
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_138_the_first_completed_run_names_the_task_once(pool: PgPool) -> TestResult {
    let server = FakeModelServer::start(
        answers(&["\"Sales CSV cleanup.\""]),
        std::sync::Arc::new(tqa_llm::TokioSleeper),
    )
    .await?;
    support::run_configured(pool, Some(registry(&server)?), "", async |f| {
        let (_, cookie) = f.admin().await?;
        let id = task(f, &cookie, "titled").await?;
        let mut stream = f.stream(&id, &cookie, 0).await?;
        complete(f, &id, 2).await?;
        let body = settled(f, &cookie, &id, support::DEADLINE).await?;
        assert_eq!(
            (&body["title"], &body["title_source"]),
            (&json!("Sales CSV cleanup"), &json!("model"))
        );
        loop {
            if let (None, ServerFrame::TaskTitled { title }) = stream.next().await? {
                assert_eq!(title, "Sales CSV cleanup");
                break;
            }
        }
        // One call, however many runs complete; its request is tiny and carries the task's own words.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        let sent = requests[0].body.to_string();
        assert!(sent.contains("sales CSV") && sent.len() < 4_000, "{sent}");
        let usage: Vec<(String, Option<i64>, Option<i64>)> = sqlx::query_as(
            "SELECT kind, prompt_tokens, completion_tokens FROM usage WHERE conversation_id=$1",
        )
        .bind(uuid::Uuid::parse_str(id.as_str())?)
        .fetch_all(&f.pool)
        .await?;
        assert_eq!(usage, [("title".to_owned(), Some(40), Some(6))]);
        Ok(())
    })
    .await
}

// TEST: GW-I-138
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_138_a_persons_title_is_never_overwritten(pool: PgPool) -> TestResult {
    let server = FakeModelServer::start(
        answers(&["Sales CSV cleanup"]),
        std::sync::Arc::new(tqa_llm::TokioSleeper),
    )
    .await?;
    support::run_configured(pool, Some(registry(&server)?), "", async |f| {
        let (_, cookie) = f.admin().await?;
        let id = task(f, &cookie, "renamed").await?;
        let (status, renamed) = call(
            f,
            Method::PATCH,
            &format!("/api/v1/conversations/{id}"),
            &cookie,
            (Some(json!({"title": "My sales work"})), Some("rename")),
        )
        .await?;
        assert_eq!(
            (status, &renamed["title_source"]),
            (200, &json!("person")),
            "{renamed}"
        );
        complete(f, &id, 1).await?;
        tokio::time::sleep(Duration::from_millis(500)).await;
        let body = read(f, &cookie, &id).await?;
        assert_eq!(
            (&body["title"], &body["title_source"]),
            (&json!("My sales work"), &json!("person"))
        );
        assert!(
            server.requests().is_empty(),
            "a person's title is never asked for"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-138
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_138_a_useless_answer_keeps_the_message(pool: PgPool) -> TestResult {
    let server =
        FakeModelServer::start(answers(&[""]), std::sync::Arc::new(tqa_llm::TokioSleeper)).await?;
    support::run_configured(pool, Some(registry(&server)?), "", async |f| {
        let (_, cookie) = f.admin().await?;
        let id = task(f, &cookie, "empty").await?;
        complete(f, &id, 1).await?;
        tokio::time::sleep(Duration::from_millis(500)).await;
        let body = read(f, &cookie, &id).await?;
        assert_eq!(
            (&body["title"], &body["title_source"]),
            (&json!(ASKED), &json!("message")),
            "an empty answer changes nothing"
        );
        assert_eq!(server.requests().len(), 1, "asked once, not again");
        Ok(())
    })
    .await
}

// TEST: GW-I-138
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_138_a_title_call_the_server_refuses_keeps_the_message(pool: PgPool) -> TestResult {
    let refused = Script::new(vec![Scripted::Status {
        code: 500,
        body: "upstream failed".into(),
        retry_after: None,
    }]);
    let server =
        FakeModelServer::start(vec![refused], std::sync::Arc::new(tqa_llm::TokioSleeper)).await?;
    support::run_configured(pool, Some(registry(&server)?), "", async |f| {
        let (_, cookie) = f.admin().await?;
        let id = task(f, &cookie, "refused").await?;
        complete(f, &id, 2).await?;
        tokio::time::sleep(Duration::from_millis(500)).await;
        let body = read(f, &cookie, &id).await?;
        assert_eq!(
            (&body["title"], &body["title_source"]),
            (&json!(ASKED), &json!("message")),
            "a failed title call leaves the task's first words"
        );
        assert_eq!(
            server.requests().len(),
            1,
            "asked once, never retried, and not again after the second run"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-138
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_138_the_file_turns_it_off(pool: PgPool) -> TestResult {
    let server = FakeModelServer::start(
        answers(&["Sales CSV cleanup"]),
        std::sync::Arc::new(tqa_llm::TokioSleeper),
    )
    .await?;
    support::run_configured(
        pool,
        Some(registry(&server)?),
        "[tasks]\nauto_title=false\n",
        async |f| {
            let (_, cookie) = f.admin().await?;
            let id = task(f, &cookie, "off").await?;
            complete(f, &id, 1).await?;
            tokio::time::sleep(Duration::from_millis(500)).await;
            assert_eq!(read(f, &cookie, &id).await?["title_source"], "message");
            assert!(server.requests().is_empty(), "turned off in the file");
            Ok(())
        },
    )
    .await
}

// TEST: GW-I-138
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_138_a_persons_message_cancels_the_title_on_a_one_slot_route(
    pool: PgPool,
) -> TestResult {
    // The model takes 3 s to answer the title call, which holds the route's only slot meanwhile.
    let mut slow = answers(&["Sales CSV cleanup"]);
    slow[0].header_delay = Duration::from_secs(3);
    let server = FakeModelServer::start(slow, std::sync::Arc::new(tqa_llm::TokioSleeper)).await?;
    let models = registry_with(&server, "concurrency=1\n")?;
    support::run_configured(pool, Some(models), "", async |f| {
        let (_, cookie) = f.admin().await?;
        let id = task(f, &cookie, "busy").await?;
        complete(f, &id, 1).await?;
        let deadline = tokio::time::Instant::now() + support::DEADLINE;
        while server.requests().is_empty() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the title call was never made"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        // The person writes again: the title call gives up the slot at once, and the task keeps its title.
        let (status, sent) = call(
            f,
            Method::POST,
            &format!("/api/v1/conversations/{id}/messages"),
            &cookie,
            (
                Some(json!({"text": "Also sort by date"})),
                Some("follow-up"),
            ),
        )
        .await?;
        assert_eq!(status, 202, "{sent}");
        tokio::time::sleep(Duration::from_secs(4)).await;
        let body = read(f, &cookie, &id).await?;
        assert_eq!(
            (&body["title"], &body["title_source"]),
            (&json!(ASKED), &json!("message")),
            "the late answer is never used"
        );
        let usage: i64 = sqlx::query_scalar("SELECT count(*) FROM usage WHERE conversation_id=$1")
            .bind(uuid::Uuid::parse_str(id.as_str())?)
            .fetch_one(&f.pool)
            .await?;
        assert_eq!(usage, 0);
        Ok(())
    })
    .await
}

// TEST: GW-I-138
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_138_a_busy_route_is_not_asked_and_the_task_keeps_its_title(
    pool: PgPool,
) -> TestResult {
    // One task's title call holds the route's only slot for 3 s.
    let mut slow = answers(&["Sales CSV cleanup"]);
    slow[0].header_delay = Duration::from_secs(3);
    let server = FakeModelServer::start(slow, std::sync::Arc::new(tqa_llm::TokioSleeper)).await?;
    let models = registry_with(&server, "concurrency=1\n")?;
    support::run_configured(pool, Some(models), "", async |f| {
        let (_, cookie) = f.admin().await?;
        let first = task(f, &cookie, "first").await?;
        let second = task(f, &cookie, "second").await?;
        complete(f, &first, 1).await?;
        let deadline = tokio::time::Instant::now() + support::DEADLINE;
        while server.requests().is_empty() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the title call was never made"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        // The other task's first run completes while the slot is taken: its title is never asked for, not even later.
        complete(f, &second, 1).await?;
        let named = settled(f, &cookie, &first, support::DEADLINE).await?;
        assert_eq!(named["title_source"], "model");
        assert_eq!(server.requests().len(), 1, "no queueing behind the slot");
        let kept = read(f, &cookie, &second).await?;
        assert_eq!(
            (&kept["title"], &kept["title_source"]),
            (&json!(ASKED), &json!("message"))
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-138
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_138_another_tasks_request_cancels_the_title_holding_the_only_slot(
    pool: PgPool,
) -> TestResult {
    use secrecy::ExposeSecret;
    // The title call would hold the route's only slot for 30 s; the other task's request is answered at once. A bound of
    // a few seconds tells the two apart on any host (3 s against 1.5 s was close to a loaded one's).
    let mut scripts = answers(&["Sales CSV cleanup", "Hello"]);
    scripts[0].header_delay = Duration::from_secs(30);
    let server =
        FakeModelServer::start(scripts, std::sync::Arc::new(tqa_llm::TokioSleeper)).await?;
    let models = registry_with(&server, "concurrency=1\n")?;
    support::run_configured(pool, Some(models), "", async |f| {
        let (owner, cookie) = f.admin().await?;
        let titled = task(f, &cookie, "titled").await?;
        complete(f, &titled, 1).await?;
        let deadline = tokio::time::Instant::now() + support::DEADLINE;
        while server.requests().is_empty() {
            assert!(tokio::time::Instant::now() < deadline, "the title call was never made");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        // Another person's task asks the model while the title call holds the slot.
        let other = f.conversation(owner.id).await?;
        let token = f
            .gateway
            .runtime_tokens()
            .mint(&other.id, &"other-sandbox".parse()?, std::net::Ipv4Addr::LOCALHOST.into())
            .await?;
        let started = tokio::time::Instant::now();
        let response = f
            .client
            .post(f.url("/relay/r/local/chat/completions"))
            .header("x-tqa-run-id", "r-1")
            .bearer_auth(token.expose_secret())
            .json(&json!({"model": "m", "messages": [{"role": "user", "content": "hello"}], "stream": true}))
            .send()
            .await?;
        assert_eq!(response.status(), 200);
        let _ = response.text().await?;
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the task waited {:?} behind a title",
            started.elapsed()
        );
        tokio::time::sleep(Duration::from_secs(3)).await;
        let body = read(f, &cookie, &titled).await?;
        assert_eq!(
            (&body["title"], &body["title_source"]),
            (&json!(ASKED), &json!("message")),
            "the title call gave way"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-155
/// F23 (candidate 12, qwen2.5): an English task named in Chinese. The call names the task's writing system, and a title
/// in another one is refused, so the task keeps its first words.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_155_a_title_in_another_writing_system_keeps_the_first_words(
    pool: PgPool,
) -> TestResult {
    let server = FakeModelServer::start(
        answers(&["创建2025年区域销售CSV"]),
        std::sync::Arc::new(tqa_llm::TokioSleeper),
    )
    .await?;
    support::run_configured(pool, Some(registry(&server)?), "", async |f| {
        let (_, cookie) = f.admin().await?;
        let id = task(f, &cookie, "script").await?;
        complete(f, &id, 1).await?;
        tokio::time::sleep(Duration::from_millis(500)).await;
        let body = read(f, &cookie, &id).await?;
        assert_eq!(
            (&body["title"], &body["title_source"]),
            (&json!(ASKED), &json!("message")),
            "a Chinese title for an English task changes nothing"
        );
        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        let system = requests[0].body["messages"][0]["content"].to_string();
        assert!(system.contains("written in the Latin alphabet"), "{system}");
        Ok(())
    })
    .await
}
