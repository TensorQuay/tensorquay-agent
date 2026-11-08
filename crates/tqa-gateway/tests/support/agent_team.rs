//! The agent-team integration tests' shared fixture (ADR-0034): a team of a lead, two members and an outsider, each
//! with its runtime token and current run, and the calls the tests make on the runtime team routes.
#![allow(dead_code)] // Each integration target compiles this shared harness on its own.
use crate::support::{self, Fixture};
use reqwest::Method;
use secrecy::ExposeSecret;
use serde_json::Value;
use std::time::Duration;
use tqa_types::{ConversationId, Event, ServerFrame, ToolStatus};
use uuid::Uuid;

pub(crate) type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(crate) const ENABLED: &str = "[agent_teams]\nenabled = true\n";

/// Each runtime token's current run, as [`team`] starts them: the helpers send it as `X-TQA-Run-ID`, which every team
/// route but the role takes. A token not here sends none.
pub(crate) static RUNS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, String>>,
> = std::sync::LazyLock::new(Default::default);

pub(crate) fn run_of(token: &str) -> Option<String> {
    RUNS.lock().ok()?.get(token).cloned()
}

/// A lead and two members, `researcher` and `reviewer`, each with its runtime token.
pub(crate) struct Team {
    pub(crate) id: Uuid,
    pub(crate) lead: (ConversationId, String),
    pub(crate) researcher: (ConversationId, String),
    pub(crate) reviewer: (ConversationId, String),
    pub(crate) outsider: (ConversationId, String),
    /// The owner's session.
    pub(crate) cookie: String,
    /// The fake runtimes, connected for the test: their tokens are what the routes check.
    pub(crate) runtimes: Vec<support::runtime::Runtime>,
}

pub(crate) async fn team(f: &Fixture) -> Result<Team> {
    let (owner, cookie) = f.admin().await?;
    team_for(f, owner.id, cookie).await
}

/// Another team of the same person, for a test that needs several (the installation's administrator is made once).
pub(crate) async fn team_for(f: &Fixture, owner: Uuid, cookie: String) -> Result<Team> {
    let (mut tokens, mut runtimes) = (Vec::new(), Vec::new());
    for _ in 0..4 {
        let conversation = f.conversation(owner).await?;
        runtimes.push(f.runtime(&conversation).await?);
        let token = f
            .backend
            .token(&conversation.id)
            .ok_or("token")?
            .expose_secret()
            .to_owned();
        tokens.push((conversation.id, token));
    }
    let [lead, researcher, reviewer, outsider]: [(ConversationId, String); 4] =
        tokens.try_into().map_err(|_| "four conversations")?;
    // Each conversation's current run, as its runtime starts it: reads must name it (GW-I-109).
    for ((conversation, _), run) in [
        (&lead, "r-1"),
        (&researcher, "r-2"),
        (&reviewer, "r-3"),
        (&outsider, "r-4"),
    ] {
        f.gateway
            .events()
            .append(conversation, support::started(1, run)?)
            .await?;
    }
    if let Ok(mut runs) = RUNS.lock() {
        for ((_, token), run) in [
            (&lead, "r-1"),
            (&researcher, "r-2"),
            (&reviewer, "r-3"),
            (&outsider, "r-4"),
        ] {
            runs.insert(token.clone(), run.to_owned());
        }
    }
    let (team, channel) = (Uuid::now_v7(), Uuid::now_v7());
    sqlx::query("INSERT INTO channels (id, owner_id, name, rule, kind) VALUES ($1, $2, 'Agent team', 'human', 'agent_team')")
        .bind(channel)
        .bind(owner)
        .execute(&f.pool)
        .await?;
    sqlx::query(
        "INSERT INTO agent_teams (id, channel_id, lead_conversation, owner_id, token_budget, deadline_at) \
         VALUES ($1, $2, $3, $4, 2000000, CURRENT_TIMESTAMP + INTERVAL '2 hours')",
    )
    .bind(team)
    .bind(channel)
    .bind(Uuid::parse_str(lead.0.as_str())?)
    .bind(owner)
    .execute(&f.pool)
    .await?;
    for (member, name) in [(&researcher, "researcher"), (&reviewer, "reviewer")] {
        // Past their first turn, as the gateway leaves a started member once its brief is stored.
        sqlx::query(
            "INSERT INTO agent_team_members (conversation_id, team_id, name, role, workspace, state) \
             VALUES ($1, $2, $3, $3, 'own_copy', 'working')",
        )
        .bind(Uuid::parse_str(member.0.as_str())?)
        .bind(team)
        .bind(name)
        .execute(&f.pool)
        .await?;
    }
    Ok(Team {
        id: team,
        lead,
        researcher,
        reviewer,
        outsider,
        cookie,
        runtimes,
    })
}

pub(crate) async fn send(
    f: &Fixture,
    token: &str,
    key: &str,
    body: &Value,
) -> Result<(u16, Value)> {
    let mut request = f
        .client
        .post(f.url("/runtime/teams/messages"))
        .bearer_auth(token)
        .header("idempotency-key", key);
    if let Some(run) = run_of(token) {
        request = request.header("x-tqa-run-id", run);
    }
    let response = request.json(body).send().await?;
    let status = response.status().as_u16();
    Ok((status, response.json().await?))
}

