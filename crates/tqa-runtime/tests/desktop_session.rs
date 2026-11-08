//! SEC-059's runtime half (ADR-0039 section 6, amended 7 Oct 2026): every run of a desktop session starts
//! signed out, except that a sign-in made during a Take over serves the run that follows it. The profile holding that
//! sign-in is still there when the next run starts; when that run ends it is gone, so the run after starts signed
//! out. After every reset, and after Sign out everywhere, the runtime starts its browser again, blank, on
//! the emptied profile, so a Take over always finds the runtime's own browser to sign in with.
#[path = "support/fixture.rs"]
mod support;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use support::*;
use tqa_types::ClientFrame;

fn desktop_session() -> Result<ClientFrame, Box<dyn std::error::Error>> {
    let mut value = serde_json::to_value(session()?)?;
    value["environment"] = json!({
        "template": "desktop", "digest": format!("sha256:{}", "a".repeat(64)),
        "capabilities": ["shell", "python", "node", "build", "browser", "desktop"],
    });
    Ok(ClientFrame::parse(&serde_json::to_vec(&value)?)?)
}

// TEST: SEC-059
// TEST: GW-I-222
#[tokio::test]
async fn sec_059_a_take_over_sign_in_serves_the_next_run_and_no_later_one() -> TestResult {
    let browser = tempfile::tempdir()?;
    let profile = browser.path().join("profile");
    let cookies = profile.join("Default").join("Cookies");
    let f = Fixture::new(vec![answer(), answer()]).await?;
    f.gateway.set_session(desktop_session()?)?;
    f.gateway.set_bound(true);
    let testing = tqa_runtime::testing::TestingHooks::default();
    *testing.browser_profile.lock().map_err(|_| "lock")? = Some(profile.clone());
    let reopened = testing.browser_reopened.clone();
    let (shutdown, task) = f.launch_with(testing)?;
    f.gateway.wait_for_registrations(1).await?;
    // The session's start leaves the browser open, blank, on an empty profile: what a Take over finds.
    signed_in_by_hand(&reopened, 1, &profile, &cookies).await?;
    f.gateway.send(user()).await?;
    f.wait_event("run.completed").await?;
    // The run that followed the sign-in has ended: the sign-in is gone, and the browser is back, blank, on the emptied
    // profile, so a Take over before the next run finds it.
    blank(&reopened, 2, &profile).await?;
    // The next run starts signed out, and its end leaves the profile empty again.
    f.gateway.send(user()).await?;
    wait_for(|| {
        f.gateway
            .frames()
            .iter()
            .filter(|frame| matches!(frame, tqa_types::ServerFrame::Event { event, .. } if event_name(event) == "run.completed"))
            .count()
            == 2
    })
    .await?;
    blank(&reopened, 3, &profile).await?;
    stop(shutdown, task).await?;
    f.gateway.shutdown().await?;
    Ok(())
}

/// Once the browser has started `starts` times, it is open on an empty profile.
async fn blank(reopened: &AtomicUsize, starts: usize, profile: &std::path::Path) -> TestResult {
    wait_for(|| reopened.load(Ordering::SeqCst) == starts).await?;
    assert!(profile.is_dir(), "the browser started again on its profile");
    assert_eq!(
        std::fs::read_dir(profile)?.count(),
        0,
        "the profile it started on is empty"
    );
    Ok(())
}

/// The person signs in during a Take over, with no run going, in the browser the runtime left open.
async fn signed_in_by_hand(
    reopened: &AtomicUsize,
    starts: usize,
    profile: &std::path::Path,
    cookies: &std::path::Path,
) -> TestResult {
    blank(reopened, starts, profile).await?;
    std::fs::create_dir_all(cookies.parent().ok_or("parent")?)?;
    std::fs::write(cookies, b"session=signed-in-during-take-over")?;
    Ok(())
}

// TEST: GW-I-222
// TEST: SEC-059
#[tokio::test]
async fn gw_i_222_sign_out_everywhere_empties_the_profile_and_starts_the_browser_blank()
-> TestResult {
    let browser = tempfile::tempdir()?;
    let profile = browser.path().join("profile");
    let cookies = profile.join("Default").join("Cookies");
    let f = Fixture::new(vec![]).await?;
    f.gateway.set_session(desktop_session()?)?;
    f.gateway.set_bound(true);
    let testing = tqa_runtime::testing::TestingHooks::default();
    *testing.browser_profile.lock().map_err(|_| "lock")? = Some(profile.clone());
    let reopened = testing.browser_reopened.clone();
    let (shutdown, task) = f.launch_with(testing)?;
    f.gateway.wait_for_registrations(1).await?;
    signed_in_by_hand(&reopened, 1, &profile, &cookies).await?;
    f.gateway.send(ClientFrame::ScreenSignOut {}).await?;
    wait_for(|| {
        f.gateway.frames().iter().any(|frame| {
            matches!(
                frame,
                tqa_types::ServerFrame::ScreenSignOutDone { ok: true, .. }
            )
        })
    })
    .await?;
    // Every sign-in is gone, and the browser is open again, blank.
    blank(&reopened, 2, &profile).await?;
    stop(shutdown, task).await?;
    f.gateway.shutdown().await?;
    Ok(())
}

