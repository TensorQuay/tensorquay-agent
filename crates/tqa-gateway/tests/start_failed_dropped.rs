//! GW-I-322: a message whose environment could not start is kept, never lost. The queued list shows it dropped with
//! the start's reason, open streams hear `message.queue` with it, so a page that reloads finds "Not sent, because the
//! environment couldn't start"; Send again starts the work once the environment can; Dismiss removes it (14 Nov 2026:
//! the 24 h run's lane and a person alike saw nothing after a first start that failed under an engine stall).
#[path = "support/fixture.rs"]
mod support;

use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::atomic::Ordering;
use support::{Fixture, TestResult};
use tqa_types::{ConversationId, ErrorCode, SandboxState, ServerFrame};

async fn listed(
    f: &Fixture,
    cookie: &str,
    id: &ConversationId,
) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let response = f
        .get(
            &format!("/api/v1/conversations/{id}/messages/queued"),
            cookie,
        )
        .await?;
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await?;
    Ok(body["items"].as_array().cloned().unwrap_or_default())
}

// TEST: GW-I-322
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_322_a_message_whose_environment_could_not_start_is_listed_as_dropped_with_its_reason(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let conversation = f.conversation(owner.id).await?;
        let id = &conversation.id;
        let path = format!("/api/v1/conversations/{id}/messages");
        f.backend.fail_create.store(true, Ordering::SeqCst);
        let mut stream = f.stream(id, &cookie, 0).await?;
        let accepted = f
            .action(&path, &cookie, Some("first"), &json!({"text": "Start the work."}))
            .await?;
        assert_eq!(accepted.status(), 202);
        let message = accepted.json::<Value>().await?["message_id"]
            .as_str()
            .ok_or("message_id")?
            .to_owned();
        // The open stream hears the retryable failure, then the queue with the message under dropped.
        let mut heard = Vec::new();
        while heard.len() < 4 {
            let (sequence, frame) = match stream.next().await {
                Ok(next) => next,
                Err(error) => return Err(format!("no queue frame after {heard:?}: {error}").into()),
            };
            assert_eq!(sequence, None, "{frame:?}");
            let queue = matches!(frame, ServerFrame::MessageQueue { .. });
            heard.push(frame);
            if queue {
                break;
            }
        }
        assert!(
            heard.iter().any(|frame| matches!(frame, ServerFrame::Error { code, retryable: true, .. } if *code == ErrorCode::SandboxFailed)),
            "{heard:?}"
        );
        assert!(
            heard.iter().any(|frame| matches!(frame, ServerFrame::MessageQueue { held, dropped, sent: None } if held.is_empty() && *dropped == [message.clone()])),
            "{heard:?}"
        );
        // The state notice came among them: the sandbox reads failed.
        assert!(
            heard.iter().any(|frame| matches!(frame, ServerFrame::SessionReady { sandbox, .. } if sandbox.state == SandboxState::Failed)),
            "{heard:?}"
        );
        // What a reload reads: the message, dropped, with the start's reason; nothing else of it is stored.
        let items = listed(f, &cookie, id).await?;
        assert_eq!(items.len(), 1, "{items:?}");
        assert_eq!(items[0]["id"], message);
        assert_eq!(items[0]["state"], "dropped");
        assert_eq!(items[0]["reason"], "sandbox_failed");
        assert_eq!(items[0]["text"], "Start the work.");
        assert_eq!(f.stream(id, &cookie, 0).await.map_err(|e| format!("later stream: {e}"))?.initial, SandboxState::Failed);
        // Dismissed by its author: gone from the list.
        let dismissed = f
            .client
            .delete(f.url(&format!("{path}/queued/{message}")))
            .header("origin", f.origin())
            .header("cookie", &cookie)
            .send()
            .await?;
        assert_eq!(dismissed.status(), 204);
        assert!(listed(f, &cookie, id).await?.is_empty());
        // Sent again once the environment can start: a new turn, and nothing dropped remains.
        f.backend.fail_create.store(false, Ordering::SeqCst);
        let again = f
            .action(&path, &cookie, Some("again"), &json!({"text": "Start the work."}))
            .await?;
        assert_eq!(again.status(), 202);
        let _runtime = f.runtime(&conversation).await.map_err(|e| format!("the retry's runtime: {e}"))?;
        assert_eq!(f.stream(id, &cookie, 0).await.map_err(|e| format!("stream after the retry: {e}"))?.initial, SandboxState::Ready);
        // The retry may still be on its way out (waiting_for_slot) when read; nothing dropped remains.
        let after = listed(f, &cookie, id).await?;
        assert!(
            after.iter().all(|item| item["state"] != "dropped"),
            "{after:?}"
        );
        Ok(())
    })
    .await
}
