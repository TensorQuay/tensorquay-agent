//! A channel's agent member for the channel tests (ADR-0014's amendment of 20 Nov 2026): its task, its fake runtime
//! and runs, its reads and posts, and a person's channel, posts and notices.
#![allow(
    dead_code,
    reason = "each channel test file uses its own share of these"
)]
use crate::support::{self, Fixture, TestResult, call};
use reqwest::Method;
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use std::time::Duration;
use tqa_types::{ChannelWake, ClientFrame, ConversationId, ErrorCode, Event, ServerFrame};
use uuid::Uuid;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// How long a wake may take to come (after a commit, or the sweep's next pass).
pub const WAKE: Duration = Duration::from_secs(15);
/// How long a test waits to be sure no wake comes.
pub const QUIET: Duration = Duration::from_secs(3);

/// An agent member: its task, its fake runtime once its sandbox exists, and its next event seq.
pub struct Agent {
    pub id: ConversationId,
    pub uuid: Uuid,
    pub runtime: Option<support::runtime::Runtime>,
    pub seq: u64,
    pub runs: u32,
}

impl Agent {
    /// A task of `owner` named `title` (the name a post uses after `@`).
    pub async fn new(f: &Fixture, owner: Uuid, title: &str) -> Result<Self> {
        let created = f.conversation(owner).await?;
        let uuid = Uuid::parse_str(created.id.as_str())?;
        sqlx::query("UPDATE conversations SET title = $2 WHERE id = $1")
            .bind(uuid)
            .bind(title)
            .execute(&f.pool)
            .await?;
        Ok(Self {
            id: created.id,
            uuid,
            runtime: None,
            seq: 0,
            runs: 0,
        })
    }

    /// The next turn the task gets within `wait`, connecting its runtime (at `revision`) once its sandbox exists.
    pub async fn next_at(
        &mut self,
        f: &Fixture,
        wait: Duration,
        revision: u16,
    ) -> Result<Option<ClientFrame>> {
        if self.runtime.is_none() {
            if !created(f, &self.id, wait).await {
                return Ok(None);
            }
            self.runtime = Some(f.runtime_at(&self.id, revision).await?);
        }
        self.runtime
            .as_mut()
            .ok_or("runtime")?
            .try_action(wait)
            .await
    }

    pub async fn next(&mut self, f: &Fixture, wait: Duration) -> Result<Option<ClientFrame>> {
        self.next_at(f, wait, tqa_types::PROTOCOL_REVISION).await
    }

    /// The next turn if it is a channel's wake: its text and marker; `None` when no turn comes.
    pub async fn wake(
        &mut self,
        f: &Fixture,
        wait: Duration,
    ) -> Result<Option<(String, ChannelWake)>> {
        match self.next(f, wait).await? {
            None => Ok(None),
            Some(ClientFrame::UserMessage {
                text,
                channel_wake: Some(wake),
                team_wake: None,
                lead_brief: None,
                ..
            }) => Ok(Some((text, *wake))),
            Some(other) => Err(format!("not a channel's wake: {other:?}").into()),
        }
    }

    /// Asserts a wake comes for `channel`, at the member's place `after`; answers its text.
    pub async fn woken(&mut self, f: &Fixture, channel: &str, after: u64) -> Result<String> {
        let (text, wake) = self.wake(f, WAKE).await?.ok_or("no wake came")?;
        assert_eq!(
            (wake.channel.as_str(), wake.after),
            (channel, after),
            "{text}"
        );
        Ok(text)
    }

    /// Asserts no turn comes for a while.
    pub async fn quiet(&mut self, f: &Fixture) -> TestResult {
        let next = self.next(f, QUIET).await?;
        assert!(next.is_none(), "no turn: {next:?}");
        Ok(())
    }

    pub async fn event(&mut self, f: &Fixture, event: Event) -> TestResult {
        self.seq += 1;
        let run = format!("r-{}", self.runs);
        let frame = ServerFrame::Event {
            seq: self.seq.into(),
            run_id: Some(run.parse()?),
            step: None,
            ts: "2026-01-01T00:00:00Z".into(),
            event,
        };
        f.gateway.events().append(&self.id, frame).await?;
        Ok(())
    }

    /// The run its last turn started.
    pub async fn start(&mut self, f: &Fixture) -> TestResult {
        self.runs += 1;
        let run_id = format!("r-{}", self.runs).parse()?;
        self.event(f, Event::RunStarted { run_id }).await
    }

