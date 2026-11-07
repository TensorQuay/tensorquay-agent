//! ADR-0039 slice 5: the live Screen panel at the gateway, with a fake bridge standing in for the runtime's.
//! - GW-I-217: the viewer route's refusals as close codes, and the origin refusal before the upgrade.
//! - GW-I-218 (gateway half): a view-only viewer's input never reaches the bridge, the holder's paste does, and a
//!   message the filter can't frame closes the viewer.
//! - GW-I-219: Take over cancels the active run and holds queued turns; a second tab's Take over is refused, keeping its
//!   socket; Hand back and
//!   the controller's closed socket each end it with a notice, and the held turn then starts.
//! - GW-I-220: the bridge's dial: paired with a viewer that opens, `403` without a desktop.
#[path = "support/fixture.rs"]
mod support;

use futures::{SinkExt, StreamExt};
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{DEADLINE, Fixture, TestResult};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message, client::IntoClientRequest},
};
use tqa_types::{ClientFrame, ConversationId, Event, ServerFrame};
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

/// Opens the viewer socket as a page would, with `cookie` and `origin`.
async fn viewer(
    f: &Fixture,
    id: &str,
    cookie: &str,
    origin: &str,
) -> std::result::Result<Socket, tokio_tungstenite::tungstenite::Error> {
    let mut request = f
        .url(&format!("/api/v1/conversations/{id}/screen"))
        .replacen("http:", "ws:", 1)
        .into_client_request()?;
    request.headers_mut().insert(
        "cookie",
        cookie
            .parse()
            .map_err(|_| tokio_tungstenite::tungstenite::Error::ConnectionClosed)?,
    );
    request.headers_mut().insert(
        "origin",
        origin
            .parse()
            .map_err(|_| tokio_tungstenite::tungstenite::Error::ConnectionClosed)?,
    );
    Ok(tokio_tungstenite::connect_async(request).await?.0)
}

/// The next frame, past pings.
async fn next(socket: &mut Socket) -> Result<Message> {
    next_within(socket, DEADLINE).await
}

/// [`next`] with its own bound, for a frame due later than [`DEADLINE`] allows.
async fn next_within(socket: &mut Socket, wait: std::time::Duration) -> Result<Message> {
    tokio::time::timeout(wait, async {
        loop {
            match socket.next().await.ok_or("ended")?? {
                Message::Ping(_) | Message::Pong(_) => {}
                other => return Ok::<_, Box<dyn std::error::Error>>(other),
            }
        }
    })
    .await?
}

/// The close code and reason a refused viewer gets.
async fn closed(socket: &mut Socket) -> Result<(u16, String)> {
    match next(socket).await? {
        Message::Close(Some(frame)) => Ok((u16::from(frame.code), frame.reason.to_string())),
        other => Err(format!("expected a close, got {other:?}").into()),
    }
}

/// The control frame: who holds the screen.
async fn holder(socket: &mut Socket) -> Result<String> {
    match next(socket).await? {
        Message::Text(text) => {
            let value: Value = serde_json::from_str(&text)?;
            assert_eq!(value["type"], "control", "{value}");
            Ok(value["holder"].as_str().ok_or("holder")?.to_owned())
        }
        other => Err(format!("expected a control frame, got {other:?}").into()),
    }
}

/// The next control frame, whole.
async fn control(socket: &mut Socket) -> Result<Value> {
    control_within(socket, DEADLINE).await
}

/// [`control`] with its own bound.
async fn control_within(socket: &mut Socket, wait: std::time::Duration) -> Result<Value> {
    match next_within(socket, wait).await? {
        Message::Text(text) => {
            let value: Value = serde_json::from_str(&text)?;
            assert_eq!(value["type"], "control", "{value}");
            Ok(value)
        }
        other => Err(format!("expected a control frame, got {other:?}").into()),
    }
}

/// The cancelled run `r-1` ends (sequence 2), then the runtime's run-end reset is over.
async fn cancelled_and_reset(
    f: &Fixture,
    runtime: &mut support::runtime::Runtime,
    id: &ConversationId,
) -> TestResult {
    runtime
        .send(&ServerFrame::Event {
            seq: 2.into(),
            run_id: Some("r-1".parse()?),
            step: None,
            ts: "2026-10-05T12:00:01Z".into(),
            event: Event::RunCancelled {
                run_id: "r-1".parse()?,
            },
        })
        .await?;
    f.until_sequence(id, 2).await?;
    runtime
        .send(&ServerFrame::ScreenReset {
            ok: true,
            ready: true,
        })
        .await?;
    Ok(())
}

