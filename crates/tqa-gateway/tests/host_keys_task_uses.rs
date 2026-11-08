//! GW-I-192 (ADR-0037 §2): a task's key uses, for a page opened after the live frames: by run, newest first, paged; the
//! key's name only for the row's sender; the host and port canonical; never a value or a placeholder; readable by who
//! may read the task's events, answered as they are otherwise (403, or 404 for no task). A key's name goes only to
//! someone who may use the key, judged when the rows are read: never to a teammate whose task used another person's
//! placeholder, nor to someone since removed from a shared key (ADR-0037 Amendment 1).
#[path = "support/keyed.rs"]
mod keyed;
#[path = "support/fixture.rs"]
mod support;

use keyed::*;
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{TestResult, call};

// TEST: GW-I-192
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_192_a_tasks_key_uses_are_read_by_run_with_the_name_for_its_sender(
    pool: PgPool,
) -> TestResult {
    let hosts = hosts().await?;
    support::run_configured(pool, None, &config(5), async |f| {
        let run = keyed_run(f, &hosts).await?;
        let bearer = format!("Bearer {}", run.placeholder);
        let mut client = keyed_client(run.proxy, HOST, run.install.clone()).await?;
        for _ in 0..2 {
            assert_eq!(
                send(&mut client, get(HOST, "/v1/me", Some(&bearer))?)
                    .await?
                    .0,
                200
            );
        }
        let misplaced = format!("/v1/me?token={}", run.placeholder);
        assert_eq!(
            send(&mut client, get(HOST, &misplaced, None)?).await?.0,
            403
        );
        let path = format!("/api/v1/conversations/{}/key-uses", run.conversation);
        // A refusal's row is queued, not awaited: the page is read until its batch is in.
        let page = page_until(f, &path, &run.owner.1, |page| {
            page["items"]
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item["outcome"] == "wrong_position"))
        })
        .await?;
        let items = page["items"].as_array().ok_or("items")?;
        let row = |outcome: &str| {
            items
                .iter()
                .find(|item| item["outcome"] == outcome)
                .cloned()
        };
        let ok = row("ok").ok_or("an ok row")?;
        assert_eq!(
            (
                &ok["run_id"],
                &ok["sender_id"],
                &ok["name"],
                &ok["host"],
                &ok["port"],
                &ok["count"]
            ),
            (
                &serde_json::json!("run-1"),
                &serde_json::json!(run.owner.0),
                &serde_json::json!("API_KEY"),
                &serde_json::json!(HOST),
                &serde_json::json!(443),
                &serde_json::json!(2)
            ),
            "{page}"
        );
        assert_eq!(
            row("wrong_position").ok_or("a refusal row")?["count"],
            1,
            "{page}"
        );
        assert!(page["next"].is_null(), "{page}");
        let text = page.to_string();
        assert!(!text.contains(VALUE) && !text.contains("tqa_ph_"), "{text}");
        // One row a page, then the next.
        let (_, first) = call(
            f,
            Method::GET,
            &format!("{path}?limit=1"),
            &run.owner.1,
            (None, None),
        )
        .await?;
        let before = first["next"].as_str().ok_or("a next page")?;
        let (_, second) = call(
            f,
            Method::GET,
            &format!("{path}?limit=1&before={before}"),
            &run.owner.1,
            (None, None),
        )
        .await?;
        assert_ne!(first["items"][0]["outcome"], second["items"][0]["outcome"]);
        // As the events answer: forbidden to anyone who may not read the task's events, not found for no task.
        let (_, member) = f.member("teammate@example.invalid").await?;
        let (status, _) = call(f, Method::GET, &path, &member, (None, None)).await?;
        assert_eq!(status, 403);
        let (status, _) = call(
            f,
            Method::GET,
            &format!("/api/v1/conversations/{}/events", run.conversation),
            &member,
            (None, None),
        )
        .await?;
        assert_eq!(status, 403, "the same answer as the task's events");
        let unknown = format!("/api/v1/conversations/{}/key-uses", uuid::Uuid::now_v7());
        let (status, _) = call(f, Method::GET, &unknown, &run.owner.1, (None, None)).await?;
        assert_eq!(status, 404);
        let (status, _) = call(
            f,
            Method::GET,
            &format!("{path}?limit=0"),
            &run.owner.1,
            (None, None),
        )
        .await?;
        assert_eq!(status, 400);
        Ok(())
    })
    .await
}

