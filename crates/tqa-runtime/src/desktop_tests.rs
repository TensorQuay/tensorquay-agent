//! GW-I-222's runtime half and SEC-059's Sign out everywhere (ADR-0039 section 6): the session's browser closed and its
//! profile emptied; a driver a run still holds, a process still naming the profile, and a profile that can't be
//! removed each refuse with their reason.
use super::{READY_WAIT, Slot, focused, release, reopened, sign_out};
use crate::SupervisedGroups;
use async_trait::async_trait;
use std::os::unix::process::ExitStatusExt as _;
use std::{sync::Arc, time::Duration};
use tqa_tools::builtin::computer::driver::{
    DriverError, ElementAct, RawSnapshot, ScreenDriver, ScreenState,
};
use tqa_tools::builtin::computer::x11::{Runner, XDo};
use tqa_types::SignOutFailure;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// A driver that does nothing: what matters here is who holds it.
struct Idle;

#[async_trait]
impl ScreenDriver for Idle {
    async fn snapshot(&self) -> Result<RawSnapshot, DriverError> {
        Err(DriverError::Unavailable("idle".into()))
    }
    async fn state(&self) -> Result<ScreenState, DriverError> {
        Err(DriverError::Unavailable("idle".into()))
    }
    async fn act(&self, _handle: &str, _act: ElementAct) -> Result<(), DriverError> {
        Err(DriverError::Unavailable("idle".into()))
    }
    async fn type_focused(&self, _text: &str) -> Result<(), DriverError> {
        Err(DriverError::Unavailable("idle".into()))
    }
    async fn key(&self, _keys: &[String]) -> Result<(), DriverError> {
        Err(DriverError::Unavailable("idle".into()))
    }
}

/// A profile with a cookie and site data in it.
fn signed_in(root: &std::path::Path) -> Result<std::path::PathBuf, std::io::Error> {
    let profile = root.join("profile");
    std::fs::create_dir_all(profile.join("Default"))?;
    std::fs::write(
        profile.join("Default").join("Cookies"),
        b"session=signed-in",
    )?;
    Ok(profile)
}

// TEST: RT-I-054
#[tokio::test]
async fn rt_i_054_sign_out_forgets_the_browser_the_reaper_spared() -> TestResult {
    // A pid the reaper spared as the browser's isn't spared once the browser is gone: another process may get it.
    let root = tempfile::tempdir()?;
    let profile = root.path().join("profile");
    let spared = SupervisedGroups::default();
    spared.browser(Some(4_242));
    assert!(spared.contains(4_242));
    let slot: Slot = Arc::default();
    *slot.lock().await = Some(Arc::new(Idle));
    sign_out(&slot, &profile, Duration::from_millis(300), &spared)
        .await
        .map_err(|failure| format!("{failure:?}"))?;
    assert!(!spared.contains(4_242));
    Ok(())
}

// TEST: GW-I-222
// TEST: SEC-059
#[tokio::test]
async fn gw_i_222_sign_out_closes_the_browser_and_empties_its_profile() -> TestResult {
    let root = tempfile::tempdir()?;
    let profile = signed_in(root.path())?;
    let slot: Slot = Arc::default();
    *slot.lock().await = Some(Arc::new(Idle));
    sign_out(
        &slot,
        &profile,
        Duration::from_secs(1),
        &SupervisedGroups::default(),
    )
    .await
    .map_err(|failure| format!("{failure:?}"))?;
    assert!(
        !profile.exists(),
        "the profile, and every sign-in in it, is gone"
    );
    assert!(
        slot.lock().await.is_none(),
        "the browser is closed; the next action starts it fresh"
    );
    // A second Sign out everywhere, or one before any browser started, is done too.
    sign_out(
        &slot,
        &profile,
        Duration::from_secs(1),
        &SupervisedGroups::default(),
    )
    .await
    .map_err(|failure| format!("{failure:?}"))?;
    Ok(())
}

// TEST: GW-I-222
#[tokio::test]
async fn gw_i_222_a_driver_a_run_still_holds_is_profile_in_use() -> TestResult {
    let root = tempfile::tempdir()?;
    let profile = signed_in(root.path())?;
    let slot: Slot = Arc::default();
    let driver: Arc<dyn ScreenDriver> = Arc::new(Idle);
    *slot.lock().await = Some(driver.clone());
    let refused = sign_out(
        &slot,
        &profile,
        Duration::from_secs(1),
        &SupervisedGroups::default(),
    )
    .await;
    assert_eq!(refused, Err(SignOutFailure::ProfileInUse));
    assert!(
        profile.join("Default").join("Cookies").exists(),
        "nothing removed while in use"
    );
    assert!(slot.lock().await.is_some());
    drop(driver);
    Ok(())
}

