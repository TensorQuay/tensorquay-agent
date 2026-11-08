//! GW-I-101 (ADR-0014 point 11): `GET /api/v1/channels/{id}/events?stream=1` replays a channel's messages in seq
//! order and then goes live, whoever posts (a person or a run); `Last-Event-ID` replays exactly what was missed; ending
//! the channel sends `channel.state`; a stranger gets `404`.
#[path = "support/fixture.rs"]
mod support;
use futures::StreamExt as _;
use reqwest::Method;
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::time::Duration;
use support::{Fixture, TestResult, call};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// An open event stream and the text read from it so far.
struct Events {
    body: futures::stream::BoxStream<'static, reqwest::Result<bytes::Bytes>>,
    buffer: String,
}

impl Events {
    async fn open(
        f: &Fixture,
        cookie: &str,
        id: &str,
        last: Option<u64>,
    ) -> Result<(u16, Option<Self>)> {
        let mut request = f
            .client
            .get(f.url(&format!("/api/v1/channels/{id}/events?stream=1")))
            .header("origin", f.origin())
            .header("cookie", cookie);
        if let Some(last) = last {
            request = request.header("last-event-id", last.to_string());
        }
        let response = request.send().await?;
        let status = response.status().as_u16();
        if status != 200 {
            return Ok((status, None));
        }
        Ok((
            status,
            Some(Self {
                body: response.bytes_stream().boxed(),
                buffer: String::new(),
            }),
        ))
    }

    /// The next event's `id` (when it has one) and data, skipping keep-alive comments; within 10 s.
    async fn next(&mut self) -> Result<(Option<u64>, Value)> {
        tokio::time::timeout(Duration::from_secs(10), self.read()).await?
    }

    async fn read(&mut self) -> Result<(Option<u64>, Value)> {
        loop {
            if let Some(event) = self.take()? {
                return Ok(event);
            }
            let chunk = self.body.next().await.ok_or("stream ended")??;
            self.buffer.push_str(&String::from_utf8_lossy(&chunk));
        }
    }
}

impl Events {
    /// The first complete event in the buffer, skipping comments; `None` until one is complete.
    fn take(&mut self) -> Result<Option<(Option<u64>, Value)>> {
        while let Some(end) = self.buffer.find("\n\n") {
            let record: String = self.buffer.drain(..end + 2).collect();
            if let Some(event) = parse(&record)? {
                return Ok(Some(event));
            }
        }
        Ok(None)
    }
}

/// One record's `id` and data; `None` for a comment.
fn parse(record: &str) -> Result<Option<(Option<u64>, Value)>> {
    let mut id = None;
    let mut data = String::new();
    for line in record.lines() {
        if let Some(value) = line.strip_prefix("id:") {
            id = Some(value.trim().parse()?);
        } else if let Some(value) = line.strip_prefix("data:") {
            data.push_str(value.trim_start());
        }
    }
    if data.is_empty() {
        return Ok(None);
    }
    Ok(Some((id, serde_json::from_str(&data)?)))
}

async fn post(f: &Fixture, cookie: &str, id: &str, words: &str) -> Result<u16> {
    let key = uuid::Uuid::now_v7().to_string();
    let (status, _) = call(
        f,
        Method::POST,
        &format!("/api/v1/channels/{id}/messages"),
        cookie,
        (
            Some(json!({"body": {"type": "text", "text": words}})),
            Some(&key),
        ),
    )
    .await?;
    Ok(status)
}

// TEST: GW-I-101
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_101_the_stream_replays_then_goes_live_in_seq_order(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let (_, stranger) = f.member("dave@example.invalid").await?;
        let agent = f.conversation(owner.id).await?;
        let _runtime = f.runtime(&agent).await?;
        let token = f
            .backend
            .token(&agent.id)
            .ok_or("token")?
            .expose_secret()
            .to_owned();
        let (_, created) = call(
            f,
            Method::POST,
            "/api/v1/channels",
            &cookie,
            (Some(json!({"name": "live", "rule": "majority"})), None),
        )
        .await?;
        let id = created["id"].as_str().ok_or("id")?.to_owned();
        call(
            f,
            Method::PUT,
            &format!("/api/v1/channels/{id}/members/conversation/{}", agent.id),
            &cookie,
            (Some(json!({"role": "member"})), None),
        )
        .await?;
        post(f, &cookie, &id, "one").await?;
        post(f, &cookie, &id, "two").await?;
        let (_, events) = Events::open(f, &cookie, &id, None).await?;
        let mut events = events.ok_or("stream")?;
        for seq in [1, 2] {
            let (event_id, data) = events.next().await?;
            assert_eq!(
                (event_id, &data["type"], &data["seq"]),
                (Some(seq), &json!("channel.message"), &json!(seq)),
                "{data}"
            );
        }
        // Live: a person's post, then a run's.
        assert_eq!(post(f, &cookie, &id, "three").await?, 201);
        let response = f
            .client
            .post(f.url(&format!("/runtime/channels/{id}/messages")))
            .bearer_auth(&token)
            .json(&json!({"body": {"type": "text", "text": "four"}}))
            .send()
            .await?;
        assert_eq!(response.status().as_u16(), 201);
        let (event_id, three) = events.next().await?;
        assert_eq!(
            (event_id, &three["body"]["text"]),
            (Some(3), &json!("three"))
        );
        let (event_id, four) = events.next().await?;
        assert_eq!(
            (event_id, &four["author"]["kind"]),
            (Some(4), &json!("conversation"))
        );
        // A reconnect from 2 replays exactly 3 and 4.
        let (_, again) = Events::open(f, &cookie, &id, Some(2)).await?;
        let mut again = again.ok_or("stream")?;
        assert_eq!(again.next().await?.0, Some(3));
        assert_eq!(again.next().await?.0, Some(4));
        // Ending the channel: an unsequenced state frame.
        let (status, _) = call(
            f,
            Method::POST,
            &format!("/api/v1/channels/{id}/end"),
            &cookie,
            (None, None),
        )
        .await?;
        assert_eq!(status, 200);
        let (event_id, ended) = events.next().await?;
        assert_eq!(
            (event_id, ended),
            (None, json!({"type": "channel.state", "state": "ended"}))
        );
        // A stranger can't tell the channel exists.
        let (status, _) = Events::open(f, &stranger, &id, None).await?;
        assert_eq!(status, 404);
        Ok(())
    })
    .await
}
