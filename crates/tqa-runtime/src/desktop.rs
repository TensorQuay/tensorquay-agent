//! The session's desktop (ADR-0039 slice 2): the runtime's own browser and the desktop driver over the display, made at
//! the first run of a session whose environment declares `desktop` and kept across its runs, as the MCP servers are.
//! The display stack itself is the image's (its init starts Xvfb, the window manager, the session bus and VNC); the
//! runtime only starts the browser, because it owns the browser's lifecycle (section 6).
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use crate::SupervisedGroups;
use rustix::process::{Pid, Signal};
use tokio::sync::Mutex;
use tqa_tools::builtin::computer::{
    a11y::{A11y, Unreachable, atspi_tree::AtspiTree},
    cdp::{Browser, profile_dir},
    desktop::{Desktop, OwnBrowser},
    driver::ScreenDriver,
    framebuffer::{Framebuffer, SCREEN_FILE},
    x11::{Process, XDo},
};
use tqa_types::SignOutFailure;

/// The Desktop's X server's socket (`DISPLAY=:0`, desktop-init's Xvfb): its clients are the display's applications.
pub(crate) const DISPLAY_SOCKET: &str = "/tmp/.X11-unix/X0";
/// Debian's Chromium, as the Desktop image installs it.
const CHROMIUM: &str = "/usr/bin/chromium";

/// The session's desktop driver, once made.
pub(crate) type Slot = Arc<Mutex<Option<Arc<dyn ScreenDriver>>>>;

/// The session's desktop driver, made on first use. It is kept only once the browser has started, so a browser that
/// failed to start (a slow first boot, say) is tried again at the next run; meanwhile the driver still reaches other
/// applications through their accessibility trees.
///
/// A browser it starts is recorded in `spared`, so the runtime's reaper leaves it alone (ADR-0039 section 10): before,
/// the first shell command of a desktop run killed it.
pub(crate) async fn driver(slot: &Slot, spared: &SupervisedGroups) -> Arc<dyn ScreenDriver> {
    Box::pin(opened(slot, spared)).await.0
}

/// [`driver`], with the browser's process and a handle on it when this call started it.
async fn opened(
    slot: &Slot,
    spared: &SupervisedGroups,
) -> (Arc<dyn ScreenDriver>, Option<(u32, Arc<Browser>)>) {
    let mut held = slot.lock().await;
    if let Some(driver) = held.as_ref() {
        return (driver.clone(), None);
    }
    let mut handle = None;
    let browser = match Browser::launch(Path::new(CHROMIUM), &profile_dir()).await {
        Ok(browser) => {
            let browser = Arc::new(browser);
            browser.pid().map(|pid| {
                handle = Some((pid, browser.clone()));
                OwnBrowser {
                    driver: browser,
                    pid,
                }
            })
        }
        Err(error) => {
            tracing::warn!(reason = %error, "desktop: the browser didn't start");
            None
        }
    };
    let tree: Arc<dyn A11y> = match AtspiTree::connect().await {
        Ok(tree) => Arc::new(tree),
        Err(error) => {
            tracing::warn!(reason = %error, "desktop: the accessibility bus isn't answering");
            Arc::new(Unreachable)
        }
    };
    let started = browser.as_ref().map(|browser| browser.pid);
    if let Some(pid) = started.and_then(|pid| i32::try_from(pid).ok()) {
        spared.browser(Some(pid));
    }
    let driver: Arc<dyn ScreenDriver> = Arc::new(Desktop::new(
        browser,
        XDo::new(Arc::new(Process)),
        tree,
        Some(Framebuffer::new(Path::new(SCREEN_FILE))),
    ));
    if started.is_some() {
        *held = Some(driver.clone());
    }
    (driver, handle)
}

/// Starts the session's browser again, blank, on its emptied profile (ADR-0039 section 6, the lead's fix of 7 Oct
/// 2026): after a run's end and after Sign out everywhere, so a Take over always finds the runtime's own browser, on the
/// profile the next run uses and the next reset empties. Without it the desktop had no browser between runs, and one
/// started by hand from the window manager's menu kept its sign-ins in the sandbox's home, out of every reset's reach.
///
/// It returns once the browser's window has the focus, or at [`READY_WAIT`]: its debugging port opens before its window
/// is mapped, and the reset this ends is what lets a Take over type. Keys sent before then went to no window, or to
/// the window manager (c16, 12 Oct 2026). Answers whether the browser is ready for a person's keys ([`reopened`]),
/// which the reset says (16r's `sec_059_a`: under load the launch outlasted the old 5 s focus wait, and a person
/// typed into a desktop with no browser yet). A browser already up (nothing emptied it) is ready.
pub(crate) async fn reopen(slot: &Slot, spared: &SupervisedGroups) -> bool {
    if slot.lock().await.is_some() {
        return true;
    }
    let readiness = reopened(
        // Boxed: the browser's launch is a large future, and this one is awaited inside the supervisor's.
        || async { Box::pin(opened(slot, spared)).await.1 },
        |pid, left| async move { focused(&XDo::new(Arc::new(Process)), pid, left, FOCUS_POLL).await },
        |browser: Arc<Browser>, left| async move { answering(&browser, left, FOCUS_POLL).await },
        READY_WAIT,
    )
    .await;
    let ms = |took: Option<Duration>| {
        took.map(|took| u64::try_from(took.as_millis()).unwrap_or(u64::MAX))
    };
    tracing::info!(
        ready = readiness.ready,
        ended = readiness.ended,
        launch_ms = ms(Some(readiness.launch)),
        focus_ms = ms(readiness.focus),
        answers_ms = ms(readiness.answers),
        "desktop: the browser started again"
    );
    readiness.ready
}

