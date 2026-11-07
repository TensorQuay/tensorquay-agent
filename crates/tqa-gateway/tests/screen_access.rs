//! ADR-0039's security review, T3 (someone else sees or drives a person's screen), at the gateway:
//! - SEC-005 (the screen's case): with the desktop up, another person, a person the task's workspace is shared with
//!   and an administrator are each refused the screen (`4403`), its notices, its screenshots and Sign out everywhere
//!   (`403`); none of it records anything, and the owner's own socket still opens.
//! - GW-I-220 (the bridge's binding): a dial without a runtime token, or with one that isn't a runtime's, is refused
//!   `401`; another desktop task's runtime token never reaches this task's viewer, whose own runtime then pairs.
#[path = "support/fixture.rs"]
mod support;

use futures::{SinkExt, StreamExt};
use reqwest::Method;
use secrecy::ExposeSecret;
use serde_json::json;
use sqlx::PgPool;
use support::{DEADLINE, Fixture, TestResult, call};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message, client::IntoClientRequest},
};
use tqa_types::{ClientFrame, ConversationId, ServerFrame};
use uuid::Uuid;

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// A Desktop template, and `conversation` bound to it.
async fn desktop(f: &Fixture, conversation: &ConversationId) -> TestResult {
    sqlx::query("INSERT INTO templates (name,display_name,image,capabilities,is_default,version,last_set_by) VALUES ('desktop','Desktop',$1,'{shell,browser,desktop}',false,1,'admin') ON CONFLICT (name) DO NOTHING")
        .bind(format!("sha256:{}", "4".repeat(64)))
        .execute(&f.pool)
        .await?;
    sqlx::query("UPDATE conversations SET template='desktop' WHERE id=$1")
        .bind(Uuid::parse_str(conversation.as_str())?)
        .execute(&f.pool)
        .await?;
    Ok(())
}

/// Opens the viewer socket as a page would, with `cookie`, from the gateway's own origin.
async fn viewer(f: &Fixture, id: &str, cookie: &str) -> Result<Socket> {
    let mut request = f
        .url(&format!("/api/v1/conversations/{id}/screen"))
        .replacen("http:", "ws:", 1)
        .into_client_request()?;
    request.headers_mut().insert("cookie", cookie.parse()?);
    request.headers_mut().insert("origin", f.origin().parse()?);
    Ok(tokio_tungstenite::connect_async(request).await?.0)
}

/// The socket's first frame past pings.
async fn first(socket: &mut Socket) -> Result<Message> {
    loop {
        let next = tokio::time::timeout(DEADLINE, socket.next())
            .await
            .map_err(|_| "no frame within the deadline: the socket stayed open")?;
        match next {
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
            Some(message) => return Ok(message?),
            None => return Err("the socket ended without a frame".into()),
        }
    }
}

/// The close code and reason the socket ends with at once.
async fn closed(socket: &mut Socket) -> Result<(u16, String)> {
    match first(socket).await? {
        Message::Close(Some(frame)) => Ok((u16::from(frame.code), frame.reason.to_string())),
        other => Err(format!("expected a close, got {other:?}").into()),
    }
}

/// Dials the bridge as a runtime does, with `token` as its bearer (none: no header).
async fn dial(f: &Fixture, token: Option<&str>) -> Result<Socket> {
    let mut request = f
        .url("/runtime/screen")
        .replacen("http:", "ws:", 1)
        .into_client_request()?;
    if let Some(token) = token {
        request
            .headers_mut()
            .insert("authorization", format!("Bearer {token}").parse()?);
    }
    Ok(tokio_tungstenite::connect_async(request).await?.0)
}

/// The task's runtime token, as its sandbox holds it.
fn token(f: &Fixture, id: &ConversationId) -> Result<String> {
    Ok(f.backend
        .token(id)
        .ok_or("a runtime token")?
        .expose_secret()
        .to_owned())
}

/// The number of screen notices stored for `id` (the owner's read).
async fn notice_count(f: &Fixture, id: &str, cookie: &str) -> Result<usize> {
    let (status, body) = call(
        f,
        Method::GET,
        &format!("/api/v1/conversations/{id}/screen/notices"),
        cookie,
        (None, None),
    )
    .await?;
    assert_eq!(status, 200, "{body}");
    Ok(body["items"].as_array().map_or(0, Vec::len))
}

