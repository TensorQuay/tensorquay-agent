//! GW-I-050: the runtime link refuses a runtime below the protocol floor before any frame (ADR-0026).

#[path = "support/fixture.rs"]
mod support;
use secrecy::ExposeSecret;
use sqlx::PgPool;
use support::{TestResult, run};
use tokio_tungstenite::tungstenite::{Error as WsError, client::IntoClientRequest};
use tqa_types::{MIN_RUNTIME_REVISION, PROTOCOL_REVISION_HEADER};

/// The link upgrade's HTTP status and body for one revision header value (`None`: no header).
async fn upgrade(
    fixture: &support::Fixture,
    token: &secrecy::SecretString,
    revision: Option<&str>,
) -> Result<(u16, serde_json::Value), Box<dyn std::error::Error>> {
    let mut request = fixture
        .url("/runtime/link")
        .replacen("http:", "ws:", 1)
        .into_client_request()?;
    let headers = request.headers_mut();
    headers.insert(
        "authorization",
        format!("Bearer {}", token.expose_secret()).parse()?,
    );
    if let Some(revision) = revision {
        headers.insert(PROTOCOL_REVISION_HEADER, revision.parse()?);
    }
    match tokio::time::timeout(support::DEADLINE, tokio_tungstenite::connect_async(request)).await?
    {
        Ok((socket, response)) => {
            drop(socket);
            Ok((response.status().as_u16(), serde_json::Value::Null))
        }
        Err(WsError::Http(response)) => {
            let body = response
                .body()
                .as_deref()
                .map(serde_json::from_slice)
                .transpose()?
                .unwrap_or(serde_json::Value::Null);
            Ok((response.status().as_u16(), body))
        }
        Err(error) => Err(error.into()),
    }
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_050_a_runtime_below_the_floor_is_refused_before_any_frame(
    pool: PgPool,
) -> TestResult {
    run(pool, async |fixture| {
        let (owner, _) = fixture.admin().await?;
        let conversation = fixture.conversation(owner.id).await?;
        let peer = std::net::Ipv4Addr::LOCALHOST.into();
        let token = fixture
            .gateway
            .runtime_tokens()
            .mint(&conversation.id, &"old-runtime".parse()?, peer)
            .await?;
        let older = (MIN_RUNTIME_REVISION - 1).to_string();
        for revision in [Some(older.as_str()), None] {
            let (status, body) = upgrade(fixture, &token, revision).await?;
            assert_eq!(status, 426, "{revision:?}");
            assert_eq!(body["error"]["code"], "runtime_too_old", "{body}");
            assert_eq!(body["error"]["retryable"], false);
            assert_eq!(body["error"]["needs"], MIN_RUNTIME_REVISION);
            assert_eq!(
                body["error"]["has"],
                revision.map_or(1, |value| value.parse::<u16>().unwrap_or(0))
            );
        }
        for malformed in ["06", "six", "0", "65536"] {
            let (status, body) = upgrade(fixture, &token, Some(malformed)).await?;
            assert_eq!(status, 400, "{malformed}: {body}");
        }
        let id = uuid::Uuid::parse_str(conversation.id.as_str())?;
        let events: i64 =
            sqlx::query_scalar("SELECT count(*) FROM events WHERE conversation_id=$1")
                .bind(id)
                .fetch_one(&fixture.pool)
                .await?;
        assert_eq!(events, 0, "no frame reached the journal");
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_050_a_refused_runtime_retires_its_sandbox_without_a_replacement(
    pool: PgPool,
) -> TestResult {
    use std::sync::atomic::Ordering;
    run(pool, async |fixture| {
        let (owner, _) = fixture.admin().await?;
        let conversation = fixture.conversation(owner.id).await?;
        // A current runtime links (the fixture states the current revision), so a sandbox is live.
        let runtime = fixture.runtime(&conversation).await?;
        assert_eq!(fixture.backend.creates.load(Ordering::SeqCst), 1);
        let token = fixture
            .backend
            .token(&conversation.id)
            .ok_or("no runtime token")?;
        let older = (MIN_RUNTIME_REVISION - 1).to_string();
        let (status, _) = upgrade(fixture, &token, Some(&older)).await?;
        assert_eq!(status, 426);
        tokio::time::timeout(support::DEADLINE, async {
            while fixture.backend.destroys.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        drop(runtime);
        // The destroy is the worker's last retirement step, and this path never schedules a new sandbox, so no
        // replacement from the same image exists. (Every fixture runtime link covers the accepted revision.)
        assert_eq!(fixture.backend.creates.load(Ordering::SeqCst), 1);
        Ok(())
    })
    .await
}
