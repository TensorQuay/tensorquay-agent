//! SEC-007 (gateway half): key-shaped strings in tool output stay out of the gateway's logs at its default filter. The
//! gateway sees tool output twice: as the events a runtime sends on its link, and inside the next model request it
//! relays, whose refusal the provider may echo back. Both pass through; neither is logged.
#[path = "support/log_capture.rs"]
mod log_capture;
#[path = "support/fixture.rs"]
mod support;
use secrecy::ExposeSecret;
use serde_json::json;
use sqlx::PgPool;
use std::sync::Arc;
use support::TestResult;
use tqa_testkit::{FakeModelServer, Script, Scripted};
use tqa_types::{Event, ServerFrame, ToolStatus};

/// Key shapes a tool might print: a provider key, a Git host token, a cloud secret and a bearer token. Synthetic, and
/// assembled here so the source holds no token-format literal (the secret scan refuses those in any file).
fn keys() -> [String; 4] {
    let body = "Zq9vXk2LmPq7RtYw3NcBd8FhJs6GaUe4TiOy1WxVb5Mn0KlPo";
    [
        ["sk", "-ant-api03-", body].concat(),
        ["gh", "p_", &body[..36]].concat(),
        "wJalrXUtnFEMI/K7MDENG/bPxRfiCYzq9XkQ2vLm".to_owned(),
        [
            "eyJhbGciOiJIUzI1NiJ9",
            "eyJzdWIiOiJ0ZXN0LXVzZXIifQ",
            "c2lnbmF0dXJlLWZvci10ZXN0",
        ]
        .join("."),
    ]
}

/// The process's subscriber at the gateway's own default filter, so it also sees spawned tasks.
fn capture_logs() -> Result<log_capture::Capture, Box<dyn std::error::Error>> {
    static CAPTURE: std::sync::OnceLock<Option<log_capture::Capture>> = std::sync::OnceLock::new();
    CAPTURE
        .get_or_init(|| {
            let capture = log_capture::Capture::default();
            let filter = tqa_gateway::logging::filter(None).ok()?;
            let subscriber = tracing_subscriber::fmt()
                .with_writer(capture.clone())
                .with_ansi(false)
                .with_env_filter(filter)
                .finish();
            tracing::subscriber::set_global_default(subscriber)
                .ok()
                .map(|()| capture)
        })
        .clone()
        .ok_or_else(|| "another global tracing subscriber was already set".into())
}

fn event(seq: u64, event: Event) -> Result<ServerFrame, Box<dyn std::error::Error>> {
    Ok(ServerFrame::Event {
        seq: seq.into(),
        run_id: Some("run-keys".parse()?),
        step: Some(1),
        ts: "2026-01-01T00:00:00Z".into(),
        event,
    })
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_007_key_shaped_tool_output_stays_out_of_the_gateway_log(pool: PgPool) -> TestResult {
    let logs = capture_logs()?;
    let keys = keys();
    let printed = format!(
        "ANTHROPIC_API_KEY={}\nGITHUB_TOKEN={}\naws_secret_access_key = {}\nAuthorization: Bearer {}\n",
        keys[0], keys[1], keys[2], keys[3]
    );
    let echoed = format!(
        "{{\"error\":{{\"message\":\"Invalid request near '{}' and Authorization: Bearer {}\"}}}}",
        keys[0], keys[3]
    );
    let server = FakeModelServer::start(
        vec![Script::new(vec![Scripted::Status {
            code: 400,
            body: echoed.clone(),
            retry_after: None,
        }])],
        Arc::new(tqa_llm::TokioSleeper),
    )
    .await?;
    let registry = tqa_llm::Registry::from_toml(&format!(
        "[[routes]]\nname='local'\nprotocol='openai_chat'\nbase_url='{}'\nmodel='pinned-model'\nauth={{kind='bearer'}}\nkey_ref='secrets/local'",
        server.base_url()
    ))?;
    let result = support::run_configured(pool, Some(registry), "", async |f| {
        let (owner, _) = f.admin().await?;
        let conversation = f.conversation(owner.id).await?;
        f.gateway
            .secret_store()
            .put(
                &"secrets/local".parse()?,
                "synthetic-provider-key".to_owned().into(),
            )
            .await?;
        // The link: the call that read them and its result.
        let mut runtime = f.runtime(&conversation).await?;
        let call_id: tqa_types::ToolCallId = "call-keys".parse()?;
        runtime
            .send(&event(
                1,
                Event::RunStarted {
                    run_id: "run-keys".parse()?,
                },
            )?)
            .await?;
        runtime
            .send(&event(
                2,
                Event::ToolCall {
                    call_id: call_id.clone(),
                    name: "shell".into(),
                    arguments: json!({"command": format!("echo {}", keys[1])}),
                },
            )?)
            .await?;
        runtime
            .send(&event(
                3,
                Event::ToolResult {
                    call_id,
                    status: ToolStatus::Ok,
                    content: printed.clone(),
                    target: None,
                    evidence: None,
                },
            )?)
            .await?;
        f.until_sequence(&conversation.id, 3).await?;
        // The relay: the next model request carries the output; the provider refuses it and echoes the keys.
        let token = f.backend.token(&conversation.id).ok_or("runtime token")?;
        let response = f
            .client
            .post(f.url("/relay/r/local/chat/completions"))
            .bearer_auth(token.expose_secret())
            .json(&json!({"model":"m","stream":true,"messages":[
                {"role":"user","content":"read the notes"},
                {"role":"tool","tool_call_id":"call-keys","content": printed}]}))
            .send()
            .await?;
        assert_eq!(response.status(), 400);
        assert_eq!(
            response.text().await?,
            echoed,
            "the relay is transparent to the runtime (GW-I-039)"
        );
        let text = logs.text()?;
        assert!(
            !text.is_empty(),
            "the gateway logged nothing at its default filter"
        );
        for key in &keys {
            assert!(!text.contains(key), "the gateway log holds a key:\n{text}");
        }
        Ok(())
    })
    .await;
    server.shutdown().await?;
    result
}