// TEST: SEC-005
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_005_a_desktop_s_screen_and_its_records_are_its_owner_s_alone(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, admin) = f.admin().await?;
        let (owner, cookie) = f.member("owner@example.invalid").await?;
        let (_, holder) = f.member("holder@example.invalid").await?;
        let (_, other) = f.member("other@example.invalid").await?;
        let task = f.conversation(owner.id).await?;
        desktop(f, &task.id).await?;
        // The task's workspace shared with `holder`, who may write in it.
        let (status, body) = call(
            f,
            Method::PUT,
            &format!("/api/v1/workspaces/{}/shares", task.workspace_id),
            &cookie,
            (
                Some(json!({"email": "holder@example.invalid", "access": "write"})),
                Some(&Uuid::now_v7().to_string()),
            ),
        )
        .await?;
        assert!((200..300).contains(&status), "share: {status} {body}");
        // The desktop up, with a screenshot kept: every refusal below is the owner rule, not a missing sandbox.
        let mut runtime = f.runtime(&task).await?;
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.resize(4096, 7);
        let shot = {
            use sha2::Digest as _;
            use std::fmt::Write as _;
            sha2::Sha256::digest(&png)
                .iter()
                .fold(String::new(), |mut text, byte| {
                    let _ = write!(text, "{byte:02x}");
                    text
                })
        };
        runtime
            .send_live(&ServerFrame::ScreenUpload {
                id: shot.clone(),
                chunk: 0,
                last: true,
                data: {
                    use base64::Engine as _;
                    base64::engine::general_purpose::STANDARD.encode(&png)
                },
            })
            .await?;
        assert!(
            matches!(
                runtime.action().await?,
                ClientFrame::ScreenStored { ok: true, .. }
            ),
            "the screenshot is kept"
        );
        let id = task.id.as_str();
        let before = notice_count(f, id, &cookie).await?;
        for (who, intruder) in [
            ("another person", &other),
            ("the share holder", &holder),
            ("an administrator", &admin),
        ] {
            let mut socket = viewer(f, id, intruder).await?;
            let refusal = closed(&mut socket)
                .await
                .map_err(|error| format!("{who}: the screen: {error}"))?;
            assert_eq!(refusal, (4403, "forbidden".to_owned()), "{who}: the screen");
            let (status, _) = call(
                f,
                Method::GET,
                &format!("/api/v1/conversations/{id}/screen/notices"),
                intruder,
                (None, None),
            )
            .await?;
            assert_eq!(status, 403, "{who}: the notices");
            let (status, _) = call(
                f,
                Method::GET,
                &format!("/api/v1/conversations/{id}/screenshots/{shot}"),
                intruder,
                (None, None),
            )
            .await?;
            assert_eq!(status, 403, "{who}: a screenshot");
            let (status, _) = call(
                f,
                Method::POST,
                &format!("/api/v1/conversations/{id}/screen/sign-out"),
                intruder,
                (Some(json!({})), Some(&Uuid::now_v7().to_string())),
            )
            .await?;
            assert_eq!(status, 403, "{who}: Sign out everywhere");
        }
        // Nothing was recorded, and the runtime was never asked to sign out: the owner's sign-out is the first ask.
        assert_eq!(notice_count(f, id, &cookie).await?, before);
        // The owner's socket pairs with the task's own bridge and hears who holds the screen.
        let mut socket = viewer(f, id, &cookie).await?;
        let _bridge = tokio::time::timeout(DEADLINE, dial(f, Some(&token(f, &task.id)?))).await??;
        assert!(
            matches!(first(&mut socket).await?, Message::Text(_)),
            "the owner's control frame"
        );
        let (status, _) = call(
            f,
            Method::POST,
            &format!("/api/v1/conversations/{id}/screen/sign-out"),
            &cookie,
            (Some(json!({})), Some(&Uuid::now_v7().to_string())),
        )
        .await?;
        assert_eq!(status, 202);
        assert!(
            runtime.action().await? == ClientFrame::ScreenSignOut {},
            "the owner's sign-out is the first the runtime hears of"
        );
        // Answered, so the gateway records it and waits for nothing more.
        runtime
            .send(&ServerFrame::ScreenSignOutDone {
                ok: true,
                reason: None,
                ready: true,
            })
            .await?;
        tokio::time::timeout(DEADLINE, async {
            while notice_count(f, id, &cookie).await? == before {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            Ok::<_, Box<dyn std::error::Error>>(())
        })
        .await??;
        Ok(())
    })
    .await
}

// TEST: GW-I-220
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_220_a_dial_reaches_only_its_own_sandbox_s_viewers(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let mine = f.conversation(owner.id).await?;
        let theirs = f.conversation(owner.id).await?;
        desktop(f, &mine.id).await?;
        desktop(f, &theirs.id).await?;
        let _mine_runtime = f.runtime(&mine).await?;
        let _their_runtime = f.runtime(&theirs).await?;
        // No token, and a token no runtime holds: refused before the upgrade.
        for token in [None, Some("not-a-runtime-token")] {
            let refused = dial(f, token).await;
            assert!(
                refused
                    .as_ref()
                    .is_err_and(|error| error.to_string().contains("401")),
                "{token:?}: {:?}",
                refused.map(|_| ())
            );
        }
        // A viewer of one task waits for its bridge; the other task's runtime dials and is never paired with it.
        let mut page = viewer(f, mine.id.as_str(), &cookie).await?;
        let theirs_token = token(f, &theirs.id)?;
        let crossed = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            dial(f, Some(&theirs_token)),
        )
        .await;
        assert!(
            crossed.is_err(),
            "another sandbox's dial is held for its own viewers, not paired with this one"
        );
        // This task's own runtime pairs at once.
        let mine_token = token(f, &mine.id)?;
        let mut sandbox = tokio::time::timeout(DEADLINE, dial(f, Some(&mine_token))).await??;
        assert!(
            matches!(first(&mut page).await?, Message::Text(_)),
            "the control frame, once paired"
        );
        page.send(Message::Binary(b"RFB 003.008\n\x01\x00".to_vec().into()))
            .await?;
        match first(&mut sandbox).await? {
            Message::Binary(bytes) => assert_eq!(
                &bytes[..],
                b"RFB 003.008\n\x01\x01",
                "the page's handshake, shared"
            ),
            other => return Err(format!("expected the page's handshake, got {other:?}").into()),
        }
        Ok(())
    })
    .await
}