/// A process naming `profile` as its `--user-data-dir`, as a browser started by hand does; `script` is what it runs.
fn holder(profile: &std::path::Path, script: &str) -> std::io::Result<tokio::process::Child> {
    tokio::process::Command::new("sh")
        .args([
            "-c",
            script,
            &format!("--user-data-dir={}", profile.display()),
        ])
        .kill_on_drop(true)
        .spawn()
}

// TEST: GW-I-222
// TEST: SEC-059
#[tokio::test]
async fn gw_i_222_a_process_holding_the_profile_is_stopped_and_the_profile_emptied() -> TestResult {
    let root = tempfile::tempdir()?;
    let profile = signed_in(root.path())?;
    let mut held = holder(&profile, "sleep 30")?;
    let slot: Slot = Arc::default();
    sign_out(
        &slot,
        &profile,
        Duration::from_secs(2),
        &SupervisedGroups::default(),
    )
    .await
    .map_err(|failure| format!("{failure:?}"))?;
    assert!(!profile.exists(), "the profile, and its sign-ins, are gone");
    let status = tokio::time::timeout(Duration::from_secs(5), held.wait()).await??;
    assert_eq!(status.signal(), Some(15), "stopped by SIGTERM");
    Ok(())
}

// TEST: GW-I-222
// TEST: SEC-059
#[tokio::test]
async fn gw_i_222_a_holder_ignoring_sigterm_is_killed_after_the_grace() -> TestResult {
    let root = tempfile::tempdir()?;
    let profile = signed_in(root.path())?;
    let mut held = holder(&profile, "trap '' TERM; while :; do sleep 0.1; done")?;
    // The shell's trap is in place before anything is sent.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let slot: Slot = Arc::default();
    sign_out(
        &slot,
        &profile,
        Duration::from_millis(300),
        &SupervisedGroups::default(),
    )
    .await
    .map_err(|failure| format!("{failure:?}"))?;
    assert!(!profile.exists());
    let status = tokio::time::timeout(Duration::from_secs(5), held.wait()).await??;
    assert_eq!(status.signal(), Some(9), "killed once the grace ran out");
    Ok(())
}

// TEST: GW-I-222
#[tokio::test]
async fn gw_i_222_a_holder_that_survives_sigkill_is_profile_in_use() -> TestResult {
    let root = tempfile::tempdir()?;
    let profile = signed_in(root.path())?;
    let mut held = holder(&profile, "sleep 30")?;
    // A process no signal reaches (another user's, say): the signals are sent nowhere.
    let refused = release(&profile, Duration::from_millis(100), &|_, _| ()).await;
    assert_eq!(refused, Err(SignOutFailure::ProfileInUse));
    assert!(
        profile.join("Default").join("Cookies").exists(),
        "a profile still in use is left as it is"
    );
    held.kill().await?;
    Ok(())
}

// TEST: GW-I-222
#[tokio::test]
async fn gw_i_222_a_profile_that_cant_be_removed_is_io_error() -> TestResult {
    // A removal that fails for any user: the profile's path holds a file, so removing it as a directory is ENOTDIR. A
    // read-only directory wouldn't do, as the Docker lane runs the tests as its user namespace's root, which writes
    // through any mode.
    let root = tempfile::tempdir()?;
    let profile = root.path().join("profile");
    std::fs::write(&profile, b"not a directory")?;
    let slot: Slot = Arc::default();
    let result = sign_out(
        &slot,
        &profile,
        Duration::from_millis(300),
        &SupervisedGroups::default(),
    )
    .await;
    assert_eq!(result, Err(SignOutFailure::IoError));
    assert!(profile.is_file(), "nothing was removed");
    Ok(())
}

/// A display whose focus moves as scripted: each `getactivewindow` takes the next window, `None` being the window
/// manager's (it names no process); the last one stays.
struct Focus {
    windows: std::sync::Mutex<std::collections::VecDeque<Option<u32>>>,
    asked: std::sync::atomic::AtomicUsize,
}

impl Focus {
    fn moving(windows: &[Option<u32>]) -> Arc<Self> {
        Arc::new(Self {
            windows: std::sync::Mutex::new(windows.iter().copied().collect()),
            asked: std::sync::atomic::AtomicUsize::new(0),
        })
    }
    fn now(&self) -> Option<u32> {
        let windows = self
            .windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        windows.front().copied().flatten()
    }
}