/// A teammate's task, its run going with `key` of theirs for the keyed host, and a client on its tunnel.
async fn teammates_run(
    f: &support::Fixture,
    run: &Run,
    cookie: &str,
    teammate: uuid::Uuid,
    key: &str,
) -> Result<
    (
        Run,
        hyper::client::conn::http1::SendRequest<http_body_util::Full<bytes::Bytes>>,
    ),
    Error,
> {
    let theirs = task_run(f, (teammate, cookie), run.proxy, key).await?;
    let client = client_as(
        theirs.proxy,
        (&format!("{HOST}:443"), HOST),
        theirs.install.clone(),
        Some(&theirs.token),
    )
    .await?;
    Ok((theirs, client))
}

/// The one row of `conversation`'s key uses as `cookie` reads it, once it is in: a refused use's row is queued.
async fn only_row(f: &support::Fixture, conversation: &str, cookie: &str) -> Result<Value, Error> {
    let path = format!("/api/v1/conversations/{conversation}/key-uses");
    let page = page_until(f, &path, cookie, |page| {
        page["items"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    })
    .await?;
    let items = page["items"].as_array().ok_or("items")?;
    assert_eq!(items.len(), 1, "{page}");
    Ok(items[0].clone())
}

// TEST: GW-I-192
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_192_a_teammates_use_of_another_persons_placeholder_never_names_the_key(
    pool: PgPool,
) -> TestResult {
    let hosts = hosts().await?;
    support::run_configured(pool, None, &config(5), async |f| {
        let run = keyed_run(f, &hosts).await?;
        let (teammate, cookie) = f.member("teammate@example.invalid").await?;
        // The teammate's own key for the host, so their calls to it go through the keyed proxy (a first-slice limit).
        let own = json!({"name": "TEAMMATE_KEY", "address": "https://api.keyed.test", "scheme": "bearer", "value": VALUE});
        let (status, added) = call(f, Method::POST, "/api/v1/host-keys", &cookie, (Some(own), Some("teammate"))).await?;
        assert_eq!(status, 201, "{added}");
        let (theirs, mut client) = teammates_run(f, &run, &cookie, teammate.id, "TEAMMATE_KEY").await?;
        let bearer = format!("Bearer {}", run.placeholder);
        assert_eq!(send(&mut client, get(HOST, "/v1/me", Some(&bearer))?).await?.0, 403);
        let row = only_row(f, &theirs.conversation, &cookie).await?;
        assert_eq!(
            (&row["outcome"], &row["sender_id"], &row["name"]),
            (&json!("foreign_sandbox"), &json!(teammate.id), &Value::Null),
            "the row is the teammate's, the key's name never: {row}"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-192
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_192_someone_removed_from_a_shared_key_no_longer_sees_its_name(
    pool: PgPool,
) -> TestResult {
    let hosts = hosts().await?;
    support::run_configured(pool, None, &config(5), async |f| {
        let run = keyed_run(f, &hosts).await?;
        let admin = run.owner.1.clone();
        let (teammate, cookie) = f.member("teammate@example.invalid").await?;
        let shared = |users: Value| {
            json!({"name": "SHARED_KEY", "address": "https://api.keyed.test", "scheme": "bearer", "value": VALUE,
                   "users": users})
        };
        let (status, key) = call(f, Method::POST, "/api/v1/admin/host-keys", &admin, (Some(shared(json!([teammate.id]))), Some("shared"))).await?;
        assert_eq!(status, 201, "{key}");
        let (theirs, mut client) = teammates_run(f, &run, &cookie, teammate.id, "SHARED_KEY").await?;
        let bearer = format!("Bearer {}", theirs.placeholder);
        assert_eq!(send(&mut client, get(HOST, "/v1/me", Some(&bearer))?).await?.0, 200);
        let row = only_row(f, &theirs.conversation, &cookie).await?;
        assert_eq!((&row["outcome"], &row["name"]), (&json!("ok"), &json!("SHARED_KEY")), "{row}");
        // Removed from the key: the old row keeps its use, the name goes (judged when read).
        let mut change = shared(json!([run.owner.0]));
        change.as_object_mut().ok_or("a body")?.remove("value");
        change["version"] = key["version"].clone();
        let path = format!("/api/v1/admin/host-keys/{}", key["id"].as_str().ok_or("id")?);
        let (status, changed) = call(f, Method::PUT, &path, &admin, (Some(change), Some("removed"))).await?;
        assert_eq!(status, 200, "{changed}");
        let row = only_row(f, &theirs.conversation, &cookie).await?;
        assert_eq!((&row["outcome"], &row["count"], &row["name"]), (&json!("ok"), &json!(1), &Value::Null), "{row}");
        Ok(())
    })
    .await
}