pub(crate) async fn read(f: &Fixture, token: &str, call: (&str, &str)) -> Result<(u16, Value)> {
    read_waiting(f, token, call, 0).await
}

/// A read that waits up to `wait` seconds.
pub(crate) async fn read_waiting(
    f: &Fixture,
    token: &str,
    (key, run): (&str, &str),
    wait: u32,
) -> Result<(u16, Value)> {
    let path = if wait > 0 {
        format!("/runtime/teams/messages?wait={wait}")
    } else {
        "/runtime/teams/messages".into()
    };
    // The read may hold for its whole wait, longer than the client's own deadline: this request's covers both, scaled.
    let deadline =
        tqa_testkit::deadline::scaled(support::DEADLINE + Duration::from_secs(wait.into()));
    let response = f
        .client
        .request(Method::GET, f.url(&path))
        .timeout(deadline)
        .bearer_auth(token)
        .header("idempotency-key", key)
        .header("x-tqa-run-id", run)
        .send()
        .await?;
    let status = response.status().as_u16();
    Ok((status, response.json().await?))
}

pub(crate) fn seqs(answer: &Value) -> Vec<u64> {
    answer["messages"]
        .as_array()
        .map(|messages| {
            messages
                .iter()
                .filter_map(|message| message["seq"].as_u64())
                .collect()
        })
        .unwrap_or_default()
}

/// The `tool.result` a read's call ends with, as the runtime sends it.
pub(crate) fn result(sequence: u64, run: &str, call: &str) -> Result<ServerFrame> {
    Ok(ServerFrame::Event {
        seq: sequence.into(),
        run_id: Some(run.parse()?),
        step: Some(1),
        ts: "2026-01-01T00:00:00Z".into(),
        event: Event::ToolResult {
            call_id: call.parse()?,
            status: ToolStatus::Ok,
            content: "read".into(),
            target: None,
            evidence: None,
        },
    })
}

/// Queues a turn for `conversation` with no sandbox, as admission leaves a run that waits for a slot.
pub(crate) async fn queue(f: &Fixture, team: Uuid, conversation: &ConversationId) -> Result<()> {
    let conversation = Uuid::parse_str(conversation.as_str())?;
    sqlx::query("DELETE FROM sandbox_instances WHERE conversation_id = $1")
        .bind(conversation)
        .execute(&f.pool)
        .await?;
    sqlx::query(
        "INSERT INTO idempotency (key, user_id, route, response_status, response_body, conversation_id, \
         queued_frame, execution_status) \
         SELECT $1, owner_id, 'test', 202, '{}', $2, '{}', 'queued' FROM agent_teams WHERE id = $3",
    )
    .bind(format!("queued:{conversation}"))
    .bind(conversation)
    .bind(team)
    .execute(&f.pool)
    .await?;
    Ok(())
}

/// Ends the team as the stop does: `ended` with its time.
pub(crate) async fn end(f: &Fixture, team: Uuid) -> Result<()> {
    sqlx::query(
        "UPDATE agent_teams SET state = 'ended', ended_at = CURRENT_TIMESTAMP WHERE id = $1",
    )
    .bind(team)
    .execute(&f.pool)
    .await?;
    Ok(())
}

pub(crate) async fn done(
    f: &Fixture,
    token: &str,
    key: &str,
    body: &Value,
) -> Result<(u16, Value)> {
    let mut request = f
        .client
        .post(f.url("/runtime/teams/done"))
        .bearer_auth(token)
        .header("idempotency-key", key);
    if let Some(run) = run_of(token) {
        request = request.header("x-tqa-run-id", run);
    }
    let response = request.json(body).send().await?;
    let status = response.status().as_u16();
    Ok((status, response.json().await?))
}

/// Ends `conversation`'s run `run` at `sequence`, as its runtime would report it.
pub(crate) async fn run_ends(
    f: &Fixture,
    conversation: &ConversationId,
    sequence: u64,
    run: &str,
) -> Result<()> {
    f.gateway
        .events()
        .append(
            conversation,
            ServerFrame::Event {
                seq: sequence.into(),
                run_id: Some(run.parse()?),
                step: None,
                ts: "2026-01-01T00:00:01Z".into(),
                event: Event::RunCompleted {
                    run_id: run.parse()?,
                },
            },
        )
        .await?;
    Ok(())
}

/// `reader`'s wakes: their keys, oldest first.
pub(crate) async fn wakes(f: &Fixture, reader: &ConversationId) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT key FROM idempotency WHERE conversation_id = $1 AND key LIKE 'team-wake:%' ORDER BY ordinal",
    )
    .bind(Uuid::parse_str(reader.as_str())?)
    .fetch_all(&f.pool)
    .await?)
}

/// Waits up to 5 s for `reader` to have `count` wakes: the wake follows the post's commit in the background.
pub(crate) async fn wakes_settle(
    f: &Fixture,
    reader: &ConversationId,
    count: usize,
) -> Result<Vec<String>> {
    for _ in 0..50 {
        let found = wakes(f, reader).await?;
        if found.len() >= count {
            return Ok(found);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    wakes(f, reader).await
}
