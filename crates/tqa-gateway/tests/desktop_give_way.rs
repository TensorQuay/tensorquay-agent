//! GW-I-305 (ADR-0039 section 10, amended 17 Oct 2026; c16k real-world A4 and A6: desktop tasks waited 3 min 20 s and
//! 9.5 min for desktops whose tasks had finished): with `desktop.max_sessions` reached, an idle desktop gives way to a
//! waiting desktop task, but only while its browser's profile is known empty (a clean run-end reset, no Take over
//! since), and never while a run is active or it is pinned. A desktop whose reset this gateway hasn't seen keeps its
//! place (GW-I-224). GW-I-308: the same guard holds when a non-desktop task waits for a slot.
#[path = "support/fixture.rs"]
mod support;

use std::sync::atomic::Ordering;

use futures::SinkExt;
use serde_json::json;
use sqlx::PgPool;
use support::{DEADLINE, Fixture, TestResult};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};
use tqa_types::{ConversationId, Event, ServerFrame};
use uuid::Uuid;

const CONFIG: &str = "[admission]\nslots=4\nper_user=4\n[desktop]\nmax_sessions=1";

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

/// One run event at `seq`, waited for: frames on the link are handled in order, so what was sent before it is too.
async fn event(
    f: &Fixture,
    runtime: &mut support::runtime::Runtime,
    id: &ConversationId,
    (seq, run): (u64, &str),
    event: Event,
) -> TestResult {
    runtime
        .send(&ServerFrame::Event {
            seq: seq.into(),
            run_id: Some(run.parse()?),
            step: None,
            ts: "2026-10-17T12:00:00Z".into(),
            event,
        })
        .await?;
    f.until_sequence(id, seq).await
}

/// Run `run` starts and completes at `seq` and `seq + 1`, and the runtime's reset after it reports `ok`.
async fn ran_and_reset(
    f: &Fixture,
    runtime: &mut support::runtime::Runtime,
    id: &ConversationId,
    (seq, run): (u64, &str),
    ok: bool,
) -> TestResult {
    let run_id: tqa_types::RunId = run.parse()?;
    event(
        f,
        runtime,
        id,
        (seq, run),
        Event::RunStarted {
            run_id: run_id.clone(),
        },
    )
    .await?;
    event(
        f,
        runtime,
        id,
        (seq + 1, run),
        Event::RunCompleted { run_id },
    )
    .await?;
    runtime
        .send(&ServerFrame::ScreenReset { ok, ready: true })
        .await?;
    Ok(())
}

/// A desktop task with its sandbox live (`first`), and a second desktop task with a message waiting for a desktop.
async fn two_desktops(
    f: &Fixture,
) -> Result<
    (
        ConversationId,
        support::runtime::Runtime,
        ConversationId,
        String,
    ),
    Box<dyn std::error::Error>,
> {
    let (user, cookie) = f.admin().await?;
    let first = f.conversation(user.id).await?;
    desktop(f, &first.id).await?;
    let runtime = f.runtime(&first).await?;
    let second = f.conversation(user.id).await?;
    desktop(f, &second.id).await?;
    Ok((first.id, runtime, second.id, cookie))
}

async fn message(f: &Fixture, id: &ConversationId, cookie: &str, key: &str) -> TestResult {
    let accepted = f
        .action(
            &format!("/api/v1/conversations/{id}/messages"),
            cookie,
            Some(key),
            &json!({"text": "work"}),
        )
        .await?;
    assert_eq!(accepted.status(), 202);
    Ok(())
}