#[async_trait]
impl Runner for Focus {
    async fn run(&self, _program: &str, args: &[String]) -> Result<String, DriverError> {
        match args.first().map(String::as_str) {
            Some("getactivewindow") => {
                let mut windows = self
                    .windows
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if self.asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst) > 0
                    && windows.len() > 1
                {
                    windows.pop_front();
                }
                Ok(if windows.front().copied().flatten().is_some() {
                    "2\n"
                } else {
                    "1\n"
                }
                .into())
            }
            Some("getwindowfocus") => Ok(if self.now().is_some() { "2\n" } else { "1\n" }.into()),
            Some("getwindowpid") => self
                .now()
                .map(|pid| format!("{pid}\n"))
                .ok_or_else(|| DriverError::Unavailable("no pid".into())),
            Some("getwindowname") => Ok("window\n".into()),
            _ => Err(DriverError::Unavailable("unscripted".into())),
        }
    }
}

// TEST: RT-I-053
#[tokio::test]
async fn rt_i_053_the_reopened_browser_has_the_focus_before_the_reset_ends() -> TestResult {
    // The window manager holds the focus while the new browser's window maps (c16, 12 Oct 2026: a Take over granted
    // then typed into no window), then the browser's window takes it: three looks, then done.
    let display = Focus::moving(&[None, None, Some(4_000)]);
    let x = XDo::new(display.clone());
    assert!(focused(&x, 4_000, Duration::from_secs(5), Duration::from_millis(10)).await);
    assert_eq!(display.asked.load(std::sync::atomic::Ordering::SeqCst), 3);
    Ok(())
}

// TEST: RT-I-053
#[tokio::test]
async fn rt_i_053_a_browser_that_never_takes_the_focus_is_waited_for_no_longer_than_the_bound()
-> TestResult {
    // Another process's window keeps the focus: the wait ends at its bound, and the reset goes on.
    let display = Focus::moving(&[None, Some(1)]);
    let x = XDo::new(display.clone());
    let started = std::time::Instant::now();
    assert!(
        !focused(
            &x,
            4_000,
            Duration::from_millis(200),
            Duration::from_millis(10)
        )
        .await
    );
    let waited = started.elapsed();
    assert!(
        waited >= Duration::from_millis(200) && waited < Duration::from_secs(5),
        "{waited:?}"
    );
    assert!(display.asked.load(std::sync::atomic::Ordering::SeqCst) > 2);
    Ok(())
}

// TEST: RT-I-055
/// The run-end reset says the browser is ready only when it is (16r's `sec_059_a`: under load a person was given
/// control over a desktop whose browser hadn't come back, typed the site's address into nothing, and the sign-in they
/// meant to make was never made): the reopened browser started, and its window took the focus, both within the bound.
/// Real time over a short bound here; the reset's own is [`READY_WAIT`].
#[tokio::test]
async fn rt_i_055_a_reset_says_ready_only_when_the_browser_is_up_and_focused_in_time() {
    let launched = |after: Duration, pid: Option<u32>| {
        move || async move {
            tokio::time::sleep(after).await;
            pid.map(|pid| (pid, ()))
        }
    };
    let answering = |(), _left: Duration| async { true };
    let focus_after =
        |after: Option<Duration>| move |_pid: u32, left: Duration| fake_focus(after, left);
    let tick = Duration::from_millis(50);
    let bound = tick * 10;
    assert_eq!(READY_WAIT, Duration::from_secs(20), "the reset's bound");
    assert!(
        reopened(
            launched(tick, Some(7)),
            focus_after(Some(tick)),
            answering,
            bound
        )
        .await
        .ready,
        "started and focused at once: ready"
    );
    assert!(
        !reopened(
            launched(tick, None),
            focus_after(Some(tick)),
            answering,
            bound
        )
        .await
        .ready,
        "the browser didn't start: not ready"
    );
    assert!(
        !reopened(launched(tick, Some(7)), focus_after(None), answering, bound)
            .await
            .ready,
        "its window never took the focus: not ready"
    );
    // A slow launch under load is never cut off: dropping it mid-launch would leave its Chromium's helper processes on
    // the profile, untracked and unspared, and the next launch would meet the profile's lock (found on 18 Oct).
    // It runs to its end, so the session keeps the browser it started, and only the answer says not ready.
    let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let slow = {
        let finished = finished.clone();
        move || async move {
            tokio::time::sleep(bound * 2).await;
            finished.store(true, std::sync::atomic::Ordering::SeqCst);
            Some((7, ()))
        }
    };
    assert!(
        !reopened(slow, focus_after(Some(tick)), answering, bound)
            .await
            .ready,
        "a launch past the bound: not ready"
    );
    assert!(
        finished.load(std::sync::atomic::Ordering::SeqCst),
        "the launch ran to its end, never dropped part way"
    );
    // What the launch took is not given to the focus again.
    assert!(
        !reopened(
            launched(bound.saturating_sub(tick * 2), Some(7)),
            focus_after(Some(tick * 4)),
            answering,
            bound
        )
        .await
        .ready,
        "the focus gets only what the launch left of the bound"
    );
}

