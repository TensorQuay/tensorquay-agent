//! GW-I-123 (ADR-0013 point 7, revision 9): the gateway gives a runtime the engine its conversation's profile names, and
//! only a runtime that can take it; a revision-8 runtime is refused for such a conversation and links for any other
//! (the floor stays 8). The runtime's `decision`, `decision.fallback` and `run.failed` with `decision_stop` are stored
//! in order, reach clients live and on replay, and end the run.
#[path = "support/fixture.rs"]
mod support;

use secrecy::ExposeSecret;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::TestResult;
use tokio_tungstenite::tungstenite::{Error as WsError, client::IntoClientRequest};
use tqa_types::{MIN_RUNTIME_REVISION, PROTOCOL_REVISION, PROTOCOL_REVISION_HEADER, ServerFrame};

const CONFIG: &str = "[[decision_engines]]\nname = \"router\"\nurl = \"https://engine.example\"\ntimeout_ms = 300\nexposure = \"calls\"\n";

/// Gives the conversation a profile copy naming `engine`, as one made from such a profile has.
async fn name_engine(
    f: &support::Fixture,
    conversation: &tqa_gateway::Conversation,
    engine: Option<&str>,
) -> TestResult {
    let profile = json!({"name":"p","display_name":"P","skills":"all","mcp_servers":[],"connectors":[],"rules":[],
        "limits":{},"decision_engine":engine});
    sqlx::query(
        "UPDATE conversations SET profile=$2, profile_id=$3, profile_version=1 WHERE id=$1::uuid",
    )
    .bind(conversation.id.as_str())
    .bind(&profile)
    .bind(uuid::Uuid::now_v7())
    .execute(&f.pool)
    .await?;
    Ok(())
}

/// Enables or disables the seeded engine through the admin API, from `version`.
async fn set_enabled(
    f: &support::Fixture,
    cookie: &str,
    enabled: bool,
    version: i64,
) -> TestResult {
    let (status, body) = support::call(
        f,
        reqwest::Method::PUT,
        "/api/v1/admin/decision/engines/router",
        cookie,
        (
            Some(
                json!({"url": "https://engine.example", "timeout_ms": 300, "exposure": "calls",
                "enabled": enabled, "version": version}),
            ),
            None,
        ),
    )
    .await?;
    assert_eq!(status, 200, "{body}");
    Ok(())
}

/// The link upgrade's status and error body for a runtime announcing `revision`, with a fresh token.
async fn upgrade(
    f: &support::Fixture,
    conversation: &tqa_gateway::Conversation,
    revision: u16,
) -> Result<(u16, Value), Box<dyn std::error::Error>> {
    let token = f
        .gateway
        .runtime_tokens()
        .mint(
            &conversation.id,
            &format!("rev-{}", uuid::Uuid::now_v7().simple()).parse()?,
            std::net::Ipv4Addr::LOCALHOST.into(),
        )
        .await?;
    dial(f, &token, revision).await
}

/// The link upgrade's status and error body for `token` announcing `revision`.
async fn dial(
    f: &support::Fixture,
    token: &secrecy::SecretString,
    revision: u16,
) -> Result<(u16, Value), Box<dyn std::error::Error>> {
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
                .transpose()?
                .unwrap_or(Value::Null);
            Ok((response.status().as_u16(), body))
        }
        Err(error) => Err(error.into()),
    }
}

fn event(seq: u64, event: &Value) -> Result<ServerFrame, Box<dyn std::error::Error>> {
    let frame = json!({"type":"event","seq":seq,"run_id":"r-1","step":1,"ts":"2026-01-01T00:00:00Z","event":event});
    Ok(ServerFrame::parse(&serde_json::to_vec(&frame)?)?)
}

