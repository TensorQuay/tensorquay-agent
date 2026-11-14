//! XTEST input and the focused window, through `xdotool` (ADR-0039 section 2: X11 because it reports exact global
//! positions and accepts XTEST input). The tool runs the program the Desktop image installs; a test runs a fake.
use std::sync::Arc;

use async_trait::async_trait;

use super::driver::{Direction, DriverError};

/// The program, as the Desktop image keeps it: off the sandbox user's PATH, the computer tool's (TOOL-U-064).
pub const XDOTOOL: &str = "/usr/libexec/tqa/xdotool";
/// `xwininfo` (x11-utils, in the Desktop image for the grounding capture): a window's real place on the screen.
pub const XWININFO: &str = "xwininfo";
/// Milliseconds between typed characters: slow enough for toolkits that drop fast input.
const TYPE_DELAY_MS: &str = "12";
/// xdotool's non-zero exit: the display refused the request, or (for `getactivewindow`) no window has the focus.
const REFUSED: &str = "the display refused the request";

/// Runs a program with arguments and answers its standard output.
#[async_trait]
pub trait Runner: Send + Sync {
    /// The program's output, or why it couldn't run or failed.
    async fn run(&self, program: &str, args: &[String]) -> Result<String, DriverError>;
}

/// The real runner: the program on the session's display, bounded in time.
pub struct Process;

#[async_trait]
impl Runner for Process {
    async fn run(&self, program: &str, args: &[String]) -> Result<String, DriverError> {
        let child = tokio::process::Command::new(program)
            .args(args)
            .kill_on_drop(true)
            .stdin(std::process::Stdio::null())
            .output();
        let output = tokio::time::timeout(std::time::Duration::from_secs(10), child)
            .await
            .map_err(|_| DriverError::Unavailable("the display didn't answer in time".into()))?
            .map_err(|error| {
                DriverError::Unavailable(format!("xdotool didn't run ({})", error.kind()))
            })?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(DriverError::Unavailable(REFUSED.into()))
        }
    }
}

/// The focused window: its X id, the process that owns it (when the window says), and its title.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActiveWindow {
    /// The X window id.
    pub id: String,
    /// `_NET_WM_PID`, when the window sets it.
    pub pid: Option<u32>,
    /// The window's title (screen text, untrusted).
    pub title: String,
}

/// The display's input and focused window.
#[derive(Clone)]
pub struct XDo {
    runner: Arc<dyn Runner>,
}

impl XDo {
    /// Over `runner` ([`Process`] in a sandbox).
    #[must_use]
    pub fn new(runner: Arc<dyn Runner>) -> Self {
        Self { runner }
    }

    async fn xdotool(&self, args: &[&str]) -> Result<String, DriverError> {
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        self.runner.run(XDOTOOL, &args).await
    }

    /// Where the pointer is, when the display says (`getmouselocation --shell`: `X=` and `Y=` lines).
    async fn pointer(&self) -> Option<(i32, i32)> {
        let text = self.xdotool(&["getmouselocation", "--shell"]).await.ok()?;
        let value = |key: &str| {
            text.lines()
                .find_map(|line| line.strip_prefix(key)?.trim().parse().ok())
        };
        Some((value("X=")?, value("Y=")?))
    }

    /// Runs pointer input: the moves to each point in `steps` the pointer isn't already at ([`move_to`]), each
    /// point's own words after its move, read from where the pointer is now.
    async fn pointer_input(&self, steps: &[((i32, i32), &[&str])]) -> Result<(), DriverError> {
        let mut at = self.pointer().await;
        let mut words = Vec::new();
        for (point, after) in steps {
            move_to(&mut words, &mut at, *point);
            words.extend(after.iter().map(|word| (*word).to_owned()));
        }
        self.runner
            .run(XDOTOOL, &words)
            .await
            .map(|_| ())
            .map_err(failed)
    }

