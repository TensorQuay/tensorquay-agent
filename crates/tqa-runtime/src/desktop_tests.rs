//! GW-I-222's runtime half and SEC-059's Sign out everywhere (ADR-0039 section 6): the session's browser closed and its
//! profile emptied; a driver a run still holds, a process still naming the profile, and a profile that can't be
//! removed each refuse with their reason.
use super::{
    READY_WAIT, Slot, exited, focused, forget_dead, front_again, release, reopened, sign_out,
};
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
/// A process holding `profile` as the browser would: a shell whose own command line names it. The script ends in `:`
/// so the shell can't hand itself over to its last command (dash execs it, and the flag would leave the command line,
/// 15 Nov 2026: GW-I-222 saw no holder on CI), and the holder is returned only once `/proc` shows it.
async fn holder(profile: &std::path::Path, script: &str) -> std::io::Result<tokio::process::Child> {
    let child = tokio::process::Command::new("sh")
        .args([
            "-c",
            &format!("{script}; :"),
            &format!("--user-data-dir={}", profile.display()),
        ])
        .kill_on_drop(true)
        .spawn()?;
    let wanted = format!("--user-data-dir={}", profile.display());
    let pid = child
        .id()
        .ok_or_else(|| std::io::Error::other("the holder exited at once"))?;
    for _ in 0..100 {
        let line = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
        if line
            .split(|byte| *byte == 0)
            .any(|argument| argument == wanted.as_bytes())
        {
            return Ok(child);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    Err(std::io::Error::other("the holder never named the profile"))
}

// TEST: GW-I-222
// TEST: SEC-059
#[tokio::test]
async fn gw_i_222_a_process_holding_the_profile_is_stopped_and_the_profile_emptied() -> TestResult {
    let root = tempfile::tempdir()?;
    let profile = signed_in(root.path())?;
    let mut held = holder(&profile, "sleep 30").await?;
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
    let mut held = holder(&profile, "trap '' TERM; while :; do sleep 0.1; done").await?;
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
    let mut held = holder(&profile, "sleep 30").await?;
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

/// A child of this test that has exited but isn't reaped yet: a zombie, as the runtime's own dead browser is while its
/// handle is held.
fn zombie() -> Result<std::process::Child, Box<dyn std::error::Error>> {
    let child = std::process::Command::new("true").spawn()?;
    let pid = i32::try_from(child.id())?;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !exited(pid) {
        if std::time::Instant::now() > deadline {
            return Err("the child never exited".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(child)
}

// TEST: RT-I-061
/// ADR-0039 section 6, "started again if it died": a filled slot whose browser died is emptied, so the next start
/// launches the browser again; one whose browser lives, or whose driver a run still holds, is kept (104a's real-stack
/// case, 16 Nov 2026: a killed browser was never started again).
#[tokio::test]
async fn rt_i_061_a_browser_that_died_is_forgotten_so_it_starts_again() -> TestResult {
    let mut living = std::process::Command::new("sleep").arg("30").spawn()?;
    let mut dead = zombie()?;
    assert!(
        !exited(i32::try_from(living.id())?),
        "a running process hasn't exited"
    );
    assert!(exited(i32::try_from(dead.id())?), "a zombie has");
    assert!(exited(i32::MAX), "nor has a pid nobody has");

    let spared = SupervisedGroups::default();
    let slot: Slot = Arc::default();
    // Alive: kept, and still spared.
    spared.browser(Some(i32::try_from(living.id())?));
    *slot.lock().await = Some(Arc::new(Idle));
    forget_dead(&mut *slot.lock().await, &spared);
    assert!(
        slot.lock().await.is_some(),
        "a living browser's driver stays"
    );
    assert_eq!(spared.spared_browser(), Some(i32::try_from(living.id())?));

    // Dead, but a run holds the driver: kept until that run's end checks again.
    spared.browser(Some(i32::try_from(dead.id())?));
    let held: Arc<dyn ScreenDriver> = Arc::new(Idle);
    *slot.lock().await = Some(held.clone());
    forget_dead(&mut *slot.lock().await, &spared);
    assert!(
        slot.lock().await.is_some(),
        "a driver a run holds isn't taken from it"
    );
    drop(held);

    // Dead and free: forgotten, and its pid no longer spared, so the next start launches a new browser.
    forget_dead(&mut *slot.lock().await, &spared);
    assert!(
        slot.lock().await.is_none(),
        "the dead browser's driver is gone"
    );
    assert_eq!(spared.spared_browser(), None, "and its pid isn't spared");

    // No browser recorded: nothing to judge, the slot stays as it is.
    *slot.lock().await = Some(Arc::new(Idle));
    forget_dead(&mut *slot.lock().await, &spared);
    assert!(slot.lock().await.is_some());

    living.kill()?;
    living.wait()?;
    dead.wait()?;
    Ok(())
}

/// A display that records what it is asked: the window active before a reset, and each `windowactivate`.
struct Front {
    active: Option<(&'static str, Option<u32>)>,
    raised: std::sync::Mutex<Vec<String>>,
}

#[async_trait]
impl Runner for Front {
    async fn run(&self, _program: &str, args: &[String]) -> Result<String, DriverError> {
        match (args.first().map(String::as_str), self.active) {
            (Some("getactivewindow"), Some((id, _))) => Ok(format!("{id}\n")),
            (Some("getwindowpid"), Some((_, Some(pid)))) => Ok(format!("{pid}\n")),
            (Some("getwindowname"), Some(_)) => Ok("Mozilla Firefox\n".into()),
            (Some("windowactivate"), _) => {
                self.raised
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(args.last().cloned().unwrap_or_default());
                Ok(String::new())
            }
            _ => Err(DriverError::Unavailable("unscripted".into())),
        }
    }
}

// TEST: RT-I-066
/// A run-end reset gives the front back to the window the run left there (the owner's real use, 23 Nov 2026): the
/// agent left its own browser on a sign-in page for the person, the reset reopened the desktop's browser, which takes
/// the focus (RT-I-057), and its blank window covered the page. A window that was the desktop's own browser, and an
/// empty desktop, are left as the reopen made them.
#[tokio::test]
async fn rt_i_066_a_reset_gives_the_front_back_to_the_window_the_run_left_there() {
    let raised = |front: &Front| {
        front
            .raised
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    };
    // The agent's own window (another process than the desktop's browser, pid 700) was in front: it is again.
    let other = Arc::new(Front {
        active: Some(("31", Some(4242))),
        raised: std::sync::Mutex::new(Vec::new()),
    });
    let x = XDo::new(other.clone());
    let before = x.active().await.ok().flatten();
    assert!(front_again(&x, before.as_ref(), Some(700)).await);
    assert_eq!(raised(&other), ["31"]);
    // A window that names no process can't be told from the desktop's own browser (a getwindowpid that failed on its
    // window): it is left as the reopen made it (RT-I-067).
    let unnamed = Arc::new(Front {
        active: Some(("32", None)),
        raised: std::sync::Mutex::new(Vec::new()),
    });
    let x = XDo::new(unnamed.clone());
    let before = x.active().await.ok().flatten();
    assert!(!front_again(&x, before.as_ref(), Some(700)).await);
    assert!(raised(&unnamed).is_empty());
    // The desktop's own browser was in front: the reopened one keeps the front.
    let own = Arc::new(Front {
        active: Some(("33", Some(700))),
        raised: std::sync::Mutex::new(Vec::new()),
    });
    let x = XDo::new(own.clone());
    let before = x.active().await.ok().flatten();
    assert!(!front_again(&x, before.as_ref(), Some(700)).await);
    assert!(raised(&own).is_empty());
    // Nothing was in front: nothing is raised.
    assert!(!front_again(&x, None, Some(700)).await);
    assert!(raised(&own).is_empty());
}

/// A display whose windows can change between the read before a reset and the front's return: each window id names
/// its process (`_NET_WM_PID`) until the test says otherwise, as X reuses a gone client's ids for the next one.
#[derive(Default)]
struct Windows {
    active: std::sync::Mutex<Option<String>>,
    pids: std::sync::Mutex<std::collections::HashMap<String, u32>>,
    raised: std::sync::Mutex<Vec<String>>,
}

impl Windows {
    fn set(&self, id: &str, pid: Option<u32>) {
        let mut pids = self
            .pids
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match pid {
            Some(pid) => pids.insert(id.to_owned(), pid),
            None => pids.remove(id),
        };
    }
    fn front(&self, id: &str) {
        *self
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(id.to_owned());
    }
    fn raised(&self) -> Vec<String> {
        self.raised
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl Runner for Windows {
    async fn run(&self, _program: &str, args: &[String]) -> Result<String, DriverError> {
        let id = args.get(1).cloned().unwrap_or_default();
        match args.first().map(String::as_str) {
            Some("getactivewindow") => self
                .active
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
                .map(|id| format!("{id}\n"))
                .ok_or_else(|| DriverError::Unavailable("no window".into())),
            Some("getwindowpid") => self
                .pids
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(&id)
                .map(|pid| format!("{pid}\n"))
                .ok_or_else(|| DriverError::Unavailable("no such window".into())),
            Some("getwindowname") => Ok("A window\n".into()),
            Some("windowactivate") => {
                self.raised
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(args.last().cloned().unwrap_or_default());
                Ok(String::new())
            }
            _ => Err(DriverError::Unavailable("unscripted".into())),
        }
    }
}

// TEST: RT-I-067
/// The front goes back only to the very window the run left there (the 18.2.1 candidate's desktop-live sign-in case,
/// 24 Nov 2026: "what was typed under control went nowhere"). The window read before the reset is checked again after
/// it: if its id now names another process (X gave a gone client's ids to a new one), or no window at all, the
/// reopened browser keeps the front, so a Take over's keys reach it.
#[tokio::test]
async fn rt_i_067_the_front_goes_back_only_to_the_same_window_of_the_same_process() {
    // Still there, same process: brought back.
    let same = Arc::new(Windows::default());
    same.set("41", Some(4242));
    same.front("41");
    let x = XDo::new(same.clone());
    let before = x.active().await.ok().flatten();
    assert!(front_again(&x, before.as_ref(), Some(700)).await);
    assert_eq!(same.raised(), ["41"]);
    // The id now belongs to another process: left alone.
    let reused = Arc::new(Windows::default());
    reused.set("42", Some(4242));
    reused.front("42");
    let x = XDo::new(reused.clone());
    let before = x.active().await.ok().flatten();
    reused.set("42", Some(9001));
    assert!(!front_again(&x, before.as_ref(), Some(700)).await);
    assert!(reused.raised().is_empty());
    // The window is gone (its process closed it during the reset): left alone.
    let gone = Arc::new(Windows::default());
    gone.set("43", Some(4242));
    gone.front("43");
    let x = XDo::new(gone.clone());
    let before = x.active().await.ok().flatten();
    gone.set("43", None);
    assert!(!front_again(&x, before.as_ref(), Some(700)).await);
    assert!(gone.raised().is_empty());
    // The desktop's browser wasn't known: a window still named by its own living process is still brought back.
    let unknown = Arc::new(Windows::default());
    unknown.set("44", Some(4242));
    unknown.front("44");
    let x = XDo::new(unknown.clone());
    let before = x.active().await.ok().flatten();
    assert!(front_again(&x, before.as_ref(), None).await);
    assert_eq!(unknown.raised(), ["44"]);
}
