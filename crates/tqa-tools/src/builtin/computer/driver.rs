//! What the `computer` tool needs from a screen (ADR-0039 section 2): a snapshot of the focused window's actionable
//! elements, the screen's state for the settle rule, and input. The browser's debugging connection is one driver
//! (slice 1); AT-SPI and XTEST follow in slice 2. Tests use a scripted one.
use async_trait::async_trait;

use super::raster::{Raster, Rect};

/// Where a snapshot's elements came from, in the observe order (DOM refs first, then accessibility refs).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    /// The browser page's DOM and accessibility snapshot.
    Dom,
    /// The AT-SPI tree of another application.
    A11y,
}

impl Source {
    /// The spelling in the observe result.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dom => "dom",
            Self::A11y => "a11y",
        }
    }
}

/// One actionable element as the driver reads it; the tool gives it its ref.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawElement {
    /// Its accessibility role (`button`, `textbox`, `link`, ...).
    pub role: String,
    /// Its accessible name, possibly empty.
    pub name: String,
    /// States worth saying (`focused`, `checked`, `disabled`, `expanded`, ...).
    pub states: Vec<String>,
    /// Its current value (a text field's text, a slider's number), when it has one.
    pub value: Option<String>,
    /// The driver's own handle, opaque to the tool and never shown to the model.
    pub handle: String,
    /// Its box on the screen, in screen pixels, when it has one showing (slice 3's marks); never shown as numbers.
    pub rect: Option<Rect>,
}

/// The focused window's actionable elements.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawSnapshot {
    /// Where they came from.
    pub source: Source,
    /// The window it belongs to, as the driver identifies it; a different one ends the snapshot.
    pub window: String,
    /// The window's title (screen text, untrusted).
    pub title: String,
    /// The elements, in reading order.
    pub elements: Vec<RawElement>,
}

/// What the screen looks like now, cheaply: enough for the settle rule and to say what an action changed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScreenState {
    /// The focused window, as in [`RawSnapshot::window`].
    pub window: String,
    /// Its title (untrusted).
    pub title: String,
    /// The title of an open modal dialog, if one is open (untrusted).
    pub dialog: Option<String>,
    /// A digest of what is shown; equal digests mean nothing visible changed.
    pub digest: u64,
}

/// What an action does to an element.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ElementAct {
    /// Its press or activate action (a click at its centre as the fallback).
    Click,
    /// A double click.
    DoubleClick,
    /// Scrolled by notches in a direction.
    Scroll(Direction, u8),
    /// Its text set (focused, then typed, as the fallback).
    Type(String),
}

/// A page's file input waiting for files: its handle (as the driver routes it) and whether it takes several.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileInput {
    /// The driver's handle for the input, never shown to the model.
    pub handle: String,
    /// It takes several files.
    pub multiple: bool,
}

/// What a pointer action does at a screen point (the coordinate forms, slice 3).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PointerAct {
    /// A left click, once or twice.
    Click(u8),
    /// The wheel, by notches in a direction.
    Scroll(Direction, u8),
}

/// A scroll direction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    /// Towards the start.
    Up,
    /// Towards the end.
    Down,
    /// Leftwards.
    Left,
    /// Rightwards.
    Right,
}

/// Why the driver couldn't do something.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum DriverError {
    /// The element is no longer there.
    #[error("the element is no longer there")]
    Gone,
    /// The display, the browser or the accessibility bus isn't answering.
    #[error("the screen isn't answering: {0}")]
    Unavailable(String),
    /// The element's own action and the fallback both failed, with the reason (no screen text).
    #[error("the action failed: {0}")]
    Failed(String),
}

/// A screen the `computer` tool drives.
#[async_trait]
pub trait ScreenDriver: Send + Sync {
    /// The focused window's actionable elements.
    async fn snapshot(&self) -> Result<RawSnapshot, DriverError>;
    /// The same, with each element's box on the screen where the driver can measure it, for a marked screenshot.
    async fn marked_snapshot(&self) -> Result<RawSnapshot, DriverError> {
        self.snapshot().await
    }
    /// The screen's state now.
    async fn state(&self) -> Result<ScreenState, DriverError>;
    /// Acts on an element by its handle.
    async fn act(&self, handle: &str, act: ElementAct) -> Result<(), DriverError>;
    /// Types into whatever has the focus.
    async fn type_focused(&self, text: &str) -> Result<(), DriverError>;
    /// Presses one key combination, already validated (`ctrl`, `s`).
    async fn key(&self, keys: &[String]) -> Result<(), DriverError>;
    /// A pointer action at a screen point, through the display's input (XTEST), whatever window is there.
    async fn pointer(&self, at: (i32, i32), act: PointerAct) -> Result<(), DriverError> {
        let _ = (at, act);
        Err(DriverError::Unavailable(
            "this screen takes no pointer input".into(),
        ))
    }
    /// A drag between two screen points, through the display's input.
    async fn drag(&self, from: (i32, i32), to: (i32, i32)) -> Result<(), DriverError> {
        let _ = (from, to);
        Err(DriverError::Unavailable(
            "this screen takes no pointer input".into(),
        ))
    }
    /// The whole screen's pixels (slice 3). A driver without a display to read has none.
    async fn screen(&self) -> Result<Raster, DriverError> {
        Err(DriverError::Unavailable(
            "this screen has no pixels to read".into(),
        ))
    }
    /// The file input a page asked files for since the last call (an action opened its chooser), taken: the page
    /// shows no native dialog for it (c16k real-world A12b).
    async fn take_file_chooser(&self) -> Result<Option<FileInput>, DriverError> {
        Ok(None)
    }
    /// The element as a file input, when it is one.
    async fn file_input(&self, handle: &str) -> Result<Option<FileInput>, DriverError> {
        let _ = handle;
        Ok(None)
    }
    /// Gives a file input these files: absolute paths the tool has already checked are the workspace's own.
    async fn choose_files(
        &self,
        input: &FileInput,
        files: &[std::path::PathBuf],
    ) -> Result<(), DriverError> {
        let _ = (input, files);
        Err(DriverError::Failed("this screen has no file inputs".into()))
    }
}
