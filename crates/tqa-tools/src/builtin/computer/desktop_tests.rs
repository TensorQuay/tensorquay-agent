//! The desktop driver without a desktop (ADR-0039 slice 2; TOOL-U-043, TOOL-U-044, TOOL-U-055, TOOL-U-057): an
//! application's accessibility tree read as a snapshot, refs routed by the focused window, keys and text through XTEST,
//! the XTEST fallback, the screen file's digest and an empty desktop with no window focused, against a scripted
//! display, tree and browser.
use std::{
    collections::HashMap,
    sync::{Arc, Mutex as StdMutex},
};

use async_trait::async_trait;

use super::{
    a11y::{A11y, Node, Window, elements},
    desktop::{Desktop, OwnBrowser},
    driver::{
        Direction, DriverError, ElementAct, RawElement, RawSnapshot, ScreenDriver, ScreenState,
        Source,
    },
    framebuffer::Framebuffer,
    x11::{Runner, XDOTOOL, XDo},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// A display that answers `getactivewindow`, `getwindowpid`, `getwindowname` and `getmouselocation` (where the last
/// recorded move left the pointer, else the screen's centre; refused when `blind`), and records every other command.
struct Display {
    window: String,
    pid: Option<u32>,
    title: String,
    commands: StdMutex<Vec<String>>,
    pointer: StdMutex<(i32, i32)>,
    blind: bool,
    /// The focused window's frame, for `getwindowgeometry`; refused when `None`.
    frame: Option<(i32, i32, u32, u32)>,
}

impl Display {
    fn focused(window: &str, pid: Option<u32>, title: &str) -> Arc<Self> {
        Arc::new(Self {
            window: window.into(),
            pid,
            title: title.into(),
            commands: StdMutex::new(Vec::new()),
            pointer: StdMutex::new((640, 400)),
            blind: false,
            frame: None,
        })
    }

    /// A focused window whose frame xdotool reports at (x, y, width, height).
    fn framed(window: &str, pid: u32, title: &str, frame: (i32, i32, u32, u32)) -> Arc<Self> {
        Arc::new(Self {
            window: window.into(),
            pid: Some(pid),
            title: title.into(),
            commands: StdMutex::new(Vec::new()),
            pointer: StdMutex::new((640, 400)),
            blind: false,
            frame: Some(frame),
        })
    }

    /// A display whose pointer can't be read.
    fn blind() -> Arc<Self> {
        Arc::new(Self {
            window: "1".into(),
            pid: None,
            title: "Mousepad".into(),
            commands: StdMutex::new(Vec::new()),
            pointer: StdMutex::new((640, 400)),
            blind: true,
            frame: None,
        })
    }

    fn input(&self) -> Vec<String> {
        self.commands
            .lock()
            .map(|commands| commands.clone())
            .unwrap_or_default()
    }
}

#[async_trait]
impl Runner for Display {
    async fn run(&self, program: &str, args: &[String]) -> Result<String, DriverError> {
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
struct Tree {
    window: Option<Window>,
    actions: HashMap<String, Vec<&'static str>>,
    editable: Vec<String>,
    focusable: Vec<String>,
    extents: HashMap<String, (i32, i32, i32, i32)>,
    done: StdMutex<Vec<String>>,
}

impl Tree {
    fn done(&self) -> Vec<String> {
        self.done
            .lock()
            .map(|done| done.clone())
            .unwrap_or_default()
    }

    fn record(&self, what: String) {
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
struct Page {
    done: StdMutex<Vec<String>>,
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
    fn record(&self, what: String) {
        if let Ok(mut done) = self.done.lock() {
            done.push(what);
        }
    }

    fn done(&self) -> Vec<String> {
        self.done
            .lock()
            .map(|done| done.clone())
            .unwrap_or_default()
    }
}

fn node(role: &str, name: &str, states: &[&str], value: Option<&str>, handle: &str) -> Node {
    Node {
        role: role.into(),
        name: name.into(),
        states: states.iter().map(|state| (*state).to_owned()).collect(),
        value: value.map(str::to_owned),
        handle: handle.into(),
        extents: None,
    }
}

fn mousepad() -> Tree {
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

fn desktop(display: &Arc<Display>, tree: Tree, page: Option<&Arc<Page>>) -> (Desktop, Arc<Tree>) {
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

// TEST: TOOL-U-043
#[test]
fn tool_u_043_an_applications_tree_reads_as_the_browsers_words() {
    let nodes = vec![
        node("push button", "OK", &["sensitive", "focused"], None, "a"),
        node(
            "check box",
            "Wrap lines",
            &["sensitive", "checked"],
            None,
            "b",
        ),
        node(
            "password text",
            "Password",
            &["sensitive"],
            Some("hunter2"),
            "c",
        ),
        node("filler", "", &["sensitive"], None, "d"),
        node("push button", "Apply", &[], None, "e"),
    ];
    let found: Vec<(String, String, Vec<String>, Option<String>)> = elements(&nodes)
        .into_iter()
        .map(|element| (element.role, element.name, element.states, element.value))
        .collect();
    assert_eq!(
        found,
        vec![
            ("button".into(), "OK".into(), vec!["focused".into()], None),
            (
                "checkbox".into(),
                "Wrap lines".into(),
                vec!["checked".into()],
                None
            ),
            (
                "textbox".into(),
                "Password".into(),
                vec!["password".into()],
                None
            ),
            (
                "button".into(),
                "Apply".into(),
                vec!["disabled".into()],
                None
            ),
        ],
        "a filler is left out, a password's value is never read, an insensitive button is disabled"
    );
}

// TEST: TOOL-U-043
#[tokio::test]
async fn tool_u_043_another_applications_window_is_its_accessibility_tree() -> TestResult {
    let display = Display::focused("52428807", Some(311), "Untitled - Mousepad");
    let page = Arc::new(Page::default());
    let (desktop, _) = desktop(&display, mousepad(), Some(&page));
    let snapshot = desktop.snapshot().await?;
    assert_eq!(
        (snapshot.source, snapshot.window.as_str()),
        (Source::A11y, "52428807")
    );
    assert_eq!(snapshot.title, "Untitled - Mousepad");
    let handles: Vec<&str> = snapshot
        .elements
        .iter()
        .map(|element| element.handle.as_str())
        .collect();
    assert_eq!(
        handles,
        [":1.5/menu", ":1.5/text", ":1.5/save", ":1.5/print"].map(|handle| format!("a:{handle}"))
    );
    assert_eq!(snapshot.elements[1].value.as_deref(), Some("hello"));
    Ok(())
}

// TEST: TOOL-U-043
#[tokio::test]
async fn tool_u_043_the_browsers_own_window_goes_to_its_driver() -> TestResult {
    let display = Display::focused("62914563", Some(4_000), "Form - Chromium");
    let page = Arc::new(Page::default());
    let (desktop, _) = desktop(&display, mousepad(), Some(&page));
    let snapshot = desktop.snapshot().await?;
    assert_eq!(
        (snapshot.source, snapshot.window.as_str()),
        (Source::Dom, "62914563/page-1")
    );
    assert_eq!(snapshot.elements[0].handle, "b:9");
    desktop.act("b:9", ElementAct::Click).await?;
    assert_eq!(page.done(), ["9 Click"]);
    // Keys are the display's, whatever has the focus (TOOL-U-057): the window manager's combinations too.
    desktop.key(&["alt".into(), "Tab".into()]).await?;
    assert_eq!(display.input(), ["key --clearmodifiers alt+Tab"]);
    Ok(())
}

// TEST: TOOL-U-057
#[tokio::test]
async fn tool_u_057_keys_and_text_go_to_the_display_with_the_browser_focused() -> TestResult {
    // A person's ctrl+l and a URL reach the browser's own address bar; through the page's CDP input they never did.
    let display = Display::focused("62914563", Some(4_000), "about:blank - Chromium");
    let page = Arc::new(Page::default());
    let (desktop, _) = desktop(&display, mousepad(), Some(&page));
    desktop.key(&["ctrl".into(), "l".into()]).await?;
    desktop.type_focused("http://192.0.2.10/form").await?;
    desktop.key(&["Return".into()]).await?;
    assert_eq!(
        display.input(),
        [
            "key --clearmodifiers ctrl+l",
            "type --clearmodifiers --delay 12 -- http://192.0.2.10/form",
            "key --clearmodifiers Return",
        ]
    );
    assert!(
        page.done().is_empty(),
        "no key or text through the page: {:?}",
        page.done()
    );
    Ok(())
}

// TEST: TOOL-U-057
#[tokio::test]
async fn tool_u_057_the_tool_takes_ctrl_l_a_url_and_return_to_the_address_bar() -> TestResult {
    // c16k real-world A5 and A7 (17 Oct): the agent's URL into the browser's address bar. Through the tool, not only the
    // driver: each step answers as done, and all of it goes to the display as a person's keys, none through the page.
    let display = Display::focused("62914563", Some(4_000), "httpbin.org/status/404 - Chromium");
    let page = Arc::new(Page::default());
    let (desktop, _) = desktop(&display, mousepad(), Some(&page));
    let tool = super::Computer::new(Arc::new(desktop));
    let directory = tempfile::tempdir()?;
    let (cx, _) = super::tests::context(&directory)?;
    let mut said = Vec::new();
    for fields in [
        serde_json::json!({"action": "key", "keys": "ctrl+l"}),
        serde_json::json!({"action": "type", "text": "https://httpbin.org/ip"}),
        serde_json::json!({"action": "key", "keys": "Return"}),
    ] {
        said.push(
            crate::Tool::call(&tool, super::tests::call(&fields), &cx)
                .await?
                .text,
        );
    }
    assert!(said[0].starts_with("Pressed ctrl+l"), "{said:?}");
    assert!(
        said[1].starts_with("Typed 22 characters into the focus"),
        "{said:?}"
    );
    assert!(said[2].starts_with("Pressed Return"), "{said:?}");
    let sent: Vec<String> = display
        .input()
        .into_iter()
        .filter(|command| command.starts_with("key ") || command.starts_with("type "))
        .collect();
    assert_eq!(
        sent,
        [
            "key --clearmodifiers ctrl+l",
            "type --clearmodifiers --delay 12 -- https://httpbin.org/ip",
            "key --clearmodifiers Return",
        ]
    );
    assert!(
        page.done().is_empty(),
        "nothing through the page: {:?}",
        page.done()
    );
    Ok(())
}

// TEST: TOOL-U-043
#[tokio::test]
async fn tool_u_043_an_elements_own_action_first_then_a_click_at_its_centre() -> TestResult {
    let display = Display::focused("1", Some(311), "Mousepad");
    let mut tree = mousepad();
    tree.actions.insert(":1.5/save".into(), vec!["click"]);
    tree.extents.insert(":1.5/menu".into(), (10, 20, 40, 18));
    let (desktop, tree) = desktop(&display, tree, None);
    desktop.act("a::1.5/save", ElementAct::Click).await?;
    assert_eq!(tree.done(), ["click :1.5/save"]);
    assert!(
        display.input().is_empty(),
        "the element's own action, no XTEST"
    );
    desktop.act("a::1.5/menu", ElementAct::Click).await?;
    desktop.act("a::1.5/menu", ElementAct::DoubleClick).await?;
    desktop
        .act("a::1.5/menu", ElementAct::Scroll(Direction::Down, 3))
        .await?;
    assert_eq!(
        display.input(),
        [
            "mousemove --sync 30 29 click --repeat 1 1",
            // The pointer is already there: no synced move, which would never return (TOOL-U-084).
            "click --repeat 2 1",
            "click --repeat 3 5",
        ]
    );
    let nowhere = desktop.act("a::1.5/print", ElementAct::Click).await;
    assert_eq!(
        nowhere,
        Err(DriverError::Failed(
            "the element has no place on the screen".into()
        ))
    );
    Ok(())
}

// TEST: TOOL-U-043
#[tokio::test]
async fn tool_u_043_text_is_set_else_typed_after_the_focus() -> TestResult {
    let display = Display::focused("1", Some(311), "Mousepad");
    let mut tree = mousepad();
    tree.editable.push(":1.5/text".into());
    tree.focusable.push(":1.5/menu".into());
    let (desktop, tree) = desktop(&display, tree, None);
    desktop
        .act("a::1.5/text", ElementAct::Type("Lovelace".into()))
        .await?;
    assert_eq!(tree.done(), ["set :1.5/text Lovelace"]);
    desktop
        .act("a::1.5/menu", ElementAct::Type("-n".into()))
        .await?;
    assert_eq!(
        display.input(),
        ["type --clearmodifiers --delay 12 -- -n"],
        "-- keeps a leading dash text"
    );
    desktop.type_focused("hi").await?;
    desktop.key(&["ctrl".into(), "s".into()]).await?;
    assert_eq!(
        &display.input()[1..],
        [
            "type --clearmodifiers --delay 12 -- hi",
            "key --clearmodifiers ctrl+s"
        ]
    );
    Ok(())
}

// TEST: TOOL-U-044
#[tokio::test]
async fn tool_u_044_a_handle_without_its_route_is_gone() {
    let display = Display::focused("1", Some(311), "Mousepad");
    let (desktop, _) = desktop(&display, mousepad(), None);
    assert_eq!(
        desktop.act(":1.5/save", ElementAct::Click).await,
        Err(DriverError::Gone)
    );
    let no_browser = desktop.act("b:9", ElementAct::Click).await;
    assert!(
        matches!(no_browser, Err(DriverError::Unavailable(_))),
        "{no_browser:?}"
    );
}

// TEST: TOOL-U-043
#[tokio::test]
async fn tool_u_043_the_screen_files_digest_moves_with_the_screen_and_titles_stand_in_without_it()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let file = directory.path().join("Xvfb_screen0");
    let screen = Framebuffer::new(&file);
    assert_eq!(screen.digest().await, None, "no file yet");
    std::fs::write(&file, b"frame one")?;
    let first = screen.digest().await;
    std::fs::write(&file, b"frame two")?;
    assert!(first.is_some() && first != screen.digest().await);

    let display = Display::focused("1", Some(311), "Mousepad");
    let (desktop, _) = desktop(&display, mousepad(), None);
    let state = desktop.state().await?;
    assert_eq!(
        (state.window.as_str(), state.title.as_str(), state.dialog),
        ("1", "Mousepad", None)
    );
    Ok(())
}

// TEST: TOOL-U-047
#[test]
fn tool_u_047_an_applications_boxes_are_its_screen_extents() {
    let mut placed = node("push button", "OK", &["sensitive"], None, "a");
    placed.extents = Some((5, 6, 70, 20));
    let mut flat = node("push button", "Cancel", &["sensitive"], None, "b");
    flat.extents = Some((5, 30, 0, 20));
    let unplaced = node("push button", "Help", &["sensitive"], None, "c");
    let rects: Vec<Option<super::raster::Rect>> = elements(&[placed, flat, unplaced])
        .into_iter()
        .map(|element| element.rect)
        .collect();
    assert_eq!(
        rects,
        vec![super::raster::Rect::new(5, 6, 70, 20), None, None],
        "a zero-size or unplaced node keeps its ref without a box"
    );
}

/// A display where no window has the focus: `getactivewindow` exits non-zero (or, `blank`, prints nothing), and
/// `getdisplaygeometry` answers unless the display is `gone`; every other command is recorded.
struct Unfocused {
    blank: bool,
    gone: bool,
    commands: StdMutex<Vec<String>>,
}

impl Unfocused {
    fn new(blank: bool, gone: bool) -> Arc<Self> {
        Arc::new(Self {
            blank,
            gone,
            commands: StdMutex::new(Vec::new()),
        })
    }

    fn input(&self) -> Vec<String> {
        self.commands
            .lock()
            .map(|commands| commands.clone())
            .unwrap_or_default()
    }
}

#[async_trait]
impl Runner for Unfocused {
    async fn run(&self, program: &str, args: &[String]) -> Result<String, DriverError> {
        assert_eq!(program, XDOTOOL);
        let refused = || DriverError::Unavailable("the display refused the request".into());
        match args.first().map(String::as_str) {
            _ if self.gone => Err(refused()),
            Some("getactivewindow") if self.blank => Ok("\n".into()),
            Some("getactivewindow") => Err(refused()),
            Some("getdisplaygeometry") => Ok("1280 800\n".into()),
            _ => {
                if let Ok(mut commands) = self.commands.lock() {
                    commands.push(args.join(" "));
                }
                Ok(String::new())
            }
        }
    }
}

// TEST: TOOL-U-055
#[tokio::test]
async fn tool_u_055_an_empty_desktop_is_seen_whole_with_no_elements_and_keys_go_to_the_display()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let file = directory.path().join("Xvfb_screen0");
    std::fs::write(&file, super::overlay_tests::xwd(64, 40, |_, _| 0x0030_6090))?;
    for blank in [false, true] {
        let display = Unfocused::new(blank, false);
        let page = Arc::new(Page::default());
        let browser = OwnBrowser {
            driver: page.clone(),
            pid: 4_000,
        };
        let desktop = Desktop::new(
            Some(browser),
            XDo::new(display.clone()),
            Arc::new(mousepad()),
            Some(Framebuffer::new(&file)),
        );
        // The screenshot's two halves: the snapshot (no window, so no elements to mark) and the whole screen.
        for snapshot in [desktop.snapshot().await?, desktop.marked_snapshot().await?] {
            assert_eq!(
                (
                    snapshot.window.as_str(),
                    snapshot.title.as_str(),
                    snapshot.elements.len()
                ),
                ("none", "no window has the focus", 0)
            );
        }
        let screen = desktop.screen().await?;
        assert_eq!(
            (screen.width, screen.height),
            (64, 40),
            "the root window, whole"
        );
        let state = desktop.state().await?;
        assert_eq!(
            (state.window.as_str(), state.title.as_str(), state.dialog),
            ("none", "no window has the focus", None)
        );
        assert_eq!(
            state.digest,
            desktop.state().await?.digest,
            "a still screen is settled"
        );
        desktop.key(&["ctrl".into(), "l".into()]).await?;
        desktop.type_focused("hello").await?;
        assert_eq!(
            display.input(),
            vec![
                "key --clearmodifiers ctrl+l",
                "type --clearmodifiers --delay 12 -- hello"
            ],
            "keys and text go to the display"
        );
        assert!(
            page.done.lock().is_ok_and(|done| done.is_empty()),
            "the browser got none of it"
        );
    }
    Ok(())
}

// TEST: TOOL-U-055
#[tokio::test]
async fn tool_u_055_a_display_that_doesnt_answer_is_still_unavailable() {
    let display = Unfocused::new(false, true);
    let desktop = Desktop::new(None, XDo::new(display.clone()), Arc::new(mousepad()), None);
    assert_eq!(
        desktop.snapshot().await,
        Err(DriverError::Unavailable(
            "the display refused the request".into()
        ))
    );
    assert!(matches!(
        desktop.state().await,
        Err(DriverError::Unavailable(_))
    ));
    assert!(
        display.input().is_empty(),
        "nothing typed into a display that isn't there"
    );
}

// TEST: TOOL-U-063
#[tokio::test]
async fn tool_u_063_a_browser_window_that_isnt_the_page_is_an_application() -> TestResult {
    // c16k real-world A12b (17 Oct 2026): ctrl+o opened GTK's Open File, a window of the browser's own process. Routed
    // by its pid, it read as the page: the page's refs, and in a screenshot the page's marks drawn over the dialog. Only
    // the page's window ("<title> - Chromium") is the page; any other is read as an application's.
    let display = Display::focused("71303171", Some(4_000), "Open File");
    let page = Arc::new(Page::default());
    let dialog = Tree {
        window: Some(Window {
            handle: ":1.9/dialog".into(),
            title: "Open File".into(),
            nodes: vec![
                node("push button", "Cancel", &["sensitive"], None, ":1.9/cancel"),
                node("push button", "Open", &["sensitive"], None, ":1.9/open"),
            ],
        }),
        ..Tree::default()
    };
    let (desktop, _) = desktop(&display, dialog, Some(&page));
    for snapshot in [desktop.snapshot().await?, desktop.marked_snapshot().await?] {
        assert_eq!(
            (
                snapshot.source,
                snapshot.window.as_str(),
                snapshot.title.as_str()
            ),
            (Source::A11y, "71303171", "Open File")
        );
        let handles: Vec<&str> = snapshot
            .elements
            .iter()
            .map(|element| element.handle.as_str())
            .collect();
        assert_eq!(handles, ["a::1.9/cancel", "a::1.9/open"]);
    }
    assert!(
        page.done().is_empty(),
        "the page wasn't asked: {:?}",
        page.done()
    );
    // The settle rule's state is the dialog's too, not the page's.
    assert_eq!(desktop.state().await?.window, "71303171");
    Ok(())
}

// TEST: TOOL-U-084
#[tokio::test]
async fn tool_u_084_input_at_the_pointers_own_place_skips_the_synced_move() -> TestResult {
    // xdotool's `mousemove --sync` waits for the pointer to move, so to the point it's already at it never returns:
    // the 10 s bound, then display_unavailable (16z: a button clicked twice, the wheel turned twice at one point).
    let display = Display::focused("1", Some(311), "Mousepad");
    let x = XDo::new(display.clone());
    x.click((100, 200), 1).await?;
    x.click((100, 200), 1).await?;
    x.wheel((100, 200), Direction::Down, 2).await?;
    x.click((150, 200), 2).await?;
    assert_eq!(
        display.input(),
        [
            "mousemove --sync 100 200 click --repeat 1 1",
            "click --repeat 1 1",
            "click --repeat 2 5",
            "mousemove --sync 150 200 click --repeat 2 1",
        ]
    );
    Ok(())
}

// TEST: TOOL-U-084
#[tokio::test]
async fn tool_u_084_a_drag_moves_only_where_the_pointer_isnt() -> TestResult {
    let display = Display::focused("1", Some(311), "Mousepad");
    let x = XDo::new(display.clone());
    // From the pointer's own place (the screen's centre) to a point beside it: the halfway point is the start.
    x.drag((640, 400), (641, 400)).await?;
    // From elsewhere to a point further on: every step moves.
    x.drag((100, 100), (300, 100)).await?;
    // Back to where it is, and a drag that ends where it starts.
    x.drag((300, 100), (300, 100)).await?;
    assert_eq!(
        display.input(),
        [
            "mousedown 1 mousemove --sync 641 400 mouseup 1",
            "mousemove --sync 100 100 mousedown 1 mousemove --sync 200 100 mousemove --sync 300 100 mouseup 1",
            "mousedown 1 mouseup 1",
        ]
    );
    Ok(())
}

// TEST: TOOL-U-084
#[tokio::test]
async fn tool_u_084_a_pointer_that_cant_be_read_moves_without_waiting() -> TestResult {
    // Where the pointer is isn't known, so a synced move might never return: the move goes unsynced, and XTEST still
    // delivers the click after it.
    let display = Display::blind();
    let x = XDo::new(display.clone());
    x.click((100, 200), 1).await?;
    x.wheel((100, 200), Direction::Up, 1).await?;
    assert_eq!(
        display.input(),
        [
            "mousemove 100 200 click --repeat 1 1",
            "mousemove 100 200 click --repeat 1 4",
        ]
    );
    Ok(())
}

/// An application whose accessibility side has stopped answering (Thunar's New Folder dialog and Ark's dialogs in
/// the agent eval, 6 Nov 2026: every screenshot waited 300 s, then failed).
struct Stalled;

#[async_trait]
impl A11y for Stalled {
    async fn window(&self, _: u32) -> Result<Option<Window>, DriverError> {
        std::future::pending().await
    }
    async fn act(&self, _: &str, _: &[&str]) -> Result<bool, DriverError> {
        std::future::pending().await
    }
    async fn set_text(&self, _: &str, _: &str) -> Result<bool, DriverError> {
        std::future::pending().await
    }
    async fn focus(&self, _: &str) -> Result<bool, DriverError> {
        std::future::pending().await
    }
    async fn extents(&self, _: &str) -> Result<Option<(i32, i32, i32, i32)>, DriverError> {
        std::future::pending().await
    }
}

// TEST: TOOL-U-138
#[tokio::test]
async fn tool_u_138_a_stalled_tree_never_holds_the_screen() -> TestResult {
    let display = Display::focused("52428807", Some(311), "Create New Folder");
    let desktop = Desktop::new(None, XDo::new(display.clone()), Arc::new(Stalled), None);
    let started = std::time::Instant::now();
    // The snapshot still comes, without the application's elements, within the window's bound (10 s).
    let bound = std::time::Duration::from_secs(20);
    let snapshot = tokio::time::timeout(bound, desktop.snapshot())
        .await
        .map_err(|_| "the snapshot waited on the stalled tree")??;
    assert_eq!(
        (snapshot.window.as_str(), snapshot.title.as_str()),
        ("52428807", "Create New Folder")
    );
    assert!(snapshot.elements.is_empty());
    assert!(
        started.elapsed() < std::time::Duration::from_secs(12),
        "{:?}",
        started.elapsed()
    );
    // An element's action is refused as quickly, so the agent goes on with the pointer and the keys.
    let started = std::time::Instant::now();
    let refused = tokio::time::timeout(bound, desktop.act("a::1.5/save", ElementAct::Click))
        .await
        .map_err(|_| "the click waited on the stalled tree")?;
    assert!(
        matches!(refused, Err(DriverError::Unavailable(_))),
        "{refused:?}"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    Ok(())
}

// TEST: TOOL-U-139
#[tokio::test]
async fn tool_u_139_a_tree_box_outside_its_window_draws_no_mark() -> TestResult {
    // A GTK4 window (Nautilus in the agent eval, 6 Nov 2026) at (390, 250): one button's box is where it is drawn,
    // two come back relative to the window and land in the screen's corner, where every mark piled up.
    let display = Display::framed("52428807", 311, "Documents", (390, 250, 890, 550));
    let placed = |handle: &str, extents| Node {
        extents: Some(extents),
        ..node("push button", handle, &["sensitive"], None, handle)
    };
    let tree = Tree {
        window: Some(Window {
            handle: ":1.5/frame".into(),
            title: "Documents".into(),
            nodes: vec![
                placed(":1.5/new", (1100, 262, 32, 32)),
                placed(":1.5/menu", (0, 12, 32, 32)),
                placed(":1.5/view", (4, 60, 120, 24)),
            ],
        }),
        ..Tree::default()
    };
    let (desktop, _) = desktop(&display, tree, None);
    let snapshot = desktop.snapshot().await?;
    let boxes: Vec<(&str, bool)> = snapshot
        .elements
        .iter()
        .map(|element| (element.name.as_str(), element.rect.is_some()))
        .collect();
    assert_eq!(
        boxes,
        [
            (":1.5/new", true),
            (":1.5/menu", false),
            (":1.5/view", false)
        ],
        "every element stays listed (its own action still works); only a box inside the window is drawn"
    );
    Ok(())
}

// TEST: TOOL-U-140
#[tokio::test]
async fn tool_u_140_line_breaks_and_tabs_are_keys_not_characters() -> TestResult {
    // xdotool types "\n" as Linefeed, which GTK's and LibreOffice's fields take as a character (the agent eval,
    // 6 Nov 2026: "the newline was typed as a character" in Calc's and GIMP's dialogs): a person presses Return.
    let display = Display::focused("52428807", Some(311), "Untitled - Mousepad");
    let mut editor = mousepad();
    editor.editable.push(":1.5/text".into());
    editor.focusable.push(":1.5/text".into());
    let (desktop, tree) = desktop(&display, editor, None);
    desktop.type_focused("name.txt\n").await?;
    desktop.type_focused("a\tb").await?;
    assert_eq!(
        display.input(),
        [
            "type --clearmodifiers --delay 12 -- name.txt",
            "key --clearmodifiers Return",
            "type --clearmodifiers --delay 12 -- a",
            "key --clearmodifiers Tab",
            "type --clearmodifiers --delay 12 -- b",
        ]
    );
    // Into an editable element: its value isn't set to text holding a line break (that would hold the break
    // itself); the element gets the focus and the keys.
    desktop
        .act("a::1.5/text", ElementAct::Type("done\n".into()))
        .await?;
    assert!(tree.done().is_empty(), "{:?}", tree.done());
    assert_eq!(
        &display.input()[5..],
        [
            "type --clearmodifiers --delay 12 -- done",
            "key --clearmodifiers Return"
        ]
    );
    Ok(())
}