/// A window that takes the focus `after` (never when `None`), waited for at most `left`: whether it did in time.
async fn fake_focus(after: Option<Duration>, left: Duration) -> bool {
    let in_time = after.filter(|after| *after <= left);
    tokio::time::sleep(in_time.unwrap_or(left)).await;
    in_time.is_some()
}

/// A display whose window manager names the browser's window active (`getactivewindow`, window 2) while the X input
/// focus, where the VNC server's keys go, stays on its own window (1, no process) for the first `held` looks.
struct InputLags {
    held: usize,
    looked: std::sync::atomic::AtomicUsize,
}

#[async_trait]
impl Runner for InputLags {
    async fn run(&self, _program: &str, args: &[String]) -> Result<String, DriverError> {
        match (
            args.first().map(String::as_str),
            args.get(1).map(String::as_str),
        ) {
            (Some("getactivewindow"), _) => Ok("2\n".into()),
            (Some("getwindowfocus"), _) => {
                let looked = self
                    .looked
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(if looked < self.held { "1\n" } else { "2\n" }.into())
            }
            (Some("getwindowpid"), Some("2")) => Ok("4000\n".into()),
            (Some("getwindowpid"), _) => Err(DriverError::Unavailable("no pid".into())),
            (Some("getwindowname"), _) => Ok("window\n".into()),
            _ => Err(DriverError::Unavailable("unscripted".into())),
        }
    }
}

// TEST: RT-I-057
/// The browser's window is ready for keys only when it holds the X input focus too, not only the window manager's
/// active window (the 16v integration's `sec_059_a`: a reset said ready, control was plain, and the keys typed reached
/// no page). The window manager moves the input focus as a step of its own after naming the window active.
#[tokio::test]
async fn rt_i_057_the_focus_is_the_input_focus_not_only_the_active_window() -> TestResult {
    // The input focus follows after three looks: focused only then.
    let display = Arc::new(InputLags {
        held: 3,
        looked: std::sync::atomic::AtomicUsize::new(0),
    });
    let x = XDo::new(display.clone());
    assert!(focused(&x, 4_000, Duration::from_secs(5), Duration::from_millis(10)).await);
    assert!(
        display.looked.load(std::sync::atomic::Ordering::SeqCst) > 3,
        "the input focus was waited for"
    );
    // It never follows: not focused, at the bound.
    let display = Arc::new(InputLags {
        held: usize::MAX,
        looked: std::sync::atomic::AtomicUsize::new(0),
    });
    let x = XDo::new(display);
    assert!(
        !focused(
            &x,
            4_000,
            Duration::from_millis(200),
            Duration::from_millis(10)
        )
        .await,
        "active but never the input focus: not focused"
    );
    Ok(())
}

// TEST: RT-I-057
/// A reopened browser is ready only once its UI and page answer too, within the one bound, and the readiness says
/// what each step took and what ended it.
#[tokio::test]
async fn rt_i_057_a_browser_whose_ui_and_page_dont_answer_in_time_is_not_ready() {
    let tick = Duration::from_millis(50);
    let bound = tick * 10;
    let launched = || async { Some((7, "handle")) };
    let focus = |_pid: u32, _left: Duration| async { true };
    // Answers at once: ready, each step timed.
    let at_once = reopened(
        launched,
        focus,
        |_: &str, _left: Duration| async { true },
        bound,
    )
    .await;
    assert!(at_once.ready, "{at_once:?}");
    assert_eq!(at_once.ended, "ready");
    assert!(
        at_once.focus.is_some() && at_once.answers.is_some(),
        "{at_once:?}"
    );
    // Answers no (a page still loading, a window not shown as normal): not ready.
    let no = reopened(
        launched,
        focus,
        |_: &str, _left: Duration| async { false },
        bound,
    )
    .await;
    assert!(!no.ready, "{no:?}");
    assert_eq!(no.ended, "no_answer");
    // Answers only after what the bound left: not ready, and the step was given only that.
    let late = reopened(
        launched,
        focus,
        |handle: &str, left: Duration| async move {
            assert_eq!(handle, "handle", "the launch's handle is the one asked");
            let in_time = (tick * 20) <= left;
            tokio::time::sleep(left).await;
            in_time
        },
        bound,
    )
    .await;
    assert!(!late.ready, "{late:?}");
    assert_eq!(late.ended, "no_answer");
    // It was given only what the launch and the focus left, give or take the timer's slack.
    assert!(
        late.answers.is_some_and(|took| took < bound + tick),
        "{late:?}"
    );
}