    /// A window's client area on the screen (x, y, width, height), as `xwininfo -id` reports it, or `None` when it
    /// can't be read (then no box is judged against it). Not xdotool's `getwindowgeometry`: under a window manager
    /// it adds the window's offset inside its frame to the frame's position, so Mousepad's client area at (320, 167)
    /// read as (321, 187) and every element in the window's top 20 px, the menu bar first, lost its box (TOOL-U-142,
    /// 14 November 2026).
    pub async fn geometry(&self, id: &str) -> Option<(i32, i32, u32, u32)> {
        let text = self
            .runner
            .run(XWININFO, &["-id".to_owned(), id.to_owned()])
            .await
            .ok()?;
        let value = |key: &str| {
            text.lines()
                .find_map(|line| line.trim_start().strip_prefix(key)?.strip_prefix(':'))
                .map(str::trim)
        };
        Some((
            value("Absolute upper-left X")?.parse().ok()?,
            value("Absolute upper-left Y")?.parse().ok()?,
            value("Width")?.parse().ok()?,
            value("Height")?.parse().ok()?,
        ))
    }

    /// The focused window, or `None` when no window has the focus: the window manager names no active window until
    /// one exists, so an empty desktop has none (c16's integration: every action on it failed `display_unavailable`).
    ///
    /// # Errors
    /// `Unavailable` when the display doesn't answer.
    pub async fn active(&self) -> Result<Option<ActiveWindow>, DriverError> {
        let id = match self.xdotool(&["getactivewindow"]).await {
            Ok(id) => id.trim().to_owned(),
            // xdotool exits 1 alike for no active window and for a display it can't reach: a display that answers a
            // request no window manager takes part in only has nothing focused.
            Err(DriverError::Unavailable(reason)) if reason == REFUSED => {
                return match self.xdotool(&["getdisplaygeometry"]).await {
                    Ok(_) => Ok(None),
                    Err(_) => Err(DriverError::Unavailable(reason)),
                };
            }
            Err(error) => return Err(error),
        };
        if id.is_empty() {
            return Ok(None);
        }
        if !id.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(DriverError::Unavailable(
                "the display named no window".into(),
            ));
        }
        // A window without _NET_WM_PID makes getwindowpid fail: that only means it isn't the browser's.
        let pid = self
            .xdotool(&["getwindowpid", &id])
            .await
            .ok()
            .and_then(|text| text.trim().parse().ok());
        let title = self
            .xdotool(&["getwindowname", &id])
            .await
            .unwrap_or_default();
        Ok(Some(ActiveWindow {
            id,
            pid,
            title: title.trim_end_matches('\n').to_owned(),
        }))
    }

    /// The process whose window holds the X server's input focus, where keys from the VNC server go, or `None` when
    /// that window names no process (the window manager's, or no window). The window manager's active window
    /// ([`Self::active`]) names the window it means to focus; the input focus follows it as a separate step, so for a
    /// moment after a window maps the two can differ (the 16v integration's `sec_059_a`: keys typed then reached no
    /// page). `getwindowfocus` reports the client window, which carries `_NET_WM_PID`.
    ///
    /// # Errors
    /// `Unavailable` when the display doesn't answer.
    pub async fn input_focus(&self) -> Result<Option<u32>, DriverError> {
        let id = match self.xdotool(&["getwindowfocus"]).await {
            Ok(id) => id.trim().to_owned(),
            Err(DriverError::Unavailable(reason)) if reason == REFUSED => return Ok(None),
            Err(error) => return Err(error),
        };
        if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
            return Ok(None);
        }
        Ok(self
            .xdotool(&["getwindowpid", &id])
            .await
            .ok()
            .and_then(|text| text.trim().parse().ok()))
    }

    /// Clicks the left button `count` times at a screen point.
    ///
    /// # Errors
    /// `Failed` when the display refuses it.
    pub async fn click(&self, at: (i32, i32), count: u8) -> Result<(), DriverError> {
        let count = count.to_string();
        self.pointer_input(&[(at, &["click", "--repeat", &count, "1"])])
            .await
    }

    /// Turns the wheel over a screen point: buttons 4 and 5 scroll up and down, 6 and 7 left and right.
    ///
    /// # Errors
    /// `Failed` when the display refuses it.
    pub async fn wheel(
        &self,
        (x, y): (i32, i32),
        direction: Direction,
        notches: u8,
    ) -> Result<(), DriverError> {
        let button = match direction {
            Direction::Up => "4",
            Direction::Down => "5",
            Direction::Left => "6",
            Direction::Right => "7",
        };
        let notches = notches.to_string();
        self.pointer_input(&[((x, y), &["click", "--repeat", &notches, button])])
            .await
    }

    /// Drags with the left button held from one screen point to another, moving in steps so the application sees
    /// the motion a person's hand makes.
    ///
    /// # Errors
    /// `Failed` when the display refuses it.
    pub async fn drag(
        &self,
        (x, y): (i32, i32),
        (to_x, to_y): (i32, i32),
    ) -> Result<(), DriverError> {
        let middle = (i32::midpoint(x, to_x), i32::midpoint(y, to_y));
        self.pointer_input(&[
            ((x, y), &["mousedown", "1"]),
            (middle, &[]),
            ((to_x, to_y), &["mouseup", "1"]),
        ])
        .await
    }

    /// Presses one combination, already validated (`ctrl`, `s`), with the modifiers a person holds cleared first.
    ///
    /// # Errors
    /// `Failed` when the display refuses it.
    pub async fn key(&self, keys: &[String]) -> Result<(), DriverError> {
        let combination = keys.join("+");
        self.xdotool(&["key", "--clearmodifiers", &combination])
            .await
            .map(|_| ())
            .map_err(failed)
    }

    /// Types text into the focus as key events. `--` keeps text that starts with a dash from reading as a flag.
    ///
    /// # Errors
    /// `Failed` when the display refuses it.
    pub async fn type_text(&self, text: &str) -> Result<(), DriverError> {
        // A line break or a tab is the key a person presses, never xdotool's character for it: "\n" typed is
        // Linefeed, which GTK's and LibreOffice's fields take as a character (TOOL-U-140).
        let mut run = String::new();
        for character in text.chars() {
            let key = match character {
                '\n' => "Return",
                '\t' => "Tab",
                _ => {
                    run.push(character);
                    continue;
                }
            };
            self.type_run(&std::mem::take(&mut run)).await?;
            self.xdotool(&["key", "--clearmodifiers", key])
                .await
                .map_err(failed)?;
        }
        self.type_run(&run).await
    }

    async fn type_run(&self, text: &str) -> Result<(), DriverError> {
        if text.is_empty() {
            return Ok(());
        }
        self.xdotool(&[
            "type",
            "--clearmodifiers",
            "--delay",
            TYPE_DELAY_MS,
            "--",
            text,
        ])
        .await
        .map(|_| ())
        .map_err(failed)
    }
}

/// Adds the words that take the pointer from `at` to `to`, and notes it there: none when it's already there, since
/// `mousemove --sync` waits for a motion and to its own place none ever comes (16z: the 10 s bound, then
/// `display_unavailable`, for a button clicked twice); a synced move when where it is is known; an unsynced one when
/// it isn't, which XTEST still delivers before the input that follows.
fn move_to(words: &mut Vec<String>, at: &mut Option<(i32, i32)>, to: (i32, i32)) {
    if *at == Some(to) {
        return;
    }
    words.push("mousemove".into());
    if at.is_some() {
        words.push("--sync".into());
    }
    words.push(to.0.to_string());
    words.push(to.1.to_string());
    *at = Some(to);
}

/// Input the display refused is the action's failure; the display being gone stays `Unavailable`.
fn failed(error: DriverError) -> DriverError {
    match error {
        DriverError::Unavailable(reason) if reason == REFUSED => {
            DriverError::Failed("the display refused the input".into())
        }
        other => other,
    }
}