    pub async fn end(&mut self, f: &Fixture) -> TestResult {
        let run_id = format!("r-{}", self.runs).parse()?;
        self.event(f, Event::RunCompleted { run_id }).await
    }

    pub async fn fail(&mut self, f: &Fixture) -> TestResult {
        let run_id = format!("r-{}", self.runs).parse()?;
        let failed = Event::RunFailed {
            run_id,
            reason: "the model's reply was cut off".into(),
            code: ErrorCode::ReplyCut,
            provider_message: None,
        };
        self.event(f, failed).await
    }

    pub fn token(&self, f: &Fixture) -> Result<String> {
        Ok(f.backend
            .token(&self.id)
            .ok_or("no token")?
            .expose_secret()
            .to_owned())
    }

    /// Reads the channel from its run, as `channel_read` does: the seqs answered.
    pub async fn read(&self, f: &Fixture, channel: &str, since: u64) -> Result<Vec<u64>> {
        let (status, page) = runtime_call(
            f,
            Method::GET,
            &format!("/runtime/channels/{channel}/messages?since={since}"),
            &self.token(f)?,
            None,
        )
        .await?;
        assert_eq!(status, 200, "{page}");
        Ok(page["messages"]
            .as_array()
            .ok_or("messages")?
            .iter()
            .filter_map(|m| m["seq"].as_u64())
            .collect())
    }

    /// Posts from its run; answers the seq.
    pub async fn post(&self, f: &Fixture, channel: &str, body: Value) -> Result<u64> {
        let (status, posted) = runtime_call(
            f,
            Method::POST,
            &format!("/runtime/channels/{channel}/messages"),
            &self.token(f)?,
            Some(json!({"body": body})),
        )
        .await?;
        assert_eq!(status, 201, "{posted}");
        posted["seq"].as_u64().ok_or_else(|| "seq".into())
    }

    /// The member's place, as stored (A2).
    pub async fn place(&self, f: &Fixture, channel: &str) -> Result<i64> {
        Ok(sqlx::query_scalar(
            "SELECT read_seq FROM channel_members WHERE channel_id = $1 AND conversation_id = $2",
        )
        .bind(Uuid::parse_str(channel)?)
        .bind(self.uuid)
        .fetch_one(&f.pool)
        .await?)
    }
}

/// Whether the task's sandbox exists within `wait`.
pub async fn created(f: &Fixture, id: &ConversationId, wait: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + wait;
    while f.backend.created(id).is_none() {
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    true
}

pub async fn runtime_call(
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

/// A channel owned by the session's person, with these agent members at these roles; answers its id.
pub async fn channel(
    f: &Fixture,
    cookie: &str,
    name: &str,
    members: &[(&Agent, &str)],
) -> Result<String> {
    let (status, created) = call(
        f,
        Method::POST,
        "/api/v1/channels",
        cookie,
        (
            Some(json!({"name": name, "purpose": "", "rule": "majority"})),
            None,
        ),
    )
    .await?;
    assert_eq!(status, 201, "{created}");
    let id = created["id"].as_str().ok_or("id")?.to_owned();
    for (agent, role) in members {
        let (status, body) = call(
            f,
            Method::PUT,
            &format!("/api/v1/channels/{id}/members/conversation/{}", agent.id),
            cookie,
            (Some(json!({"role": role})), None),
        )
        .await?;
        assert_eq!(status, 200, "{body}");
    }
    Ok(id)
}

/// A person's post; answers its seq.
pub async fn post(
    f: &Fixture,
    cookie: &str,
    channel: &str,
    body: Value,
    parent: Option<u64>,
) -> Result<u64> {
    let mut request = json!({"body": body});
    if let Some(parent) = parent {
        request["parent"] = json!(parent);
    }
    let key = Uuid::now_v7().to_string();
    let (status, posted) = call(
        f,
        Method::POST,
        &format!("/api/v1/channels/{channel}/messages"),
        cookie,
        (Some(request), Some(key.as_str())),
    )
    .await?;
    assert_eq!(status, 201, "{posted}");
    posted["seq"].as_u64().ok_or_else(|| "seq".into())
}

pub fn text(words: &str) -> Value {
    json!({"type": "text", "text": words})
}

/// The channel's notices of one kind.
pub async fn notices(f: &Fixture, channel: &str, kind: &str) -> Result<i64> {
    Ok(sqlx::query_scalar("SELECT count(*) FROM channel_messages WHERE channel_id = $1 AND type = 'notice' AND body->>'kind' = $2")
        .bind(Uuid::parse_str(channel)?)
        .bind(kind)
        .fetch_one(&f.pool)
        .await?)
}
