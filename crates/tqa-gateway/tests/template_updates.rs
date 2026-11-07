//! GW-I-087 (ADR-0032 decision 7): a template's new image reaches the idle conversations bound to it at their next
//! sandbox, as an upgrade of the built-in templates needs; a run in progress keeps its image; a template that is not
//! offered keeps the old pin; and a pin the runtime floor refuses tells the owner why.
#[path = "support/fixture.rs"]
mod support;

use reqwest::Method;
use serde_json::json;
use sqlx::PgPool;
use support::{Fixture, TestResult, call};
use tqa_types::ConversationId;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const OLD: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const NEW: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";

fn templates(workstation: &str, extra: &str) -> String {
    format!(
        "[[templates]]\nname='workstation'\ndisplay_name='W'\nimage='{workstation}'\ncapabilities=['shell']\ndefault=true\n\n\
         [[templates]]\nname='extra'\ndisplay_name='E'\nimage='{extra}'\ncapabilities=['shell']\n"
    )
}

/// The image the conversation is bound to.
async fn pinned(f: &Fixture, id: &ConversationId) -> Result<String> {
    Ok(
        sqlx::query_scalar("SELECT template_digest FROM conversations WHERE id=$1")
            .bind(uuid::Uuid::parse_str(id.as_str())?)
            .fetch_one(&f.pool)
            .await?,
    )
}

/// The image the conversation's latest sandbox was created with.
fn created(f: &Fixture, id: &ConversationId) -> Result<String> {
    Ok(f.backend.created(id).ok_or("no sandbox was created")?.0)
}

