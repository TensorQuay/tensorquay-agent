//! Agent teams, slice 2 (ADR-0034): a member's first turn.
//! - GW-I-106: the gateway sends the member its first turn as the lead's brief (protocol revision 12): the fixed frame
//!   as its text and the brief beside it, never the person's words; an own copy's turn goes once the copy is ready,
//!   and a turn for the lead's own files waits for the lead's sandbox even where the installation refuses busy
//!   workspaces; a retried start sends no second turn; a member's runtime below revision 12 is refused at the link,
//!   while any other task links from the floor.
//! - GW-I-106 also: a member whose copy is still being made waits for it through the sweep, neither failed nor sent a
//!   turn, and gets its turn once the copy is ready.
//! - GW-I-214: a team's lead or member whose runtime announces less than revision 13 is refused at the link, needing
//!   13 (the gateway's wake carries its marker from 13), even at 12, which reads a brief.
//! - GW-I-300: a member's first turn waiting for the lead's files is listed as the gateway's (`lead_brief`), never the
//!   person's message, and no one may cancel it (`409 team_turn`): cancelled, the member would never get its brief.
//! - GW-I-215: a member's own copy of its lead's workspace is `kind: helper_copy`, named by the member's task, and
//!   every other workspace `standard`; the upgrade (migration 0081) marks existing copies through the member records,
//!   never by a name, and strips only their suffix.
//! - GW-I-211: once its first turn is stored, a member is no longer starting, so after its run ends its lead's message
//!   wakes it with a turn; nothing here sets a member's state by hand, except to stand in for a failure the sweep
//!   repairs.
#[path = "support/fixture.rs"]
mod support;
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::time::Duration;
use support::{Fixture, TestResult};
use tokio_tungstenite::tungstenite::{Error as WsError, client::IntoClientRequest};
use tqa_types::{
    ClientFrame, ConversationId, Event, PROTOCOL_REVISION_HEADER, ServerFrame, WorkspaceMode,
};
use uuid::Uuid;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const ENABLED: &str = "[agent_teams]\nenabled = true\n";

/// The lead: a person's task in a run, as it is when it calls `start_member`, with its runtime connected, and the
/// runtime's token.
async fn lead(f: &Fixture) -> Result<(Uuid, String, support::runtime::Runtime)> {
    let (lead, token, runtime, _) = owned_lead(f).await?;
    Ok((lead, token, runtime))
}

/// [`lead`], with its person's session cookie.
async fn owned_lead(f: &Fixture) -> Result<(Uuid, String, support::runtime::Runtime, String)> {
    f.admin().await?;
    let (person, cookie) = f.member("owner@example.invalid").await?;
    let conversation = f.conversation(person.id).await?;
    let runtime = f.runtime(&conversation).await?;
    f.gateway
        .events()
        .append(&conversation.id, support::started(1, "r-1")?)
        .await?;
    let token = f
        .backend
        .token(&conversation.id)
        .ok_or("token")?
        .expose_secret()
        .to_owned();
    Ok((
        Uuid::parse_str(conversation.id.as_str())?,
        token,
        runtime,
        cookie,
    ))
}

async fn start(f: &Fixture, token: &str, key: &str, workspace: &str) -> Result<Value> {
    let response = f
        .client
        .post(f.url("/runtime/teams/members"))
        .bearer_auth(token)
        .header("idempotency-key", key)
        .header("x-tqa-run-id", "r-1")
        .json(&json!({
            "role": "researcher", "workspace": workspace,
            "brief": {"objective": "Find the Q3 revenue by region.", "output": "A table.",
                      "boundaries": "Don't edit the reports."},
        }))
        .send()
        .await?;
    assert_eq!(response.status(), 201);
    Ok(response.json().await?)
}

fn member_id(started: &Value) -> Result<ConversationId> {
    Ok(started["member"]["conversation"]
        .as_str()
        .ok_or("conversation")?
        .parse()?)
}