// TEST: RT-I-056
/// Sign out everywhere answers once the browser has started again, and says whether it is ready for a person's keys,
/// as a run's end reset does (RT-I-055). Before, it answered first and started the browser after, so a Take over right
/// after it was granted plain control over a desktop with no browser yet (found before 17.0).
#[tokio::test]
async fn rt_i_056_sign_out_everywhere_answers_after_the_browser_is_back_with_its_ready()
-> TestResult {
    let browser = tempfile::tempdir()?;
    let profile = browser.path().join("profile");
    let cookies = profile.join("Default").join("Cookies");
    let f = Fixture::new(vec![]).await?;
    f.gateway.set_session(desktop_session()?)?;
    f.gateway.set_bound(true);
    let testing = tqa_runtime::testing::TestingHooks::default();
    *testing.browser_profile.lock().map_err(|_| "lock")? = Some(profile.clone());
    let reopened = testing.browser_reopened.clone();
    // A slow start whose window never takes the focus.
    testing.browser_reopen_ms.store(300, Ordering::SeqCst);
    let not_ready = testing.browser_not_ready.clone();
    let (shutdown, task) = f.launch_with(testing)?;
    f.gateway.wait_for_registrations(1).await?;
    signed_in_by_hand(&reopened, 1, &profile, &cookies).await?;
    not_ready.store(true, Ordering::SeqCst);
    f.gateway.send(ClientFrame::ScreenSignOut {}).await?;
    let answer = || {
        f.gateway.frames().iter().find_map(|frame| {
            matches!(frame, tqa_types::ServerFrame::ScreenSignOutDone { .. })
                .then(|| serde_json::to_value(frame).ok())
                .flatten()
        })
    };
    wait_for(|| answer().is_some()).await?;
    assert_eq!(
        reopened.load(Ordering::SeqCst),
        2,
        "the answer came before the browser started again"
    );
    let answer = answer().ok_or("no answer")?;
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(
        answer["ready"], false,
        "a browser not back in time is said: {answer}"
    );
    blank(&reopened, 2, &profile).await?;
    stop(shutdown, task).await?;
    f.gateway.shutdown().await?;
    Ok(())
}

// TEST: RT-I-051
#[tokio::test]
async fn rt_i_051_a_desktop_run_end_reset_is_announced_once_it_is_over() -> TestResult {
    let browser = tempfile::tempdir()?;
    let profile = browser.path().join("profile");
    let f = Fixture::new(vec![answer()]).await?;
    f.gateway.set_session(desktop_session()?)?;
    f.gateway.set_bound(true);
    let testing = tqa_runtime::testing::TestingHooks::default();
    *testing.browser_profile.lock().map_err(|_| "lock")? = Some(profile.clone());
    let reopened = testing.browser_reopened.clone();
    let (shutdown, task) = f.launch_with(testing)?;
    f.gateway.wait_for_registrations(1).await?;
    blank(&reopened, 1, &profile).await?;
    f.gateway.send(user()).await?;
    f.wait_event("run.completed").await?;
    // The run's reset is announced after its end, once the browser has started again on the emptied profile. (The
    // session's bootstrap ends like a run, so a reset may come before the run too.)
    let after_end = || {
        let frames = f.gateway.frames();
        let ended = frames.iter().position(|frame| {
            matches!(frame, tqa_types::ServerFrame::Event { event, .. } if event_name(event) == "run.completed")
        });
        ended.map_or(0, |ended| {
            frames[ended..]
                .iter()
                .filter(|frame| clean_reset(frame))
                .count()
        })
    };
    wait_for(|| after_end() == 1).await?;
    let resets = f
        .gateway
        .frames()
        .iter()
        .filter(|frame| clean_reset(frame))
        .count();
    // Each reset went once the browser was started again (the session's first start is its bootstrap's reset).
    assert_eq!(reopened.load(Ordering::SeqCst), resets);
    stop(shutdown, task).await?;
    f.gateway.shutdown().await?;
    Ok(())
}

/// A run-end reset that emptied the profile.
fn clean_reset(frame: &tqa_types::ServerFrame) -> bool {
    matches!(frame, tqa_types::ServerFrame::ScreenReset { ok: true, .. })
}
