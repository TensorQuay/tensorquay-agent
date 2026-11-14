//! The desktop driver tests' scripted display, accessibility tree and browser (shared by `desktop_tests` and
//! `desktop_frame_tests`): a move of the fakes out of the test file, no behaviour of their own.
use std::{
    collections::HashMap,
    sync::{Arc, Mutex as StdMutex},
};

use async_trait::async_trait;

use super::{
    a11y::{A11y, Node, Window},
    desktop::{Desktop, OwnBrowser},
    driver::{DriverError, ElementAct, RawElement, RawSnapshot, ScreenDriver, ScreenState, Source},
    x11::{Runner, XDOTOOL, XDo, XWININFO},
};

pub(super) type TestResult = Result<(), Box<dyn std::error::Error>>;

/// A display that answers `getactivewindow`, `getwindowpid`, `getwindowname` and `getmouselocation` (where the last
/// recorded move left the pointer, else the screen's centre; refused when `blind`), and records every other command.
pub(super) struct Display {
    pub(super) window: String,
    pub(super) pid: Option<u32>,
    pub(super) title: String,
    pub(super) commands: StdMutex<Vec<String>>,
    pub(super) pointer: StdMutex<(i32, i32)>,
    pub(super) blind: bool,
    /// The focused window's frame as xdotool's `getwindowgeometry` reports it; refused when `None`.
    pub(super) frame: Option<(i32, i32, u32, u32)>,
    /// The window's client area as xwininfo reports it (absolute origin, size); `frame` again when `None`.
    pub(super) client: Option<(i32, i32, u32, u32)>,
}

impl Display {
    pub(super) fn focused(window: &str, pid: Option<u32>, title: &str) -> Arc<Self> {
        Arc::new(Self {
            window: window.into(),
            pid,
            title: title.into(),
            commands: StdMutex::new(Vec::new()),
            pointer: StdMutex::new((640, 400)),
            blind: false,
            frame: None,
            client: None,
        })
    }

    /// A focused window whose frame xdotool reports at (x, y, width, height).
    pub(super) fn framed(
        window: &str,
        pid: u32,
        title: &str,
        frame: (i32, i32, u32, u32),
    ) -> Arc<Self> {
        Arc::new(Self {
            window: window.into(),
            pid: Some(pid),
            title: title.into(),
            commands: StdMutex::new(Vec::new()),
            pointer: StdMutex::new((640, 400)),
            blind: false,
            frame: Some(frame),
            client: None,
        })
    }

    /// A focused window under a window manager: xdotool reports `frame`, xwininfo the client area `client` (the
    /// window's real place on the screen).
    pub(super) fn managed(
        window: &str,
        pid: u32,
        title: &str,
        frame: (i32, i32, u32, u32),
        client: (i32, i32, u32, u32),
    ) -> Arc<Self> {
        Arc::new(Self {
            window: window.into(),
            pid: Some(pid),
            title: title.into(),
            commands: StdMutex::new(Vec::new()),
            pointer: StdMutex::new((640, 400)),
            blind: false,
            frame: Some(frame),
            client: Some(client),
        })
    }

    /// A display whose pointer can't be read.
    pub(super) fn blind() -> Arc<Self> {
        Arc::new(Self {
            window: "1".into(),
            pid: None,
            title: "Mousepad".into(),
            commands: StdMutex::new(Vec::new()),
            pointer: StdMutex::new((640, 400)),
            blind: true,
            frame: None,
            client: None,
        })
    }

    pub(super) fn input(&self) -> Vec<String> {
        self.commands
            .lock()
            .map(|commands| commands.clone())
            .unwrap_or_default()
    }
}

#[async_trait]
impl Runner for Display {
    async fn run(&self, program: &str, args: &[String]) -> Result<String, DriverError> {
        if program == XWININFO {
            // `xwininfo -id W`, as x11-utils prints it: the absolute client origin, the offset inside the frame.
            assert_eq!(args.first().map(String::as_str), Some("-id"));
            return self
                .client
                .or(self.frame)
                .map(|(x, y, width, height)| {
                    format!(
                        "\nxwininfo: Window id: {} \"Mousepad\"\n\n  Absolute upper-left X:  {x}\n  Absolute upper-left Y:  {y}\n  Relative upper-left X:  1\n  Relative upper-left Y:  20\n  Width: {width}\n  Height: {height}\n  Depth: 24\n",
                        self.window
                    )
                })
                .ok_or_else(|| DriverError::Unavailable("no geometry".into()));
        }
        assert_eq!(program, XDOTOOL);
        match args.first().map(String::as_str) {
            Some("getactivewindow") => Ok(format!("{}\n", self.window)),
            Some("getwindowpid") => self
                .pid
                .map(|pid| format!("{pid}\n"))
                .ok_or_else(|| DriverError::Unavailable("no pid".into())),
            Some("getwindowname") => Ok(format!("{}\n", self.title)),
            Some("getwindowgeometry") => self
                .frame
                .map(|(x, y, width, height)| {
                    format!(
                        "WINDOW={}\nX={x}\nY={y}\nWIDTH={width}\nHEIGHT={height}\nSCREEN=0\n",
                        self.window
                    )
                })
                .ok_or_else(|| DriverError::Unavailable("no geometry".into())),
            Some("getmouselocation") if self.blind => Err(DriverError::Unavailable(
                "the display refused the request".into(),
            )),
            Some("getmouselocation") => {
                let (x, y) = self.pointer.lock().map_or((0, 0), |at| *at);
                Ok(format!("X={x}\nY={y}\nSCREEN=0\nWINDOW=1\n"))
            }
            _ => {
                if let Ok(mut pointer) = self.pointer.lock() {
                    // Each move, synced or not, leaves the pointer at its point: "mousemove [--sync] X Y".
                    let words: Vec<&str> = args.iter().map(String::as_str).collect();
                    for (index, word) in words.iter().enumerate() {
                        if *word != "mousemove" {
                            continue;
                        }
                        let at = if words.get(index + 1) == Some(&"--sync") {
                            index + 2
                        } else {
                            index + 1
                        };
                        if let (Some(Ok(x)), Some(Ok(y))) = (
                            words.get(at).map(|word| word.parse()),
                            words.get(at + 1).map(|word| word.parse()),
                        ) {
                            *pointer = (x, y);
                        }
                    }
                }
                if let Ok(mut commands) = self.commands.lock() {
                    commands.push(args.join(" "));
                }
                Ok(String::new())
            }
        }
    }
}

