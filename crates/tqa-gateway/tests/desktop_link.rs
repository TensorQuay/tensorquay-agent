//! ADR-0039 slice 2: the Desktop at the link.
//! - GW-I-223: a conversation whose environment declares `desktop` needs protocol revision 14: a runtime announcing 13
//!   is refused (426 `runtime_too_old`, `needs` 14), 14 links, and a task on another environment links from the floor.
#[path = "support/fixture.rs"]
mod support;
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult};
use tokio_tungstenite::tungstenite::{Error as WsError, client::IntoClientRequest};
use tqa_types::{ConversationId, PROTOCOL_REVISION_HEADER};
use uuid::Uuid;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

// TEST: GW-I-223
/// A conversation whose environment declares `desktop` needs revision 14 at the link (ADR-0039): 13 is refused with
/// `needs` 14, 14 links, and a task on another environment still links from the floor.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_223_a_desktop_environment_needs_revision_14_at_the_link(pool: PgPool) -> TestResult {
    Box::pin(support::run(pool, async |f| {
        sqlx::query("INSERT INTO templates (name,display_name,image,capabilities,is_default,version,last_set_by) VALUES ('desktop','Desktop',$1,'{shell,browser,desktop}',false,1,'admin')")
            .bind(format!("sha256:{}", "4".repeat(64)))
            .execute(&f.pool)
            .await?;
        let (person, _) = f.admin().await?;
        let desktop = f.conversation(person.id).await?;
        let other = f.conversation(person.id).await?;
        sqlx::query("UPDATE conversations SET template='desktop' WHERE id=$1")
            .bind(Uuid::parse_str(desktop.id.as_str())?)
            .execute(&f.pool)
            .await?;
        let (status, body) = dial(f, &desktop.id, tqa_types::DESKTOP_REVISION - 1).await?;
        assert_eq!(
            (status, &body["error"]["code"], &body["error"]["needs"]),
            (
                426,
                &json!("runtime_too_old"),
                &json!(tqa_types::DESKTOP_REVISION)
            ),
            "{body}"
        );
        let (status, body) = dial(f, &desktop.id, tqa_types::DESKTOP_REVISION).await?;
        assert_eq!(status, 101, "{body}");
        let (status, body) = dial(f, &other.id, tqa_types::MIN_RUNTIME_REVISION).await?;
        assert_eq!(status, 101, "{body}");
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
