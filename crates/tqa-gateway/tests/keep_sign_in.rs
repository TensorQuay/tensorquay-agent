//! GW-I-324 (ADR-0039 section 6, amended for 18.0; PRD item 24): the task's "keep the sign-in" switch. Its PATCH
//! answers with the field, records one screen notice per change naming who made it, tells a linked runtime that
//! speaks revision 15 at once (`screen.keep_sign_in`) and an older one nothing, and a new session's `session.config`
//! carries it to a revision-15 runtime only.
#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use support::{Fixture, TestResult, call};
use tqa_types::{ClientFrame, ConversationId, ServerFrame};

/// Makes the task's environment a Desktop (the switch is for desktop tasks; the gateway stores it for any).
async fn desktop(f: &Fixture, conversation: &ConversationId) -> TestResult {
    sqlx::query("INSERT INTO templates (name,display_name,image,capabilities,is_default,version,last_set_by) VALUES ('desktop','Desktop',$1,'{shell,browser,desktop}',false,1,'admin') ON CONFLICT (name) DO NOTHING")
        .bind(format!("example/desktop@sha256:{}", "b".repeat(64)))
        .execute(&f.pool)
        .await?;
    sqlx::query("UPDATE conversations SET template='desktop' WHERE id=$1")
        .bind(uuid::Uuid::parse_str(conversation.as_str())?)
        .execute(&f.pool)
        .await?;
    Ok(())
}

/// A desktop sandbox's first reset, which the real runtime sends after its bootstrap (RT-I-051) and the gateway waits
/// for before it starts a turn: the fake runtime sends it here.
async fn reset_over(runtime: &mut support::runtime::Runtime) -> TestResult {
    runtime
        .send(&ServerFrame::ScreenReset {
            ok: true,
            ready: true,
            kept: false,
        })
        .await
}

/// Waits (bounded) for `condition`: a retirement runs in the background, and once the sandbox is gone no worker reads
/// the clock any more.
async fn until(f: &Fixture, condition: impl Fn(&Fixture) -> bool) -> TestResult {
    tokio::time::timeout(support::DEADLINE, async {
        while !condition(f) {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    Ok(())
}

async fn switch(
    f: &Fixture,
    id: &ConversationId,
    cookie: &str,
    on: bool,
    key: &str,
) -> Result<(u16, Value), Box<dyn std::error::Error>> {
    call(
        f,
        Method::PATCH,
        &format!("/api/v1/conversations/{id}"),
        cookie,
        (Some(json!({"keep_sign_in": on})), Some(key)),
    )
    .await
}

async fn notice_kinds(
    f: &Fixture,
    id: &ConversationId,
    cookie: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let (status, body) = call(
        f,
        Method::GET,
        &format!("/api/v1/conversations/{id}/screen/notices"),
        cookie,
        (None, None),
    )
    .await?;
    assert_eq!(status, 200, "{body}");
    Ok(body["items"]
        .as_array()
        .ok_or("items")?
        .iter()
        .map(|item| item["kind"].as_str().unwrap_or("").to_owned())
        .collect())
}

// TEST: GW-I-324
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_324_the_switch_answers_notes_who_and_reaches_a_runtime_that_speaks_15(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let task = f.conversation(owner.id).await?;
        desktop(f, &task.id).await?;
        let mut runtime = f.runtime(&task).await?;
        reset_over(&mut runtime).await?;
        // Off by default: a new session says nothing of it (off is every runtime's starting point).
        assert!(
            runtime.config.get("keep_sign_in").is_none(),
            "{}",
            runtime.config
        );
        let (status, body) = switch(f, &task.id, &cookie, true, "keep-1").await?;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["keep_sign_in"], json!(true));
        assert_eq!(
            runtime.action().await?,
            ClientFrame::ScreenKeepSignIn { on: true }
        );
        // The same value again: no notice and no frame; the answer is the record.
        let (status, body) = switch(f, &task.id, &cookie, true, "keep-2").await?;
        assert_eq!((status, &body["keep_sign_in"]), (200, &json!(true)));
        let (status, body) = switch(f, &task.id, &cookie, false, "keep-3").await?;
        assert_eq!((status, &body["keep_sign_in"]), (200, &json!(false)));
        assert_eq!(
            runtime.action().await?,
            ClientFrame::ScreenKeepSignIn { on: false }
        );
        let kinds = notice_kinds(f, &task.id, &cookie).await?;
        assert_eq!(
            kinds,
            vec![
                "screen.sign_in_kept".to_owned(),
                "screen.sign_in_released".to_owned()
            ],
            "one notice per change, none for a repeat"
        );
        let (_, body) = call(
            f,
            Method::GET,
            &format!("/api/v1/conversations/{}/screen/notices", task.id),
            &cookie,
            (None, None),
        )
        .await?;
        assert_eq!(body["items"][0]["by"]["id"], json!(owner.id.to_string()));
        // The record itself carries it.
        let (status, body) = call(
            f,
            Method::GET,
            &format!("/api/v1/conversations/{}", task.id),
            &cookie,
            (None, None),
        )
        .await?;
        assert_eq!((status, &body["keep_sign_in"]), (200, &json!(false)));
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_324_an_older_runtime_hears_nothing_and_a_new_session_carries_the_switch(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let task = f.conversation(owner.id).await?;
        desktop(f, &task.id).await?;
        // A revision-14 runtime: the session.config has no keep_sign_in, and a change sends it nothing.
        let mut old = f.runtime_at(&task.id, 14).await?;
        reset_over(&mut old).await?;
        assert!(old.config.get("keep_sign_in").is_none(), "{}", old.config);
        let (status, body) = switch(f, &task.id, &cookie, true, "keep-old").await?;
        assert_eq!((status, &body["keep_sign_in"]), (200, &json!(true)));
        let response = f
            .action(
                &format!("/api/v1/conversations/{}/messages", task.id),
                &cookie,
                Some("after-switch"),
                &json!({"text": "after"}),
            )
            .await?;
        assert_eq!(response.status(), 202);
        // The next frame the old runtime gets is the turn, never the switch.
        assert!(matches!(
            old.action().await?,
            ClientFrame::UserMessage { .. }
        ));
        old.close().await?;
        // The kept sandbox stays up without its runtime (no idle retirement while the switch is on). A new session on it,
        // at the current revision, as a runtime started again inside the sandbox: the switch rides on its session.config.
        let mut fresh = f.runtime_of_sandbox(&task.id).await?;
        reset_over(&mut fresh).await?;
        assert_eq!(fresh.config["keep_sign_in"], json!(true));
        Ok(())
    })
    .await
}

/// Idle never retires a sandbox whose task keeps its sign-in (decided on 15 Nov 2026: a person's pause
/// between messages must not end the sign-in), as a pinned task's; its lifetime limit still does. Off again, the idle
/// window retires it as before.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_324_a_kept_sign_in_outlives_the_idle_window_until_the_lifetime(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let task = f.conversation(owner.id).await?;
        desktop(f, &task.id).await?;
        f.clock.freeze();
        let mut runtime = f.runtime(&task).await?;
        reset_over(&mut runtime).await?;
        let (status, body) = switch(f, &task.id, &cookie, true, "keep-idle").await?;
        assert_eq!((status, &body["keep_sign_in"]), (200, &json!(true)));
        f.clock.observed().await?;
        f.clock.observed().await?;
        // Well past the 600 s idle window, before the 28,800 s default lifetime: the sandbox stays.
        f.clock.advance(Duration::from_secs(28_799));
        f.clock.observed().await?;
        f.clock.observed().await?;
        assert_eq!(
            f.backend.destroys.load(Ordering::SeqCst),
            0,
            "idle didn't retire it"
        );
        // The lifetime limit still ends it.
        f.clock.advance(Duration::from_secs(1));
        until(f, |f| f.backend.destroys.load(Ordering::SeqCst) == 1).await?;
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_324_switched_off_the_idle_window_retires_it_as_before(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let task = f.conversation(owner.id).await?;
        desktop(f, &task.id).await?;
        f.clock.freeze();
        let mut runtime = f.runtime(&task).await?;
        reset_over(&mut runtime).await?;
        switch(f, &task.id, &cookie, true, "keep-on").await?;
        switch(f, &task.id, &cookie, false, "keep-off").await?;
        f.clock.observed().await?;
        f.clock.observed().await?;
        f.clock.advance(Duration::from_secs(601));
        until(f, |f| f.backend.destroys.load(Ordering::SeqCst) == 1).await?;
        Ok(())
    })
    .await
}