/// A task on the Desktop with run `r-1` active, a viewer with its bridge, and the viewer's Take over pressed.
async fn pressed_during_a_run(
    f: &Fixture,
) -> Result<(support::runtime::Runtime, ConversationId, Socket, Socket)> {
    let (owner, cookie) = f.admin().await?;
    let task = f.conversation(owner.id).await?;
    desktop(f, &task.id).await?;
    let mut runtime = f.runtime(&task).await?;
    runtime
        .send(&ServerFrame::Event {
            seq: 1.into(),
            run_id: Some("r-1".parse()?),
            step: None,
            ts: "2026-10-05T12:00:00Z".into(),
            event: Event::RunStarted {
                run_id: "r-1".parse()?,
            },
        })
        .await?;
    f.until_sequence(&task.id, 1).await?;
    let mut page = viewer(f, task.id.as_str(), &cookie, f.origin()).await?;
    let bridge = bridge(f, &task.id).await?;
    assert_eq!(holder(&mut page).await?, "none");
    page.send(Message::Text(r#"{"type":"take_over"}"#.into()))
        .await?;
    assert_eq!(
        runtime.action().await?,
        ClientFrame::RunCancel {
            run_id: "r-1".parse()?
        }
    );
    Ok((runtime, task.id, page, bridge))
}

/// Dials the bridge as the runtime does; the socket once paired.
async fn bridge(f: &Fixture, id: &ConversationId) -> Result<Socket> {
    let token = f.backend.token(id).ok_or("a runtime token")?;
    let mut request = f
        .url("/runtime/screen")
        .replacen("http:", "ws:", 1)
        .into_client_request()?;
    request.headers_mut().insert(
        "authorization",
        format!("Bearer {}", token.expose_secret()).parse()?,
    );
    Ok(
        tokio::time::timeout(DEADLINE * 4, tokio_tungstenite::connect_async(request))
            .await??
            .0,
    )
}

/// Through the RFB 3.8 handshake on both sides of the relay.
async fn handshake(page: &mut Socket, sandbox: &mut Socket) -> TestResult {
    page.send(Message::Binary(b"RFB 003.008\n\x01\x00".to_vec().into()))
        .await?;
    assert_eq!(
        binary(sandbox).await?,
        b"RFB 003.008\n\x01\x01",
        "passed, the session forced shared"
    );
    Ok(())
}

async fn binary(socket: &mut Socket) -> Result<Vec<u8>> {
    match next(socket).await? {
        Message::Binary(bytes) => Ok(bytes.to_vec()),
        other => Err(format!("expected bytes, got {other:?}").into()),
    }
}

const KEY: [u8; 8] = [4, 1, 0, 0, 0, 0, 0, 0x61];
const PASTE: [u8; 10] = [6, 0, 0, 0, 0, 0, 0, 2, b'h', b'i'];
const UPDATE: [u8; 10] = [3, 1, 0, 0, 0, 0, 5, 0, 3, 32];

// TEST: GW-I-217
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_217_the_viewer_route_refuses_with_close_codes(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let (_, other) = f.member("other@example.invalid").await?;
        let origin = f.origin();
        let plain = f.conversation(owner.id).await?;
        let task = f.conversation(owner.id).await?;
        desktop(f, &task.id).await?;
        for (id, cookie, code) in [
            (task.id.to_string(), other.as_str(), (4403, "forbidden")),
            (
                Uuid::now_v7().to_string(),
                cookie.as_str(),
                (4404, "not_found"),
            ),
            (plain.id.to_string(), cookie.as_str(), (4409, "no_desktop")),
            (
                task.id.to_string(),
                cookie.as_str(),
                (4409, "sandbox_not_running"),
            ),
        ] {
            let mut socket = viewer(f, &id, cookie, origin).await?;
            let (number, reason) = closed(&mut socket).await?;
            assert_eq!((number, reason.as_str()), code, "{id}");
        }
        // A wrong Origin is refused before the upgrade.
        assert!(
            viewer(f, task.id.as_str(), &cookie, "https://evil.example.invalid")
                .await
                .is_err()
        );
        // With the sandbox up: the owner opens and hears who holds the screen; a fifth viewer is refused.
        let _runtime = f.runtime(&task).await?;
        let mut open = Vec::new();
        for _ in 0..4 {
            let mut socket = viewer(f, task.id.as_str(), &cookie, origin).await?;
            let mut sandbox = bridge(f, &task.id).await?;
            assert_eq!(holder(&mut socket).await?, "none");
            handshake(&mut socket, &mut sandbox).await?;
            open.push((socket, sandbox));
        }
        let mut fifth = viewer(f, task.id.as_str(), &cookie, origin).await?;
        assert_eq!(
            closed(&mut fifth).await?,
            (4409, "screen_viewers".to_owned())
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-218
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_218_only_the_holder_s_input_reaches_the_sandbox(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let origin = f.origin();
        let task = f.conversation(owner.id).await?;
        desktop(f, &task.id).await?;
        let _runtime = f.runtime(&task).await?;
        let mut page = viewer(f, task.id.as_str(), &cookie, origin).await?;
        let mut sandbox = bridge(f, &task.id).await?;
        assert_eq!(holder(&mut page).await?, "none");
        handshake(&mut page, &mut sandbox).await?;
        // View-only: the key and the paste are dropped; the update request after them passes, alone.
        let mut sent = KEY.to_vec();
        sent.extend(PASTE);
        sent.extend(UPDATE);
        page.send(Message::Binary(sent.into())).await?;
        assert_eq!(binary(&mut sandbox).await?, UPDATE);
        // Holding the screen, the paste passes.
        page.send(Message::Text(r#"{"type":"take_over"}"#.into()))
            .await?;
        assert_eq!(holder(&mut page).await?, "you");
        page.send(Message::Binary(PASTE.to_vec().into())).await?;
        assert_eq!(binary(&mut sandbox).await?, PASTE);
        // A message the filter doesn't know closes the viewer.
        page.send(Message::Binary(vec![150, 0, 0, 0].into()))
            .await?;
        assert_eq!(closed(&mut page).await?.0, 1008);
        Ok(())
    })
    .await
}

// TEST: GW-I-219
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_219_take_over_cancels_the_run_and_holds_turns_until_the_hand_back(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let origin = f.origin();
        let task = f.conversation(owner.id).await?;
        desktop(f, &task.id).await?;
        let mut runtime = f.runtime(&task).await?;
        runtime
            .send(&ServerFrame::Event {
                seq: 1.into(),
                run_id: Some("r-1".parse()?),
                step: None,
                ts: "2026-10-05T12:00:00Z".into(),
                event: Event::RunStarted {
                    run_id: "r-1".parse()?,
                },
            })
            .await?;
        f.until_sequence(&task.id, 1).await?;
        let mut first = viewer(f, task.id.as_str(), &cookie, origin).await?;
        let _first_bridge = bridge(f, &task.id).await?;
        let mut second = viewer(f, task.id.as_str(), &cookie, origin).await?;
        let _second_bridge = bridge(f, &task.id).await?;
        assert_eq!(holder(&mut first).await?, "none");
        assert_eq!(holder(&mut second).await?, "none");
        first
            .send(Message::Text(r#"{"type":"take_over"}"#.into()))
            .await?;
        // The active run is cancelled as Stop does.
        assert_eq!(
            runtime.action().await?,
            ClientFrame::RunCancel {
                run_id: "r-1".parse()?
            }
        );
        cancelled_and_reset(f, &mut runtime, &task.id).await?;
        // Control comes once the run's end reset is over (GW-I-230 has the wait itself).
        assert_eq!(holder(&mut first).await?, "you");
        assert_eq!(holder(&mut second).await?, "other");
        // A second tab's Take over is refused: it keeps its picture and is told again that another holds the screen.
        second
            .send(Message::Text(r#"{"type":"take_over"}"#.into()))
            .await?;
        assert_eq!(holder(&mut second).await?, "other");
        // A message sent meanwhile waits: no turn starts while the screen is taken over.
        let accepted = f
            .action(
                &format!("/api/v1/conversations/{}/messages", task.id),
                &cookie,
                Some("held"),
                &json!({"text": "I've signed in; carry on"}),
            )
            .await?;
        assert_eq!(accepted.status(), 202);
        // And it says so: held, as a message behind a run is (GW-I-129), so the page lists it as waiting for the
        // hand-back rather than sending (the 16d page pass, Z15: it read "Sending…" until the hand-back).
        let body: Value = accepted.json().await?;
        assert_eq!(body["held"], json!(true), "{body}");
        assert!(
            runtime
                .try_action(std::time::Duration::from_millis(500))
                .await?
                .is_none()
        );
        // The controller's socket closes: Take over ends with a notice, and the held turn starts.
        drop(first);
        match runtime.action().await? {
            ClientFrame::UserMessage { text, .. } => assert_eq!(text, "I've signed in; carry on"),
            other => return Err(format!("expected the held turn, got {other:?}").into()),
        }
        let (_, notices) = support::call(
            f,
            reqwest::Method::GET,
            &format!("/api/v1/conversations/{}/screen/notices", task.id),
            &cookie,
            (None, None),
        )
        .await?;
        let kinds: Vec<_> = notices["items"]
            .as_array()
            .ok_or("items")?
            .iter()
            .map(|item| (item["kind"].clone(), item["ended"].clone()))
            .collect();
        assert_eq!(
            kinds,
            [
                (json!("screen.taken_over"), Value::Null),
                (json!("screen.handed_back"), json!("socket_closed"))
            ]
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-220
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_220_a_dial_pairs_with_a_viewer_and_is_refused_without_a_desktop(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let origin = f.origin();
        let plain = f.conversation(owner.id).await?;
        let _plain_runtime = f.runtime(&plain).await?;
        match bridge(f, &plain.id).await {
            Err(error) => assert!(error.to_string().contains("403"), "{error}"),
            Ok(_) => return Err("a dial without a desktop was upgraded".into()),
        }
        let task = f.conversation(owner.id).await?;
        desktop(f, &task.id).await?;
        let _runtime = f.runtime(&task).await?;
        // The dial is held until a viewer opens, then paired with it.
        let held = tokio::spawn({
            let url = f.url("/runtime/screen").replacen("http:", "ws:", 1);
            let token = f.backend.token(&task.id).ok_or("a runtime token")?;
            async move {
                let mut request = url.into_client_request()?;
                request.headers_mut().insert(
                    "authorization",
                    format!("Bearer {}", token.expose_secret()).parse()?,
                );
                let socket = tokio_tungstenite::connect_async(request).await?.0;
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(socket)
            }
        });
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let mut page = viewer(f, task.id.as_str(), &cookie, origin).await?;
        let mut sandbox = tokio::time::timeout(DEADLINE, held)
            .await??
            .map_err(|error| error.to_string())?;
        assert_eq!(holder(&mut page).await?, "none");
        handshake(&mut page, &mut sandbox).await?;
        // What the sandbox sends reaches the page as it is.
        sandbox
            .send(Message::Binary(b"RFB 003.008\n".to_vec().into()))
            .await?;
        assert_eq!(binary(&mut page).await?, b"RFB 003.008\n");
        Ok(())
    })
    .await
}

// TEST: GW-I-274
/// Just after a restart, the runtime's link is down while its sandbox lives (the runtime redials within a few seconds).
/// A viewer that opens meanwhile waits for the bridge, not told the desktop isn't running, which the page never retries
/// by itself (WEB-E2E-073's flake); and a bridge that drops before the link is back is `screen_unavailable`, retried.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_274_a_viewer_waits_for_a_live_sandbox_whose_link_is_down(pool: PgPool) -> TestResult {
    support::run_restarting(pool, async |f| {
        let (owner, _) = f.admin().await?;
        let task = f.conversation(owner.id).await?;
        desktop(f, &task.id).await?;
        drop(f.runtime(&task).await?);
        f.restart().await?;
        let (_, cookie) = f.login("admin@example.invalid").await?;
        let mut page = viewer(f, task.id.as_str(), &cookie, f.origin()).await?;
        // Open, it waits: nothing comes until the bridge does (before the fix, `4409 sandbox_not_running` at once).
        if let Ok(early) = next_within(&mut page, std::time::Duration::from_millis(500)).await {
            return Err(format!("the viewer was answered before any bridge: {early:?}").into());
        }
        let mut sandbox = bridge(f, &task.id).await?;
        assert_eq!(holder(&mut page).await?, "none");
        handshake(&mut page, &mut sandbox).await?;
        drop(sandbox);
        assert_eq!(
            closed(&mut page).await?,
            (4409, "screen_unavailable".to_owned())
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-230
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_230_control_waits_for_the_cancelled_run_s_end_reset(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (mut runtime, id, mut page, _bridge) = pressed_during_a_run(f).await?;
        // Not before the run's end, and not at its end either: its reset could still clear a sign-in.
        let early = std::time::Duration::from_millis(700);
        assert!(
            tokio::time::timeout(early, next(&mut page)).await.is_err(),
            "no control before the run ends"
        );
        runtime
            .send(&ServerFrame::Event {
                seq: 2.into(),
                run_id: Some("r-1".parse()?),
                step: None,
                ts: "2026-10-05T12:00:01Z".into(),
                event: Event::RunCancelled {
                    run_id: "r-1".parse()?,
                },
            })
            .await?;
        f.until_sequence(&id, 2).await?;
        assert!(
            tokio::time::timeout(early, next(&mut page)).await.is_err(),
            "nor before its reset is over"
        );
        runtime
            .send(&ServerFrame::ScreenReset {
                ok: true,
                ready: true,
            })
            .await?;
        assert_eq!(
            control(&mut page).await?,
            json!({"type": "control", "holder": "you"})
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-230
// TEST: GW-U-155
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_230_with_no_reset_control_comes_10_s_after_the_press_marked_not_ready(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let pressed = std::time::Instant::now();
        let (mut runtime, id, mut page, _bridge) = pressed_during_a_run(f).await?;
        // The runtime never answers: control comes anyway at the bound, saying the reset wasn't confirmed. It is due
        // 10 s after the press, as long as DEADLINE, so the wait has its own longer bound.
        let frame = control_within(&mut page, std::time::Duration::from_secs(15)).await?;
        assert_eq!(
            frame,
            json!({"type": "control", "holder": "you", "ready": false})
        );
        let waited = pressed.elapsed();
        assert!(
            waited >= std::time::Duration::from_secs(9)
                && waited < std::time::Duration::from_secs(13),
            "{waited:?}"
        );
        // The reset lands after all (the CU review's #20): the late grant is settled, and the holder is told so with
        // plain control, which the page reads as ready.
        runtime
            .send(&ServerFrame::Event {
                seq: 2.into(),
                run_id: Some("r-1".parse()?),
                step: None,
                ts: "2026-10-05T12:00:11Z".into(),
                event: Event::RunCancelled {
                    run_id: "r-1".parse()?,
                },
            })
            .await?;
        f.until_sequence(&id, 2).await?;
        runtime
            .send(&ServerFrame::ScreenReset {
                ok: true,
                ready: true,
            })
            .await?;
        assert_eq!(
            control(&mut page).await?,
            json!({"type": "control", "holder": "you"})
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-230
// TEST: GW-U-156
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_230_a_reset_that_couldn_t_empty_the_profile_is_said_on_control(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (mut runtime, id, mut page, _bridge) = pressed_during_a_run(f).await?;
        runtime
            .send(&ServerFrame::Event {
                seq: 2.into(),
                run_id: Some("r-1".parse()?),
                step: None,
                ts: "2026-10-05T12:00:01Z".into(),
                event: Event::RunCancelled {
                    run_id: "r-1".parse()?,
                },
            })
            .await?;
        f.until_sequence(&id, 2).await?;
        runtime
            .send(&ServerFrame::ScreenReset {
                ok: false,
                ready: true,
            })
            .await?;
        assert_eq!(
            control(&mut page).await?,
            json!({"type": "control", "holder": "you", "reset_ok": false})
        );
        Ok(())
    })
    .await
}

// TEST: GW-U-156
/// The unclean mark is the browser profile's, and the profile lives in the sandbox: once the task's sandbox is
/// replaced (here after a loss) by a new one, Take over warns of nothing (the takeover pair's review, P3).
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_u_156_a_new_sandbox_starts_without_the_unclean_warning(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (mut runtime, id, mut page, first_bridge) = pressed_during_a_run(f).await?;
        runtime
            .send(&ServerFrame::Event {
                seq: 2.into(),
                run_id: Some("r-1".parse()?),
                step: None,
                ts: "2026-10-05T12:00:01Z".into(),
                event: Event::RunCancelled {
                    run_id: "r-1".parse()?,
                },
            })
            .await?;
        f.until_sequence(&id, 2).await?;
        runtime
            .send(&ServerFrame::ScreenReset {
                ok: false,
                ready: true,
            })
            .await?;
        assert_eq!(
            control(&mut page).await?,
            json!({"type": "control", "holder": "you", "reset_ok": false})
        );
        drop((page, first_bridge));
        // The sandbox is lost: recovery makes a new one for the task, through the same create path as any start.
        // Once the monitor is watching (as SBX-D-003 does), so it sees the loss at once.
        f.clock.observed().await?;
        let original = tqa_sandbox::SandboxBackend::list(f.backend.as_ref())
            .await?
            .pop()
            .ok_or("sandbox missing")?;
        f.backend.lose(&original).await?;
        drop(runtime);
        tokio::time::timeout(DEADLINE, async {
            while f.backend.creates.load(std::sync::atomic::Ordering::SeqCst) < 2 {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        f.forget_runtime(&id)?;
        let _runtime = f.runtime_for(&id).await?;
        // The owner, signed in again (`pressed_during_a_run` made it as the bootstrap admin).
        let (_, cookie) = f.login("admin@example.invalid").await?;
        let mut page = viewer(f, id.as_str(), &cookie, f.origin()).await?;
        let _bridge = bridge(f, &id).await?;
        assert_eq!(holder(&mut page).await?, "none");
        page.send(Message::Text(r#"{"type":"take_over"}"#.into()))
            .await?;
        assert_eq!(
            control(&mut page).await?,
            json!({"type": "control", "holder": "you"}),
            "the new sandbox's browser has no earlier sign-ins"
        );
        Ok(())
    })
    .await
}

// TEST: GW-U-156
/// A deleted task keeps no unclean mark: the delete's in-memory cleanup forgets it with the task's link and streams
/// (the takeover P3 review's gap: nothing tested that call).
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_u_156_a_deleted_task_keeps_no_unclean_mark(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (mut runtime, id, mut page, _bridge) = pressed_during_a_run(f).await?;
        runtime
            .send(&ServerFrame::Event {
                seq: 2.into(),
                run_id: Some("r-1".parse()?),
                step: None,
                ts: "2026-10-05T12:00:01Z".into(),
                event: Event::RunCancelled {
                    run_id: "r-1".parse()?,
                },
            })
            .await?;
        f.until_sequence(&id, 2).await?;
        runtime
            .send(&ServerFrame::ScreenReset {
                ok: false,
                ready: true,
            })
            .await?;
        assert_eq!(
            control(&mut page).await?,
            json!({"type": "control", "holder": "you", "reset_ok": false})
        );
        assert!(
            tqa_gateway::Gateway::screen_unclean(&id),
            "marked by the failed reset"
        );
        // The owner, signed in again (`pressed_during_a_run` made it as the bootstrap admin), deletes the task.
        let (_, cookie) = f.login("admin@example.invalid").await?;
        let deleted = f
            .client
            .delete(f.url(&format!("/api/v1/conversations/{id}")))
            .header("origin", f.origin())
            .header("cookie", &cookie)
            .send()
            .await?;
        assert_eq!(deleted.status().as_u16(), 204);
        assert!(
            !tqa_gateway::Gateway::screen_unclean(&id),
            "nothing kept for a deleted task"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-313
/// A reset whose browser didn't come back in time (16r's `sec_059_a`: under load the person got plain control over a
/// desktop whose browser wasn't there, typed the site's address into nothing, and no sign-in was made): the Take over
/// that waited for it is granted control that says a sign-in now may not last, never plain control.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_313_a_reset_without_a_ready_browser_grants_control_marked_not_ready(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (mut runtime, id, mut page, _bridge) = pressed_during_a_run(f).await?;
        runtime
            .send(&ServerFrame::Event {
                seq: 2.into(),
                run_id: Some("r-1".parse()?),
                step: None,
                ts: "2026-10-05T12:00:01Z".into(),
                event: Event::RunCancelled {
                    run_id: "r-1".parse()?,
                },
            })
            .await?;
        f.until_sequence(&id, 2).await?;
        runtime
            .send(&ServerFrame::ScreenReset {
                ok: true,
                ready: false,
            })
            .await?;
        assert_eq!(
            control(&mut page).await?,
            json!({"type": "control", "holder": "you", "ready": false})
        );
        Ok(())
    })
    .await
}