// TEST: GW-I-123
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_123_session_config_carries_the_profiles_engine_and_nothing_else(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, CONFIG, async |f| {
        let (user, _) = f.admin().await?;
        let named = f.conversation(user.id).await?;
        name_engine(f, &named, Some("router")).await?;
        let runtime = f.runtime(&named).await?;
        assert_eq!(
            runtime.config["decision_engine"],
            json!({"name":"router","relay_path":"/relay/d/router","timeout_ms":300,"exposure":"calls"}),
            "the relay path, deadline and exposure; never the engine's address"
        );
        assert!(!runtime.config.to_string().contains("engine.example"));
        let plain = f.conversation(user.id).await?;
        name_engine(f, &plain, None).await?;
        let runtime = f.runtime(&plain).await?;
        assert!(
            runtime.config.get("decision_engine").is_none(),
            "{}",
            runtime.config
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-123
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_123_a_revision_8_runtime_is_refused_with_or_without_an_engine(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, CONFIG, async |f| {
        let (user, _) = f.admin().await?;
        let named = f.conversation(user.id).await?;
        name_engine(f, &named, Some("router")).await?;
        let (status, body) = upgrade(f, &named, 8).await?;
        assert_eq!(status, 426, "{body}");
        assert_eq!(body["error"]["code"], "runtime_too_old", "{body}");
        // The runtime floor (10 since reply_cut) is above the engine's revision (9), so the refusal names the floor.
        assert_eq!(body["error"]["needs"], MIN_RUNTIME_REVISION);
        assert_eq!(body["error"]["has"], 8);
        // Without an engine a revision-8 runtime, and one just below the floor, are refused all the same; the current
        // revision links.
        let plain = f.conversation(user.id).await?;
        assert_eq!(upgrade(f, &plain, 8).await?.0, 426);
        let below = f.conversation(user.id).await?;
        assert_eq!(upgrade(f, &below, MIN_RUNTIME_REVISION - 1).await?.0, 426);
        let current = f.conversation(user.id).await?;
        assert_eq!(upgrade(f, &current, PROTOCOL_REVISION).await?.0, 101);
        Ok(())
    })
    .await
}

// TEST: GW-I-123
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_123_decision_frames_are_stored_in_order_relayed_and_end_the_run(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, CONFIG, async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        name_engine(f, &conversation, Some("router")).await?;
        let id = &conversation.id;
        let mut runtime = f.runtime(&conversation).await?;
        let mut live = f.stream(id, &cookie, 0).await?;
        let sent = [
            event(1, &json!({"type":"run.started","run_id":"r-1"}))?,
            event(
                2,
                &json!({"type":"decision","point":"before_step","engine":"router",
                    "choice":{"kind":"call_model","route":"local","tools":["read_file"]},"latency_ms":12}),
            )?,
            event(
                3,
                &json!({"type":"decision.fallback","point":"after_response","engine":"router","reason":"timeout"}),
            )?,
            event(
                4,
                &json!({"type":"run.failed","run_id":"r-1","reason":"the engine stopped the run: done","code":"decision_stop"}),
            )?,
        ];
        for frame in &sent {
            runtime.send(frame).await?;
        }
        runtime.roundtrip().await?;
        f.until_sequence(id, 4).await?;
        for (seq, frame) in (1..).zip(&sent) {
            assert_eq!(
                live.durable().await?,
                (seq.into(), frame.clone().for_clients()),
                "live, in order"
            );
        }
        let mut replay = f.stream(id, &cookie, 0).await?;
        for (seq, frame) in (1..).zip(&sent) {
            assert_eq!(
                replay.durable().await?,
                (seq.into(), frame.clone().for_clients()),
                "replay"
            );
        }
        let listed: Value = f.get("/api/v1/conversations", &cookie).await?.json().await?;
        let item = listed["items"]
            .as_array()
            .and_then(|items| items.iter().find(|item| item["id"] == id.as_str()))
            .ok_or("the task")?;
        assert_eq!(item["run_status"], "failed", "the run ended: {item}");
        Ok(())
    })
    .await
}