/// The most a run-end reset waits for the reopened browser, its launch and then its window taking the focus.
pub(crate) const READY_WAIT: Duration = Duration::from_secs(20);

/// How a reopened browser came up: whether it is ready for a person's keys, and what each step took (RT-I-057: one log
/// line a reopen says it, so a lost sign-in is read, not guessed).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Readiness {
    /// Ready for a person's keys.
    pub(crate) ready: bool,
    /// The launch, up to its debugging port answering.
    pub(crate) launch: Duration,
    /// The wait for its window to hold the focus, when the launch left time for it.
    pub(crate) focus: Option<Duration>,
    /// The wait for its UI and page to answer, when the focus came in time.
    pub(crate) answers: Option<Duration>,
    /// What ended it: `ready`, `no_browser`, `launch_past_bound`, `focus` or `no_answer`.
    pub(crate) ended: &'static str,
}

/// Whether the browser `launch` starts is ready for a person's keys within `bound` (RT-I-055, RT-I-057): it started,
/// its debugging port answering (`launch` gives its process and a handle on it), `focus` saw its window take the focus,
/// and `answers` saw its UI and page answer through that handle, each in what the steps before left of the bound. Not
/// ready when the launch failed or outlasted the bound, the window never took the focus, or the browser didn't answer.
/// The launch itself is never cut off: dropped part way, its Chromium's helper processes (a process group of their own)
/// would stay on the profile, untracked and unspared, and the next launch would meet the profile's lock (found on
/// 18 Oct 2026). It has its own bound for the debugging port, and runs to its end.
pub(crate) async fn reopened<L, Launched, H, F, Focused, A, Answered>(
    launch: L,
    focus: F,
    answers: A,
    bound: Duration,
) -> Readiness
where
    L: FnOnce() -> Launched,
    Launched: std::future::Future<Output = Option<(u32, H)>>,
    F: FnOnce(u32, Duration) -> Focused,
    Focused: std::future::Future<Output = bool>,
    A: FnOnce(H, Duration) -> Answered,
    Answered: std::future::Future<Output = bool>,
{
    let started = tokio::time::Instant::now();
    let launched = launch().await;
    let mut readiness = Readiness {
        ready: false,
        launch: started.elapsed(),
        focus: None,
        answers: None,
        ended: "no_browser",
    };
    let Some((pid, handle)) = launched else {
        return readiness;
    };
    let left = bound.saturating_sub(started.elapsed());
    if left.is_zero() {
        readiness.ended = "launch_past_bound";
        return readiness;
    }
    let focusing = tokio::time::Instant::now();
    let focused = focus(pid, left).await;
    readiness.focus = Some(focusing.elapsed());
    if !focused {
        readiness.ended = "focus";
        return readiness;
    }
    let left = bound.saturating_sub(started.elapsed());
    let asking = tokio::time::Instant::now();
    let answered = !left.is_zero() && answers(handle, left).await;
    readiness.answers = Some(asking.elapsed());
    if !answered {
        readiness.ended = "no_answer";
        return readiness;
    }
    readiness.ready = true;
    readiness.ended = "ready";
    readiness
}
/// How often it looks.
const FOCUS_POLL: Duration = Duration::from_millis(100);

/// Whether `pid`'s window has the focus within `bound`: the window manager's active window and the X input focus,
/// where the VNC server's keys go, both its (RT-I-057: the manager names a window active, then moves the input focus as
/// a step of its own). The time running out is only said.
async fn focused(x: &XDo, pid: u32, bound: Duration, poll: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + bound;
    loop {
        if let Ok(Some(active)) = x.active().await
            && active.pid == Some(pid)
            && x.input_focus().await.ok().flatten() == Some(pid)
        {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            tracing::warn!(
                pid,
                "desktop: the reopened browser's window didn't take the focus in time"
            );
            return false;
        }
        tokio::time::sleep(poll).await;
    }
}