/// Waits, bounded, until `ready` holds.
async fn until<F: AsyncFn() -> Result<bool>>(ready: F) -> TestResult {
    let deadline = tokio::time::Instant::now() + support::DEADLINE;
    while !ready().await? {
        if tokio::time::Instant::now() > deadline {
            return Err("timed out".into());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok(())
}

/// The member's queued first turns: rows under the start key, and how many are still queued.
async fn first_turns(f: &Fixture, member: &ConversationId) -> Result<(i64, i64)> {
    Ok(sqlx::query_as(
        "SELECT count(*), count(*) FILTER (WHERE execution_status = 'queued') FROM idempotency \
         WHERE conversation_id = $1 AND key = $2",
    )
    .bind(Uuid::parse_str(member.as_str())?)
    .bind(format!("team-start:{member}"))
    .fetch_one(&f.pool)
    .await?)
}

/// As if `member`'s first turn had never been stored: no turn under its key, and the member still starting, as a failed
/// send or a restart before it leaves it (the member is marked working only once its turn is stored).
async fn unsent(f: &Fixture, member: &ConversationId) -> TestResult {
    let id = Uuid::parse_str(member.as_str())?;
    let mut transaction = f.pool.begin().await?;
    sqlx::query("DELETE FROM idempotency WHERE conversation_id = $1")
        .bind(id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("UPDATE agent_team_members SET state = 'starting' WHERE conversation_id = $1")
        .bind(id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(())
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_106_an_own_copys_first_turn_is_the_lead_brief_once_the_copy_is_ready(
    pool: PgPool,
) -> TestResult {
    Box::pin(support::run_configured(pool, None, ENABLED, async |f| {
        let (_, token, _runtime) = lead(f).await?;
        let started = start(f, &token, "c-1", "own_copy").await?;
        let member = member_id(&started)?;
        until(async || Ok(f.backend.created(&member).is_some())).await?;
        let state: String = sqlx::query_scalar("SELECT state FROM workspaces WHERE id = $1")
            .bind(Uuid::parse_str(
                started["member"]["workspace_id"]
                    .as_str()
                    .ok_or("workspace")?,
            )?)
            .fetch_one(&f.pool)
            .await?;
        assert_eq!(state, "ready", "the turn went only once the copy was ready");
        let mut runtime = f.runtime_for(&member).await?;
        let ClientFrame::UserMessage {
            text, lead_brief, ..
        } = runtime.action().await?
        else {
            return Err("not a message".into());
        };
        let brief = lead_brief.ok_or("the first turn carries the lead's brief")?;
        assert_eq!(
            (brief.role.as_str(), brief.workspace),
            ("researcher", WorkspaceMode::OwnCopy)
        );
        assert_eq!(brief.brief.objective, "Find the Q3 revenue by region.");
        assert!(
            text.contains("researcher") && text.contains("send_to_lead"),
            "{text}"
        );
        // The copy was taken from the lead's live files, and the member is told; no title in the frame.
        assert_eq!(started["member"]["consistent"], false);
        assert!(text.contains("may be partial"), "{text}");
        assert!(
            !text.contains("Fixture"),
            "the lead's title stays out of the gateway's sentence: {text}"
        );
        assert!(
            !text.contains("Find the Q3 revenue"),
            "the brief rides beside the frame, not in it"
        );
        // A retried start sends no second turn.
        assert_eq!(start(f, &token, "c-1", "own_copy").await?, started);
        assert_eq!(first_turns(f, &member).await?.0, 1);
        Ok(())
    }))
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_106_a_turn_on_the_leads_files_waits_for_its_sandbox_even_where_busy_is_refused(
    pool: PgPool,
) -> TestResult {
    let config = format!("{ENABLED}[workspaces]\nbusy_policy = 'refuse'\n");
    Box::pin(support::run_configured(pool, None, &config, async |f| {
        let (_, token, _runtime) = lead(f).await?;
        let started = start(f, &token, "c-1", "my_files").await?;
        let member = member_id(&started)?;
        // Queued behind the lead's live sandbox, never refused as workspace_busy, and no sandbox of its own yet.
        until(async || Ok(first_turns(f, &member).await? == (1, 1))).await?;
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(f.backend.created(&member).is_none());
        assert_eq!(first_turns(f, &member).await?, (1, 1));
        assert_eq!(
            member_state(f, &member).await?,
            "queued",
            "its turn waits for the lead's files"
        );
        Ok(())
    }))
    .await
}

/// The link upgrade's status and error body for `conversation`'s runtime announcing `revision`, with a fresh token.
async fn dial(f: &Fixture, conversation: &ConversationId, revision: u16) -> Result<(u16, Value)> {
    let token = f
        .gateway
        .runtime_tokens()
        .mint(
            conversation,
            &format!("rev-{}", Uuid::now_v7().simple()).parse()?,
            std::net::Ipv4Addr::LOCALHOST.into(),
        )
        .await?;
    let mut request = f
        .url("/runtime/link")
        .replacen("http:", "ws:", 1)
        .into_client_request()?;
    let headers = request.headers_mut();
    headers.insert(
        "authorization",
        format!("Bearer {}", token.expose_secret()).parse()?,
    );
    headers.insert(PROTOCOL_REVISION_HEADER, revision.to_string().parse()?);
    match tokio::time::timeout(support::DEADLINE, tokio_tungstenite::connect_async(request)).await?
    {
        Ok((socket, response)) => {
            drop(socket);
            Ok((response.status().as_u16(), Value::Null))
        }
        Err(WsError::Http(response)) => {
            let body = response
                .body()
                .as_deref()
                .map(serde_json::from_slice)
                .transpose()?;
            Ok((response.status().as_u16(), body.unwrap_or(Value::Null)))
        }
        Err(error) => Err(error.into()),
    }
}

// TEST: GW-I-214
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_214_a_team_lead_or_member_below_revision_13_is_refused(pool: PgPool) -> TestResult {
    Box::pin(support::run_configured(pool, None, ENABLED, async |f| {
        let (lead_id, token, _runtime) = lead(f).await?;
        let started = start(f, &token, "c-1", "my_files").await?;
        let member = member_id(&started)?;
        let lead: ConversationId = lead_id.to_string().parse()?;
        // At 12 a member could read its brief, but a wake would reach the page as the person's words.
        for conversation in [&member, &lead] {
            let (status, body) = dial(f, conversation, tqa_types::LEAD_BRIEF_REVISION).await?;
            assert_eq!(
                (status, &body["error"]["code"], &body["error"]["needs"]),
                (
                    426,
                    &json!("runtime_too_old"),
                    &json!(tqa_types::TEAM_WAKE_REVISION)
                ),
                "{conversation}: {body}"
            );
        }
        Ok(())
    }))
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_106_a_members_runtime_below_12_is_refused_and_any_other_links(
    pool: PgPool,
) -> TestResult {
    Box::pin(support::run_configured(pool, None, ENABLED, async |f| {
        let (_, token, _runtime) = lead(f).await?;
        let started = start(f, &token, "c-1", "my_files").await?;
        let member = member_id(&started)?;
        let (status, body) = dial(f, &member, tqa_types::LEAD_BRIEF_REVISION - 1).await?;
        assert_eq!(
            (status, &body["error"]["code"]),
            (426, &json!("runtime_too_old")),
            "{body}"
        );
        // Every team conversation needs the wake's revision now (GW-I-214).
        assert_eq!(body["error"]["needs"], tqa_types::TEAM_WAKE_REVISION);
        // A task that isn't a member links from the floor.
        let (person, _) = f.member("other@example.invalid").await?;
        let other = f.conversation(person.id).await?;
        let (status, body) = dial(f, &other.id, tqa_types::MIN_RUNTIME_REVISION).await?;
        assert_eq!(status, 101, "{body}");
        Ok(())
    }))
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_106_a_copy_that_fails_fails_its_member_and_tells_the_lead(
    pool: PgPool,
) -> TestResult {
    Box::pin(support::run_configured(pool, None, ENABLED, async |f| {
        let (lead_id, token, _runtime) = lead(f).await?;
        // The lead's files are over the copy's quota (its source's own, 1 MiB), so the copy fails.
        let workspace: Uuid =
            sqlx::query_scalar("SELECT workspace_id FROM conversations WHERE id = $1")
                .bind(lead_id)
                .fetch_one(&f.pool)
                .await?;
        sqlx::query("UPDATE workspaces SET quota_bytes = 1048576 WHERE id = $1")
            .bind(workspace)
            .execute(&f.pool)
            .await?;
        let files = f
            .config_storage_root()
            .join("workspaces")
            .join(workspace.to_string());
        std::fs::create_dir_all(&files)?;
        std::fs::write(files.join("big.bin"), vec![0_u8; 2 * 1024 * 1024])?;
        let started = start(f, &token, "c-1", "own_copy").await?;
        let member = member_id(&started)?;
        let failed = async || -> Result<bool> {
            let state: String = sqlx::query_scalar(
                "SELECT state FROM agent_team_members WHERE conversation_id = $1",
            )
            .bind(Uuid::parse_str(member.as_str())?)
            .fetch_one(&f.pool)
            .await?;
            Ok(state == "failed")
        };
        until(failed).await?;
        let (author, to, body): (String, String, Value) = sqlx::query_as(
            "SELECT m.author_kind, m.to_kind, m.body FROM channel_messages m JOIN agent_teams t \
             ON t.channel_id = m.channel_id WHERE t.lead_conversation = $1 AND m.type = 'notice'",
        )
        .bind(lead_id)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!((author.as_str(), to.as_str()), ("gateway", "lead"));
        assert_eq!(
            (&body["kind"], &body["member"]),
            (&json!("member_failed"), &json!("researcher"))
        );
        assert_eq!(
            first_turns(f, &member).await?.0,
            0,
            "no turn for a member without its files"
        );
        Ok(())
    }))
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_106_a_start_cut_short_by_a_restart_sends_its_first_turn_at_start_up(
    pool: PgPool,
) -> TestResult {
    support::run_restarting_with(pool, None, ENABLED, async |f| {
        let (_, token, runtime) = lead(f).await?;
        let started = start(f, &token, "c-1", "my_files").await?;
        let member = member_id(&started)?;
        until(async || Ok(first_turns(f, &member).await?.0 == 1)).await?;
        // As if the gateway stopped between the start's commit and its first turn.
        unsent(f, &member).await?;
        drop(runtime);
        f.restart().await?;
        until(async || Ok(first_turns(f, &member).await?.0 == 1)).await?;
        // A second start-up finds the turn sent and sends no other.
        f.restart().await?;
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(first_turns(f, &member).await?.0, 1);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_106_a_first_turn_that_failed_to_send_is_sent_by_the_sweep(
    pool: PgPool,
) -> TestResult {
    Box::pin(support::run_configured(pool, None, ENABLED, async |f| {
        let (_, token, _runtime) = lead(f).await?;
        let started = start(f, &token, "c-1", "my_files").await?;
        let member = member_id(&started)?;
        until(async || Ok(first_turns(f, &member).await?.0 == 1)).await?;
        // As if the send after the start had failed: no turn stored, and no restart.
        unsent(f, &member).await?;
        until(async || Ok(first_turns(f, &member).await?.0 == 1)).await?;
        Ok(())
    }))
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_106_a_member_whose_copy_is_still_being_made_waits_through_the_sweep(
    pool: PgPool,
) -> TestResult {
    Box::pin(support::run_configured(pool, None, ENABLED, async |f| {
        let (_, token, _runtime) = lead(f).await?;
        let waiting = start(f, &token, "c-1", "own_copy").await?;
        let (copying, sent) = (
            member_id(&waiting)?,
            member_id(&start(f, &token, "c-2", "own_copy").await?)?,
        );
        until(async || {
            Ok(first_turns(f, &copying).await?.0 == 1 && first_turns(f, &sent).await?.0 == 1)
        })
        .await?;
        // On purpose, standing in for a slow copy: no turn stored for either, one's copy still being made.
        unsent(f, &copying).await?;
        let workspace = Uuid::parse_str(
            waiting["member"]["workspace_id"]
                .as_str()
                .ok_or("workspace")?,
        )?;
        sqlx::query("UPDATE workspaces SET state = 'forking' WHERE id = $1")
            .bind(workspace)
            .execute(&f.pool)
            .await?;
        unsent(f, &sent).await?;
        // The sweep sends the ready one its turn: that pass looked at the other too.
        until(async || Ok(first_turns(f, &sent).await?.0 == 1)).await?;
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(
            member_state(f, &copying).await?,
            "starting",
            "it waits for its copy, not failed"
        );
        assert_eq!(first_turns(f, &copying).await?.0, 0, "and gets no turn yet");
        // Once its copy is ready, the sweep sends its turn.
        sqlx::query("UPDATE workspaces SET state = 'ready' WHERE id = $1")
            .bind(workspace)
            .execute(&f.pool)
            .await?;
        until(async || Ok(first_turns(f, &copying).await?.0 == 1)).await?;
        Ok(())
    }))
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_106_a_member_whose_first_turn_never_went_fails_and_tells_the_lead(
    pool: PgPool,
) -> TestResult {
    Box::pin(support::run_configured(pool, None, ENABLED, async |f| {
        let (lead_id, token, _runtime) = lead(f).await?;
        let started = start(f, &token, "c-1", "my_files").await?;
        let member = member_id(&started)?;
        let id = Uuid::parse_str(member.as_str())?;
        until(async || Ok(first_turns(f, &member).await?.0 == 1)).await?;
        // Ten minutes and more without its turn stored: the next sweep fails it rather than letting it wait forever.
        let mut transaction = f.pool.begin().await?;
        sqlx::query("DELETE FROM idempotency WHERE conversation_id = $1").bind(id).execute(&mut *transaction).await?;
        sqlx::query("UPDATE agent_team_members SET state = 'starting', \
                     created_at = CURRENT_TIMESTAMP - INTERVAL '11 minutes' WHERE conversation_id = $1")
            .bind(id)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        let failed = async || -> Result<bool> {
            let state: String = sqlx::query_scalar("SELECT state FROM agent_team_members WHERE conversation_id = $1")
                .bind(id)
                .fetch_one(&f.pool)
                .await?;
            Ok(state == "failed")
        };
        until(failed).await?;
        let body: Value = sqlx::query_scalar(
            "SELECT m.body FROM channel_messages m JOIN agent_teams t ON t.channel_id = m.channel_id \
             WHERE t.lead_conversation = $1 AND m.type = 'notice'",
        )
        .bind(lead_id)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(body["kind"], "member_failed");
        assert!(body["text"].as_str().is_some_and(|text| text.contains("brief couldn't reach it")), "{body}");
        assert_eq!(first_turns(f, &member).await?.0, 0, "a failed member gets no turn");
        Ok(())
    }))
    .await
}

async fn member_state(f: &Fixture, member: &ConversationId) -> Result<String> {
    Ok(
        sqlx::query_scalar("SELECT state FROM agent_team_members WHERE conversation_id = $1")
            .bind(Uuid::parse_str(member.as_str())?)
            .fetch_one(&f.pool)
            .await?,
    )
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_211_a_member_whose_run_ended_is_woken_by_its_leads_message(
    pool: PgPool,
) -> TestResult {
    Box::pin(support::run_configured(pool, None, ENABLED, async |f| {
        let (_, token, _runtime) = lead(f).await?;
        let member = member_id(&start(f, &token, "c-1", "own_copy").await?)?;
        until(async || Ok(f.backend.created(&member).is_some())).await?;
        let mut runtime = f.runtime_for(&member).await?;
        let ClientFrame::UserMessage {
            lead_brief: Some(_),
            ..
        } = runtime.action().await?
        else {
            return Err("the first turn is the lead's brief".into());
        };
        // Its brief is stored: queued until its runtime takes the turn, or already working.
        let first = member_state(f, &member).await?;
        assert!(matches!(first.as_str(), "queued" | "working"), "{first}");
        // The member's run starts and ends, as its runtime reports them, with nothing read: waiting for its lead.
        f.gateway
            .events()
            .append(&member, support::started(1, "m-1")?)
            .await?;
        assert_eq!(member_state(f, &member).await?, "working");
        let ended = Event::RunCompleted {
            run_id: "m-1".parse()?,
        };
        let frame = ServerFrame::Event {
            seq: 2.into(),
            run_id: Some("m-1".parse()?),
            step: None,
            ts: "2026-01-01T00:00:01Z".into(),
            event: ended,
        };
        f.gateway.events().append(&member, frame).await?;
        assert_eq!(
            member_state(f, &member).await?,
            "waiting",
            "no report since its assignment"
        );
        let posted = f
            .client
            .post(f.url("/runtime/teams/messages"))
            .bearer_auth(&token)
            .header("idempotency-key", "r-1:c-2")
            .header("x-tqa-run-id", "r-1")
            .json(&json!({"type": "assign", "to": "researcher", "text": "Round half up."}))
            .send()
            .await?;
        assert_eq!(posted.status(), 201);
        // The wake is the gateway's: its fixed sentence, marked (revision 13), no brief.
        let ClientFrame::UserMessage {
            text,
            lead_brief: None,
            team_wake: Some(_),
            ..
        } = runtime.action().await?
        else {
            return Err(
                "the wake is a turn of the gateway's fixed sentence, marked as a wake".into(),
            );
        };
        assert!(
            text.starts_with("New messages from your lead since message"),
            "{text}"
        );
        Ok(())
    }))
    .await
}

// TEST: GW-I-111
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_111_a_helper_whose_run_ends_without_reading_is_told_idle(pool: PgPool) -> TestResult {
    // E2E-AGENTS-029 on the pre-16d build: the first turn carries the brief, so the brief is read; a helper that ends
    // without a read call and without a report is told to the lead as idle, not taken for one with work waiting.
    Box::pin(support::run_configured(pool, None, ENABLED, async |f| {
        let (_, token, _runtime) = lead(f).await?;
        let member = member_id(&start(f, &token, "c-1", "own_copy").await?)?;
        until(async || Ok(f.backend.created(&member).is_some())).await?;
        let mut runtime = f.runtime_for(&member).await?;
        let ClientFrame::UserMessage {
            lead_brief: Some(_),
            ..
        } = runtime.action().await?
        else {
            return Err("the first turn is the lead's brief".into());
        };
        let member_uuid = Uuid::parse_str(member.as_str())?;
        let read: i64 = sqlx::query_scalar(
            "SELECT m.last_read_seq FROM agent_team_members m WHERE m.conversation_id = $1",
        )
        .bind(member_uuid)
        .fetch_one(&f.pool)
        .await?;
        let brief: i64 = sqlx::query_scalar(
            "SELECT seq FROM channel_messages WHERE to_conversation = $1 AND type = 'assign'",
        )
        .bind(member_uuid)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(read, brief, "the first turn delivered the brief, so it is read");
        // Its run starts and ends with nothing read and no report, then 11 minutes pass (the default limit is 10).
        f.gateway
            .events()
            .append(&member, support::started(1, "m-1")?)
            .await?;
        let frame = ServerFrame::Event {
            seq: 2.into(),
            run_id: Some("m-1".parse()?),
            step: None,
            ts: "2026-01-01T00:00:01Z".into(),
            event: Event::RunCompleted {
                run_id: "m-1".parse()?,
            },
        };
        f.gateway.events().append(&member, frame).await?;
        sqlx::query("UPDATE events SET created_at = created_at - INTERVAL '11 minutes' WHERE conversation_id = $1")
            .bind(member_uuid)
            .execute(&f.pool)
            .await?;
        f.gateway.reconcile_agent_teams_now().await?;
        let told: Vec<String> = sqlx::query_scalar(
            "SELECT n.body->>'kind' FROM channel_messages n JOIN agent_team_members m \
             ON m.conversation_id = $1 JOIN agent_teams t ON t.id = m.team_id \
             WHERE n.channel_id = t.channel_id AND n.type = 'notice' AND n.body->>'member' = m.name",
        )
        .bind(member_uuid)
        .fetch_all(&f.pool)
        .await?;
        assert_eq!(told, ["member_idle"]);
        Ok(())
    }))
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_211_a_member_left_starting_after_its_first_turn_is_marked_again_by_the_sweep(
    pool: PgPool,
) -> TestResult {
    Box::pin(support::run_configured(pool, None, ENABLED, async |f| {
        let (_, token, _runtime) = lead(f).await?;
        let member = member_id(&start(f, &token, "c-1", "my_files").await?)?;
        until(async || Ok(first_turns(f, &member).await?.0 == 1)).await?;
        // Its turn waits behind the lead's live sandbox, so the send marks it queued.
        until(async || Ok(member_state(f, &member).await? == "queued")).await?;
        // As if the gateway had failed between storing the turn and marking the member.
        sqlx::query("UPDATE agent_team_members SET state = 'starting' WHERE conversation_id = $1")
            .bind(Uuid::parse_str(member.as_str())?)
            .execute(&f.pool)
            .await?;
        until(async || Ok(member_state(f, &member).await? == "queued")).await?;
        assert_eq!(first_turns(f, &member).await?.0, 1, "no second first turn");
        Ok(())
    }))
    .await
}

/// The workspace `member` works on, and the member task's title.
async fn copy_of(f: &Fixture, member: &ConversationId) -> Result<(Uuid, String)> {
    Ok(
        sqlx::query_as("SELECT workspace_id, title FROM conversations WHERE id = $1")
            .bind(Uuid::parse_str(member.as_str())?)
            .fetch_one(&f.pool)
            .await?,
    )
}

/// Each workspace's kind and name, by id.
async fn kinds(f: &Fixture, ids: &[Uuid]) -> Result<Vec<(String, String)>> {
    let mut found = Vec::new();
    for id in ids {
        found.push(
            sqlx::query_as("SELECT kind, name FROM workspaces WHERE id = $1")
                .bind(id)
                .fetch_one(&f.pool)
                .await?,
        );
    }
    Ok(found)
}

// TEST: GW-I-215
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_215_a_helpers_own_copy_is_marked_and_named_by_its_task(pool: PgPool) -> TestResult {
    Box::pin(support::run_configured(pool, None, ENABLED, async |f| {
        let (_, token, _runtime) = lead(f).await?;
        let member = member_id(&start(f, &token, "c-1", "own_copy").await?)?;
        let (copy, title) = copy_of(f, &member).await?;
        let (_, cookie) = f.login("owner@example.invalid").await?;
        let (status, body) = support::call(
            f,
            reqwest::Method::GET,
            &format!("/api/v1/workspaces/{copy}"),
            &cookie,
            (None, None),
        )
        .await?;
        assert_eq!(status, 200, "{body}");
        // The kind for the page to word, the name the task's own: no English suffix from the gateway.
        assert_eq!(
            (&body["kind"], &body["name"]),
            (&json!("helper_copy"), &json!(title)),
            "{body}"
        );
        let (status, listed) = support::call(
            f,
            reqwest::Method::GET,
            "/api/v1/workspaces",
            &cookie,
            (None, None),
        )
        .await?;
        assert_eq!(status, 200, "{listed}");
        let items = listed["items"].as_array().ok_or("items")?;
        assert!(items.len() >= 2, "{listed}");
        for item in items {
            let expected = if item["id"] == json!(copy) {
                "helper_copy"
            } else {
                "standard"
            };
            assert_eq!(item["kind"], expected, "{item}");
        }
        Ok(())
    }))
    .await
}

// TEST: GW-I-215
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_215_the_upgrade_marks_copies_by_their_members_never_by_their_names(
    pool: PgPool,
) -> TestResult {
    Box::pin(support::run_configured(pool, None, ENABLED, async |f| {
        let (_, token, _runtime) = lead(f).await?;
        let member = member_id(&start(f, &token, "c-1", "own_copy").await?)?;
        let (copy, title) = copy_of(f, &member).await?;
        // A person's own workspace whose name happens to end as the old copies' did.
        let (_, cookie) = f.login("owner@example.invalid").await?;
        let (status, notes) = support::call(
            f,
            reqwest::Method::POST,
            "/api/v1/workspaces",
            &cookie,
            (Some(json!({"name": "Notes (helper copy)"})), None),
        )
        .await?;
        assert_eq!(status, 201, "{notes}");
        let notes: Uuid = notes["id"].as_str().ok_or("id")?.parse()?;
        // The data as the release before left it: no kind, and the gateway's suffix on the copy's name.
        sqlx::raw_sql("ALTER TABLE workspaces DROP COLUMN kind")
            .execute(&f.pool)
            .await?;
        sqlx::query("UPDATE workspaces SET name = name || ' (helper copy)' WHERE id = $1")
            .bind(copy)
            .execute(&f.pool)
            .await?;
        let upgrade = tqa_gateway::MIGRATOR
            .migrations
            .iter()
            .find(|migration| migration.version == 81)
            .ok_or("no migration 0081")?;
        sqlx::raw_sql(upgrade.sql.as_ref()).execute(&f.pool).await?;
        assert_eq!(
            kinds(f, &[copy, notes]).await?,
            [
                ("helper_copy".to_owned(), title),
                ("standard".to_owned(), "Notes (helper copy)".to_owned()),
            ]
        );
        Ok(())
    }))
    .await
}

// TEST: GW-I-300
// The c16j team demo, finding 1, as for the wake (GW-I-298): a helper's first turn is queued as the wake is, under the team's
// owner on the message route, so while it waits (here for the lead's files, as the demo's my_files helpers did) the
// helper's page listed the gateway's frame as the person's own message, with Cancel. The list marks it, and its DELETE
// is refused: cancelled, its key would replay as cancelled and the member would never get its brief.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_300_a_members_waiting_first_turn_is_listed_as_the_gateways_and_cannot_be_cancelled(
    pool: PgPool,
) -> TestResult {
    Box::pin(support::run_configured(pool, None, ENABLED, async |f| {
        let (_, token, _runtime, cookie) = owned_lead(f).await?;
        let started = start(f, &token, "c-1", "my_files").await?;
        let member = member_id(&started)?;
        until(async || Ok(first_turns(f, &member).await? == (1, 1))).await?;
        let path = format!("/api/v1/conversations/{member}/messages/queued");
        let body: Value = f.get(&path, &cookie).await?.json().await?;
        let items = body["items"].as_array().ok_or("items")?;
        assert_eq!(items.len(), 1, "{body}");
        assert_eq!(items[0]["state"], "waiting_for_slot", "{body}");
        assert_eq!(items[0]["lead_brief"], true, "{body}");
        assert!(items[0].get("team_wake").is_none(), "{body}");
        let id = items[0]["id"].as_str().ok_or("id")?;
        let response = f
            .client
            .delete(f.url(&format!("{path}/{id}")))
            .header("origin", f.origin())
            .header("cookie", &cookie)
            .send()
            .await?;
        assert_eq!(response.status(), 409);
        let refused: Value = response.json().await?;
        assert_eq!(refused["error"]["code"], "team_turn", "{refused}");
        assert_eq!(first_turns(f, &member).await?, (1, 1), "it still waits");
        Ok(())
    }))
    .await
}