/// Waits for the conversation's `count`th sandbox and returns its runtime token.
async fn sandbox(
    f: &support::Fixture,
    id: &tqa_types::ConversationId,
    count: usize,
) -> Result<secrecy::SecretString, Box<dyn std::error::Error>> {
    tokio::time::timeout(support::DEADLINE, async {
        loop {
            if f.backend.creates.load(std::sync::atomic::Ordering::SeqCst) >= count
                && let Some(token) = f.backend.token(id)
            {
                return token;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(Into::into)
}

// TEST: GW-I-123
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_123_an_engine_gone_before_the_link_retires_the_sandbox_once_and_visibly(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, CONFIG, async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let id = &conversation.id;
        name_engine(f, &conversation, Some("router")).await?;
        let mut stream = f.stream(id, &cookie, 0).await?;
        let path = format!("/api/v1/conversations/{id}/messages");
        let accepted = f
            .action(&path, &cookie, Some("engine-1"), &json!({"text":"start"}))
            .await?;
        assert_eq!(accepted.status(), 202, "accepted while the engine exists");
        let token = sandbox(f, id, 1).await?;
        // An administrator disables the engine before the runtime links.
        set_enabled(f, &cookie, false, 1).await?;
        let (status, body) = dial(f, &token, tqa_types::PROTOCOL_REVISION).await?;
        assert_eq!(status, 409, "{body}");
        assert_eq!(
            body["error"]["code"], "decision_engine_unavailable",
            "{body}"
        );
        // The owner is told why, retryable, and then sees the sandbox failed.
        let mut told = false;
        for _ in 0..16 {
            match stream.next().await?.1 {
                ServerFrame::Error {
                    code,
                    message,
                    retryable,
                    ..
                } => {
                    assert_eq!(
                        (code, retryable),
                        (tqa_types::ErrorCode::DecisionEngineUnavailable, true)
                    );
                    assert!(message.contains("restore or re-enable"), "{message}");
                    told = true;
                }
                ServerFrame::SessionReady { sandbox, .. }
                    if sandbox.state == tqa_types::SandboxState::Failed =>
                {
                    break;
                }
                _ => {}
            }
        }
        assert!(told, "the notice came before Failed");
        // The token is revoked, so the runtime's next dial is refused and it stops; nothing replaced the sandbox.
        assert_eq!(dial(f, &token, tqa_types::PROTOCOL_REVISION).await?.0, 401);
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        assert_eq!(
            f.backend.creates.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        // While the engine is gone a message is refused; once it is back, a message starts a sandbox that links.
        let refused = f
            .action(&path, &cookie, Some("engine-2"), &json!({"text":"again"}))
            .await?;
        assert_eq!(refused.status(), 409);
        set_enabled(f, &cookie, true, 2).await?;
        let resumed = f
            .action(&path, &cookie, Some("engine-3"), &json!({"text":"again"}))
            .await?;
        assert_eq!(resumed.status(), 202);
        sandbox(f, id, 2).await?;
        f.forget_runtime(id)?;
        let runtime = f.runtime_for(id).await?;
        assert_eq!(runtime.config["decision_engine"]["name"], "router");
        Ok(())
    })
    .await
}

// TEST: GW-I-123
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_123_work_queued_before_the_engine_went_fails_once_and_visibly(
    pool: PgPool,
) -> TestResult {
    let config = format!("{CONFIG}[admission]\nslots=1\n");
    support::run_configured(pool, None, &config, async |f| {
        let (user, cookie) = f.admin().await?;
        // The one slot is held by a run in progress, so the next task's message waits for it.
        let busy = f.conversation(user.id).await?;
        let mut runtime = f.runtime(&busy).await?;
        runtime
            .send(&event(1, &json!({"type":"run.started","run_id":"r-1"}))?)
            .await?;
        runtime.roundtrip().await?;
        f.until_sequence(&busy.id, 1).await?;
        let waiting = f.conversation(user.id).await?;
        name_engine(f, &waiting, Some("router")).await?;
        let path = format!("/api/v1/conversations/{}/messages", waiting.id);
        let accepted = f
            .action(&path, &cookie, Some("queued-1"), &json!({"text":"start"}))
            .await?;
        assert_eq!(accepted.status(), 202);
        f.gateway.sandboxes().admission_pass_now().await?;
        assert!(f.backend.created(&waiting.id).is_none(), "still queued");
        let mut stream = f.stream(&waiting.id, &cookie, 0).await?;
        // An administrator disables the engine while the message waits: the next pass fails it once, visibly, and
        // starts nothing.
        set_enabled(f, &cookie, false, 1).await?;
        for _ in 0..3 {
            f.gateway.sandboxes().admission_pass_now().await?;
        }
        let mut told = 0;
        let mut failed = false;
        for _ in 0..16 {
            match stream.next().await?.1 {
                ServerFrame::Error {
                    code, retryable, ..
                } => {
                    assert_eq!(
                        (code, retryable),
                        (tqa_types::ErrorCode::DecisionEngineUnavailable, true)
                    );
                    told += 1;
                }
                ServerFrame::SessionReady { sandbox, .. }
                    if sandbox.state == tqa_types::SandboxState::Failed =>
                {
                    failed = true;
                    break;
                }
                _ => {}
            }
        }
        assert_eq!((told, failed), (1, true), "one notice, then Failed");
        assert!(f.backend.created(&waiting.id).is_none(), "no sandbox");
        let status: String = sqlx::query_scalar(
            "SELECT execution_status FROM idempotency WHERE conversation_id=$1::uuid",
        )
        .bind(waiting.id.as_str())
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(status, "failed", "the queued action failed");
        // A stream opened afterwards is told too, with the same code.
        let mut late = f.stream(&waiting.id, &cookie, 0).await?;
        let mut replayed = None;
        for _ in 0..8 {
            if let ServerFrame::Error { code, .. } = late.next().await?.1 {
                replayed = Some(code);
                break;
            }
        }
        assert_eq!(
            replayed,
            Some(tqa_types::ErrorCode::DecisionEngineUnavailable)
        );
        Ok(())
    })
    .await
}