/// Whether `browser` answers a person's keys ([`Browser::answers`]) within `bound`, asking every `poll`; the time
/// running out is only said.
async fn answering(browser: &Browser, bound: Duration, poll: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + bound;
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        if let Ok(Ok(true)) = tokio::time::timeout(left, browser.answers()).await {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            tracing::warn!("desktop: the reopened browser's window and page didn't answer in time");
            return false;
        }
        tokio::time::sleep(poll).await;
    }
}

/// How long a process holding the profile has to exit after SIGTERM before it gets SIGKILL.
pub(crate) const RELEASE_WAIT: Duration = Duration::from_secs(2);

/// Sign out everywhere (ADR-0039 section 6, `screen.sign_out`): closes the session's browser and empties its profile,
/// so every site it was signed in to is signed out; the next action starts the browser fresh. The slot stays locked
/// throughout, so nothing starts the browser again meanwhile. Any other process holding the profile (a browser started
/// by hand) is stopped ([`release`], with `wait` as its grace). A driver a run still holds, or a holder that survives
/// SIGKILL, is `profile_in_use`; a failure to remove it is `io_error`.
///
/// # Errors
/// The [`SignOutFailure`] the gateway records.
pub(crate) async fn sign_out(
    slot: &Slot,
    profile: &Path,
    wait: Duration,
    spared: &SupervisedGroups,
) -> Result<(), SignOutFailure> {
    let mut held = slot.lock().await;
    if held
        .as_ref()
        .is_some_and(|driver| Arc::strong_count(driver) > 1)
    {
        return Err(SignOutFailure::ProfileInUse);
    }
    // The browser's process is killed when its driver is dropped; the reaper no longer spares its pid.
    drop(held.take());
    spared.browser(None);
    release(profile, wait, &send).await?;
    match tokio::fs::remove_dir_all(profile).await {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(SignOutFailure::IoError),
        _ => Ok(()),
    }
}

/// How long a process still holding the profile after SIGKILL is waited for before the reset says it is in use.
const KILL_WAIT: Duration = Duration::from_secs(1);

/// Sends `signal` to `pid`; one already gone is no matter.
fn send(pid: Pid, signal: Signal) {
    if let Err(error) = rustix::process::kill_process(pid, signal) {
        tracing::debug!(pid = pid.as_raw_nonzero().get(), %error, "desktop: a signal wasn't sent");
    }
}

/// Stops every process holding `profile` (ADR-0039 section 6, 7 Oct 2026: fail safe): SIGTERM, `grace` to exit,
/// then SIGKILL. Only a holder that outlives SIGKILL leaves the profile in use, and each one is logged. `send` is the
/// signal sender ([`send`]; a test's own to model a holder no signal reaches).
///
/// # Errors
/// `profile_in_use` when a holder survives SIGKILL.
pub(crate) async fn release(
    profile: &Path,
    grace: Duration,
    send: &(dyn Fn(Pid, Signal) + Sync),
) -> Result<(), SignOutFailure> {
    for (signal, wait) in [(Signal::TERM, grace), (Signal::KILL, KILL_WAIT)] {
        let held = holders(profile);
        if held.is_empty() {
            return Ok(());
        }
        for (pid, _) in &held {
            send(*pid, signal);
        }
        let deadline = Instant::now() + wait;
        while !holders(profile).is_empty() && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    let survivors = holders(profile);
    for (pid, name) in &survivors {
        tracing::warn!(
            pid = pid.as_raw_nonzero().get(),
            name,
            "desktop: a process still holds the browser's profile after SIGKILL"
        );
    }
    if survivors.is_empty() {
        Ok(())
    } else {
        Err(SignOutFailure::ProfileInUse)
    }
}

/// Every process but this one whose command line names `profile` as its `--user-data-dir`, with its name.
fn holders(profile: &Path) -> Vec<(Pid, String)> {
    let wanted = format!("--user-data-dir={}", profile.display());
    let own = std::process::id();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let number: u32 = entry.file_name().to_str()?.parse().ok()?;
            let line = std::fs::read(entry.path().join("cmdline")).ok()?;
            let names = line
                .split(|byte| *byte == 0)
                .any(|argument| argument == wanted.as_bytes());
            if !names || number == own {
                return None;
            }
            let pid = Pid::from_raw(i32::try_from(number).ok()?)?;
            let name = std::fs::read_to_string(entry.path().join("comm")).unwrap_or_default();
            Some((pid, name.trim_end().to_owned()))
        })
        .collect()
}

#[cfg(test)]
#[path = "desktop_tests.rs"]
mod tests;