/// Without a desktop the switch is refused with a field problem (the review of 15 Nov 2026): it would keep a sandbox
/// alive through idle for nothing. Off is always accepted.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_324_the_switch_is_refused_on_a_task_without_a_desktop(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let task = f.conversation(owner.id).await?;
        let (status, body) = switch(f, &task.id, &cookie, true, "keep-plain").await?;
        // A field problem answers 400 invalid_request with its details, as every field problem of the API does.
        assert_eq!(status, 400, "{body}");
        assert_eq!(body["error"]["code"], json!("invalid_request"));
        assert_eq!(body["error"]["details"][0]["field"], json!("keep_sign_in"));
        assert_eq!(body["error"]["details"][0]["reason"], json!("no_desktop"));
        let (status, body) = switch(f, &task.id, &cookie, false, "keep-plain-off").await?;
        assert_eq!((status, &body["keep_sign_in"]), (200, &json!(false)));
        assert!(
            notice_kinds(f, &task.id, &cookie).await?.is_empty(),
            "off on off: no change, no notice"
        );
        Ok(())
    })
    .await
}

/// An environment switch away from the desktop clears the switch, so the sandbox retires after its idle window again.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_324_a_switch_away_from_the_desktop_clears_it(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let task = f.conversation(owner.id).await?;
        desktop(f, &task.id).await?;
        let (status, body) = switch(f, &task.id, &cookie, true, "keep-then-switch").await?;
        assert_eq!((status, &body["keep_sign_in"]), (200, &json!(true)));
        let (status, body) = call(
            f,
            Method::PUT,
            &format!("/api/v1/conversations/{}/environment", task.id),
            &cookie,
            (Some(json!({"template": "minimal"})), Some("to-minimal")),
        )
        .await?;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            body["keep_sign_in"],
            json!(false),
            "cleared with the desktop gone"
        );
        Ok(())
    })
    .await
}