/// Admission passes until `second` has a sandbox: the idle desktop gave way.
async fn gives_way(f: &Fixture, second: &ConversationId) -> TestResult {
    tokio::time::timeout(DEADLINE * 4, async {
        while f.backend.created(second).is_none() {
            f.gateway.sandboxes().admission_pass_now().await?;
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        Ok::<(), Box<dyn std::error::Error>>(())
    })
    .await??;
    assert!(
        f.backend.destroys.load(Ordering::SeqCst) >= 1,
        "the idle desktop was retired for it"
    );
    Ok(())
}

/// Several admission passes, and the first desktop is still there: the second waits.
async fn keeps_its_place(f: &Fixture, second: &ConversationId, why: &str) -> TestResult {
    for _ in 0..5 {
        f.gateway.sandboxes().admission_pass_now().await?;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(f.backend.destroys.load(Ordering::SeqCst), 0, "{why}");
    assert!(f.backend.created(second).is_none(), "{why}");
    Ok(())
}

// TEST: GW-I-305
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_305_a_finished_desktop_gives_way_to_a_waiting_one(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, CONFIG, async |f| {
        let (first, mut runtime, second, cookie) = two_desktops(f).await?;
        ran_and_reset(f, &mut runtime, &first, (1, "r-1"), true).await?;
        message(f, &second, &cookie, "second").await?;
        gives_way(f, &second).await
    })
    .await
}

// TEST: GW-I-305
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_305_a_desktop_taken_over_since_its_reset_keeps_its_sign_in(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, CONFIG, async |f| {
        let (first, mut runtime, second, cookie) = two_desktops(f).await?;
        ran_and_reset(f, &mut runtime, &first, (1, "r-1"), true).await?;
        // The person takes over (granted after the reset) and hands back without a run: a sign-in may be there.
        let mut page = viewer(f, &first, &cookie).await?;
        let _bridge = bridge(f, &first).await?;
        page.send(Message::Text(r#"{"type":"take_over"}"#.into()))
            .await?;
        until_holder(&mut page, "you").await?;
        drop(page);
        message(f, &second, &cookie, "second").await?;
        keeps_its_place(
            f,
            &second,
            "a Take over since the reset: its sign-in serves the next run",
        )
        .await?;
        // That run's end empties the profile: then it gives way.
        ran_and_reset(f, &mut runtime, &first, (3, "r-2"), true).await?;
        gives_way(f, &second).await
    })
    .await
}

// TEST: GW-I-305
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_305_a_running_or_pinned_desktop_or_a_failed_reset_keeps_its_place(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, CONFIG, async |f| {
        let (first, mut runtime, second, cookie) = two_desktops(f).await?;
        // A reset that couldn't empty the profile.
        ran_and_reset(f, &mut runtime, &first, (1, "r-1"), false).await?;
        message(f, &second, &cookie, "second").await?;
        keeps_its_place(f, &second, "a failed reset leaves the profile unknown").await?;
        // A clean one, then a run that hasn't ended.
        ran_and_reset(f, &mut runtime, &first, (3, "r-2"), true).await?;
        event(
            f,
            &mut runtime,
            &first,
            (5, "r-3"),
            Event::RunStarted {
                run_id: "r-3".parse()?,
            },
        )
        .await?;
        keeps_its_place(f, &second, "a run is active").await?;
        event(
            f,
            &mut runtime,
            &first,
            (6, "r-3"),
            Event::RunCompleted {
                run_id: "r-3".parse()?,
            },
        )
        .await?;
        runtime
            .send(&ServerFrame::ScreenReset {
                ok: true,
                ready: true,
            })
            .await?;
        // Pinned by its person.
        sqlx::query("UPDATE conversations SET pinned=true WHERE id=$1")
            .bind(Uuid::parse_str(first.as_str())?)
            .execute(&f.pool)
            .await?;
        keeps_its_place(f, &second, "pinned").await?;
        sqlx::query("UPDATE conversations SET pinned=false WHERE id=$1")
            .bind(Uuid::parse_str(first.as_str())?)
            .execute(&f.pool)
            .await?;
        gives_way(f, &second).await
    })
    .await
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// The task's screen, opened as its page does.
async fn viewer(
    f: &Fixture,
    id: &ConversationId,
    cookie: &str,
) -> Result<Socket, Box<dyn std::error::Error>> {
    let mut request = f
        .url(&format!("/api/v1/conversations/{id}/screen"))
        .replacen("http:", "ws:", 1)
        .into_client_request()?;
    request.headers_mut().insert("cookie", cookie.parse()?);
    request.headers_mut().insert("origin", f.origin().parse()?);
    Ok(tokio_tungstenite::connect_async(request).await?.0)
}

/// The runtime's bridge dial for the task's screen.
async fn bridge(f: &Fixture, id: &ConversationId) -> Result<Socket, Box<dyn std::error::Error>> {
    use secrecy::ExposeSecret;
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

/// Reads control frames until one names `holder`.
async fn until_holder(page: &mut Socket, holder: &str) -> TestResult {
    use futures::StreamExt;
    tokio::time::timeout(DEADLINE * 4, async {
        while let Some(message) = page.next().await {
            if names_holder(&message?, holder)? {
                return Ok(());
            }
        }
        Err::<(), Box<dyn std::error::Error>>("the screen closed".into())
    })
    .await?
}

/// Whether `message` is a control frame naming `holder`.
fn names_holder(message: &Message, holder: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let Message::Text(text) = message else {
        return Ok(false);
    };
    let value: serde_json::Value = serde_json::from_str(text)?;
    Ok(value["type"] == "control" && value["holder"] == holder)
}

// TEST: GW-I-308
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_308_a_desktop_that_may_hold_a_sign_in_isnt_evicted_for_a_slot(
    pool: PgPool,
) -> TestResult {
    // The lead, 17 Oct 2026: a slot's eviction took any idle sandbox, a Desktop with a hand sign-in too. One slot, an
    // idle Desktop in it, and a task on another environment waiting for the slot.
    support::run_configured(
        pool,
        None,
        "[admission]\nslots=1\nper_user=1\n[desktop]\nmax_sessions=1",
        async |f| {
            let (user, cookie) = f.admin().await?;
            let first = f.conversation(user.id).await?;
            desktop(f, &first.id).await?;
            let mut runtime = f.runtime(&first).await?;
            ran_and_reset(f, &mut runtime, &first.id, (1, "r-1"), true).await?;
            let mut page = viewer(f, &first.id, &cookie).await?;
            let _bridge = bridge(f, &first.id).await?;
            page.send(Message::Text(r#"{"type":"take_over"}"#.into()))
                .await?;
            until_holder(&mut page, "you").await?;
            drop(page);
            // Another person's task, so it waits for the slot, not for its owner's cap.
            let (other, other_cookie) = f.member("slot-waiter@example.invalid").await?;
            let plain = f.conversation(other.id).await?;
            message(f, &plain.id, &other_cookie, "plain").await?;
            keeps_its_place(
                f,
                &plain.id,
                "a Take over since the reset: not evicted for a slot",
            )
            .await?;
            // The next run's end empties the profile: then the slot's eviction may take it.
            ran_and_reset(f, &mut runtime, &first.id, (3, "r-2"), true).await?;
            gives_way(f, &plain.id).await
        },
    )
    .await
}