fn started(sequence: u64, run: &str) -> Result<tqa_types::ServerFrame> {
    Ok(tqa_types::ServerFrame::Event {
        seq: sequence.into(),
        run_id: Some(run.parse()?),
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event: tqa_types::Event::RunStarted {
            run_id: run.parse()?,
        },
    })
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_087_an_upgrade_moves_idle_conversations_and_keeps_runs_in_progress(
    pool: PgPool,
) -> TestResult {
    Box::pin(support::run_restarting_with(
        pool,
        None,
        &templates(OLD, OLD),
        async |f| {
            let (owner, cookie) = f.admin().await?;
            let idle = f.conversation(owner.id).await?;
            let busy = f.conversation(owner.id).await?;
            assert_eq!(
                (pinned(f, &idle.id).await?, pinned(f, &busy.id).await?),
                (OLD.to_owned(), OLD.to_owned())
            );
            // A run starts on the old image; its sandbox is then lost mid-run.
            let handle = f.gateway.sandboxes().ensure(&busy.id).await?;
            let mut runtime = f.runtime(&busy).await?;
            runtime.send(&started(1, "r-1")?).await?;
            f.until_sequence(&busy.id, 1).await?;
            drop(runtime);
            f.backend.lose(&handle).await?;
            // The upgrade: the file pins the built-in template's new image. No one switches anything.
            f.reconfigure(&templates(NEW, OLD))?;
            f.restart().await?;
            f.gateway.sandboxes().ensure(&idle.id).await?;
            assert_eq!(
                (created(f, &idle.id)?, pinned(f, &idle.id).await?),
                (NEW.to_owned(), NEW.to_owned())
            );
            // Its session carries the template version it moved to, so the runtime announces the update (RT-I-029).
            let version: i64 =
                sqlx::query_scalar("SELECT version FROM templates WHERE name='workstation'")
                    .fetch_one(&f.pool)
                    .await?;
            // Kept connected, as a live session is, so the conversation's worker is there to act on the switch below.
            let moved = f.runtime(&idle).await?;
            assert_eq!(moved.config["environment"]["version"], json!(version));
            // The run in progress resumes on a new sandbox (the restart found the old one lost), made from the image it
            // started with, and has no version to announce.
            let replaced = f.gateway.sandboxes().ensure(&busy.id).await?;
            assert_ne!(replaced.id, handle.id, "the busy task got no new sandbox");
            assert_eq!(
                (created(f, &busy.id)?, pinned(f, &busy.id).await?),
                (OLD.to_owned(), OLD.to_owned())
            );
            // Its sandbox was replaced after the loss, so the token cached before it is stale.
            f.forget_runtime(&busy.id)?;
            assert!(
                f.runtime(&busy).await?.config["environment"]
                    .get("version")
                    .is_none()
            );
            // A switch to another template leaves the old template's version behind.
            let path = format!("/api/v1/conversations/{}/environment", idle.id);
            let (status, _) = call(
                f,
                Method::PUT,
                &path,
                &cookie,
                (Some(json!({"template": "extra"})), Some("switch")),
            )
            .await?;
            assert_eq!(status, 200);
            let version: Option<i64> =
                sqlx::query_scalar("SELECT template_version FROM conversations WHERE id=$1")
                    .bind(uuid::Uuid::parse_str(idle.id.as_str())?)
                    .fetch_one(&f.pool)
                    .await?;
            assert_eq!(version, None);
            // The switch retired the sandbox, so the next session is a new one: it carries no version either. The
            // retirement runs in the background, and admission refuses a new start until it has released the record.
            f.until_released(&idle.id).await?;
            drop(moved);
            f.forget_runtime(&idle.id)?;
            let config = f.runtime(&idle).await?.config.clone();
            assert_eq!(config["environment"]["template"], json!("extra"));
            assert!(config["environment"].get("version").is_none(), "{config}");
            Ok(())
        },
    ))
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_087_a_template_that_is_not_offered_keeps_the_pin(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, &templates(OLD, OLD), async |f| {
        let (_, cookie) = f.admin().await?;
        let created_body = json!({"title": "Extra", "model_route": "local", "template": "extra"});
        let response = f
            .client
            .post(f.url("/api/v1/conversations"))
            .header("origin", f.origin())
            .header("cookie", &cookie)
            .header("idempotency-key", "extra")
            .json(&created_body)
            .send()
            .await?;
        assert_eq!(response.status(), 201);
        let id: ConversationId = response.json::<serde_json::Value>().await?["id"]
            .as_str()
            .ok_or("id")?
            .parse()?;
        // An administrator disables the template while replacing its image: it is not offered.
        let replaced = json!({"display_name": "E", "image": NEW, "capabilities": ["shell"], "enabled": false, "version": 1});
        let (status, _) = call(f, Method::PUT, "/api/v1/admin/templates/extra", &cookie, (Some(replaced), None)).await?;
        assert_eq!(status, 200);
        f.gateway.sandboxes().ensure(&id).await?;
        assert_eq!((created(f, &id)?, pinned(f, &id).await?), (OLD.to_owned(), OLD.to_owned()));
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_087_a_pin_below_the_runtime_floor_tells_the_owner_why(pool: PgPool) -> TestResult {
    use secrecy::ExposeSecret;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let conversation = f.conversation(owner.id).await?;
        let _runtime = f.runtime(&conversation).await?;
        let mut stream = f.stream(&conversation.id, &cookie, 0).await?;
        // The sandbox's runtime is older than the floor: the link refuses it.
        let token = f
            .backend
            .token(&conversation.id)
            .ok_or("no runtime token")?;
        let mut request = f
            .url("/runtime/link")
            .replacen("http:", "ws:", 1)
            .into_client_request()?;
        let headers = request.headers_mut();
        headers.insert(
            "authorization",
            format!("Bearer {}", token.expose_secret()).parse()?,
        );
        let older = (tqa_types::MIN_RUNTIME_REVISION - 1).to_string();
        headers.insert(tqa_types::PROTOCOL_REVISION_HEADER, older.parse()?);
        assert!(
            tokio_tungstenite::connect_async(request).await.is_err(),
            "refused"
        );
        // The owner is told why, before the sandbox shows as failed.
        for _ in 0..16 {
            match stream.next().await?.1 {
                tqa_types::ServerFrame::Error {
                    code,
                    message,
                    retryable,
                    ..
                } => {
                    assert_eq!(
                        (code, retryable),
                        (tqa_types::ErrorCode::UnsupportedProtocol, false)
                    );
                    assert!(message.contains("switch the environment"), "{message}");
                    return Ok(());
                }
                tqa_types::ServerFrame::SessionReady { sandbox, .. }
                    if sandbox.state == tqa_types::SandboxState::Failed =>
                {
                    return Err("failed without a reason".into());
                }
                _ => {}
            }
        }
        Err("no reason reached the owner".into())
    })
    .await
}