/// A tree with one application window; each element's actions, editability and extents are scripted.
#[derive(Default)]
pub(super) struct Tree {
    pub(super) window: Option<Window>,
    pub(super) actions: HashMap<String, Vec<&'static str>>,
    pub(super) editable: Vec<String>,
    pub(super) focusable: Vec<String>,
    pub(super) extents: HashMap<String, (i32, i32, i32, i32)>,
    pub(super) done: StdMutex<Vec<String>>,
}

impl Tree {
    pub(super) fn done(&self) -> Vec<String> {
        self.done
            .lock()
            .map(|done| done.clone())
            .unwrap_or_default()
    }

    pub(super) fn record(&self, what: String) {
        if let Ok(mut done) = self.done.lock() {
            done.push(what);
        }
    }
}

#[async_trait]
impl A11y for Tree {
    async fn window(&self, _: u32) -> Result<Option<Window>, DriverError> {
        Ok(self.window.clone())
    }

    async fn act(&self, handle: &str, names: &[&str]) -> Result<bool, DriverError> {
        let found = self
            .actions
            .get(handle)
            .and_then(|actions| actions.iter().find(|name| names.contains(name)));
        if let Some(name) = found {
            self.record(format!("{name} {handle}"));
        }
        Ok(found.is_some())
    }

    async fn set_text(&self, handle: &str, text: &str) -> Result<bool, DriverError> {
        let editable = self.editable.iter().any(|known| known == handle);
        if editable {
            self.record(format!("set {handle} {text}"));
        }
        Ok(editable)
    }

    async fn focus(&self, handle: &str) -> Result<bool, DriverError> {
        Ok(self.focusable.iter().any(|known| known == handle))
    }

    async fn extents(&self, handle: &str) -> Result<Option<(i32, i32, i32, i32)>, DriverError> {
        Ok(self.extents.get(handle).copied())
    }
}

/// The runtime's browser, scripted: one page, one button.
#[derive(Default)]
pub(super) struct Page {
    pub(super) done: StdMutex<Vec<String>>,
}

#[async_trait]
impl ScreenDriver for Page {
    async fn snapshot(&self) -> Result<RawSnapshot, DriverError> {
        let save = RawElement {
            role: "button".into(),
            name: "Save".into(),
            states: vec![],
            value: None,
            handle: "9".into(),
            rect: None,
        };
        Ok(RawSnapshot {
            source: Source::Dom,
            window: "page-1".into(),
            title: "Form".into(),
            elements: vec![save],
        })
    }

    async fn state(&self) -> Result<ScreenState, DriverError> {
        Ok(ScreenState {
            window: "page-1".into(),
            title: "Form".into(),
            dialog: None,
            digest: 7,
        })
    }

    async fn act(&self, handle: &str, act: ElementAct) -> Result<(), DriverError> {
        self.record(format!("{handle} {act:?}"));
        Ok(())
    }

    async fn type_focused(&self, text: &str) -> Result<(), DriverError> {
        self.record(format!("type {text}"));
        Ok(())
    }

    async fn key(&self, keys: &[String]) -> Result<(), DriverError> {
        self.record(format!("key {}", keys.join("+")));
        Ok(())
    }
}

impl Page {
    pub(super) fn record(&self, what: String) {
        if let Ok(mut done) = self.done.lock() {
            done.push(what);
        }
    }

    pub(super) fn done(&self) -> Vec<String> {
        self.done
            .lock()
            .map(|done| done.clone())
            .unwrap_or_default()
    }
}

pub(super) fn node(
    role: &str,
    name: &str,
    states: &[&str],
    value: Option<&str>,
    handle: &str,
) -> Node {
    Node {
        role: role.into(),
        name: name.into(),
        states: states.iter().map(|state| (*state).to_owned()).collect(),
        value: value.map(str::to_owned),
        handle: handle.into(),
        extents: None,
    }
}

pub(super) fn mousepad() -> Tree {
    Tree {
        window: Some(Window {
            handle: ":1.5/frame".into(),
            title: "Untitled - Mousepad".into(),
            nodes: vec![
                node("menu", "File", &["sensitive"], None, ":1.5/menu"),
                node(
                    "text",
                    "",
                    &["sensitive", "focused", "editable"],
                    Some("hello"),
                    ":1.5/text",
                ),
                node("push button", "Save", &["sensitive"], None, ":1.5/save"),
                node("push button", "Print", &[], None, ":1.5/print"),
            ],
        }),
        ..Tree::default()
    }
}

pub(super) fn desktop(
    display: &Arc<Display>,
    tree: Tree,
    page: Option<&Arc<Page>>,
) -> (Desktop, Arc<Tree>) {
    let tree = Arc::new(tree);
    let browser = page.map(|page| OwnBrowser {
        driver: page.clone(),
        pid: 4_000,
    });
    (
        Desktop::new(browser, XDo::new(display.clone()), tree.clone(), None),
        tree,
    )
}
