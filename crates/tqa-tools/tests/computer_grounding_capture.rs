//! The grounding evaluation's capture (docs/evals/computer-grounding.md section 3): on the Desktop image, each
//! synthetic page in the runtime's browser and each Mousepad state, captured once as the product sees it. For each
//! screen it writes the unmarked screen, the marked screenshot and its text exactly as the `computer` tool answers
//! them, and every target's box from the product's own marked snapshot (the page's DOM, or AT-SPI extents), so the
//! scorer's ground truth is the same geometry the marks are drawn on. Run in the desktop-probe image, never on the host:
//!
//! ```sh
//! python3 tests/e2e/grounding_eval/targets.py /eval
//! TQA_GROUNDING_DIR=/eval computer-grounding-capture --ignored --test-threads=1 --nocapture
//! ```
use std::{
    collections::BTreeMap,
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use tqa_tools::{
    Tool, ToolContext, ToolLimits, WorkspaceRoot,
    builtin::{
        Computer,
        computer::{
            View,
            a11y::atspi_tree::AtspiTree,
            cdp::{BROWSER_SCRATCH, Browser},
            desktop::{Desktop, OwnBrowser},
            driver::{RawElement, RawSnapshot, ScreenDriver, Source},
            framebuffer::{Framebuffer, SCREEN_FILE},
            raster::Rect,
            x11::{Process, XDo},
        },
    },
};
use tqa_types::{Clock, Sleeper};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct RealClock;
impl Clock for RealClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

struct RealSleeper;
impl Sleeper for RealSleeper {
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(tokio::time::sleep(duration))
    }
}

/// A route with images and good grounding, so the screenshot is the product's own and nothing is scaled.
fn view() -> View {
    View {
        vision: true,
        pixel_grounding: true,
        ..View::default()
    }
}

async fn act(
    tool: &Computer,
    cx: &ToolContext,
    fields: Value,
) -> Result<tqa_tools::ToolOutput, Box<dyn std::error::Error>> {
    let mut args = json!({"action": null, "ref": null, "text": null, "keys": null, "ms": null, "direction": null,
                          "amount": null, "x": null, "y": null, "to": null});
    if let (Some(args), Some(fields)) = (args.as_object_mut(), fields.as_object()) {
        for (name, value) in fields {
            args.insert(name.clone(), value.clone());
        }
    }
    Ok(tool.call(args, cx).await?)
}

/// An application's name as a person reads it: GTK's mnemonic underscores, a trailing ellipsis and the spaces it
/// pads a menu item's name with ("Open...      ", as Mousepad's AT-SPI tree has it) left out.
fn plain(name: &str) -> &str {
    name.trim_end()
        .trim_end_matches('…')
        .trim_end_matches("...")
        .trim_end()
}

/// The element a target names, as its ref and its box: by its exact role and name on a page, and in an application by
/// its role and its name as a person reads it ([`plain`]), never by a prefix: "New" must not also find "New Window".
/// Exactly one must match.
fn find(snapshot: &RawSnapshot, target: &Value, application: bool) -> Option<Value> {
    let (role, name) = (target["role"].as_str()?, target["name"].as_str()?);
    let matches: Vec<_> = snapshot
        .elements
        .iter()
        .enumerate()
        .filter(|(_, element)| element.role == role)
        .filter(|(_, element)| {
            if application {
                plain(&element.name.replace('_', "")) == name
            } else {
                element.name == name
            }
        })
        .collect();
    let [(index, only)] = matches.as_slice() else {
        return None;
    };
    let rect = only.rect?;
    Some(
        json!({"ref": format!("e{}", index + 1), "box": [rect.x, rect.y, rect.width, rect.height]}),
    )
}

/// A target's ground truth: its own element ([`find`]), or, for a target with no element of its own (a tool painted on
/// a canvas, a button drawn in an image), its box from its page's anchor heading: the heading's top-left plus the
/// target's `box_in_anchor` (x, y, width, height from it, in the screen pixels the capture's boxes are in: the
/// generator lays its pages out in CSS pixels, equal to screen pixels at the Desktop's 100 % zoom and device pixel
/// ratio 1), with no ref, since nothing in the snapshot is the target. The heading must be found once, as any target
/// is, and the box must have a size.
fn truth(snapshot: &RawSnapshot, target: &Value, application: bool) -> Option<Value> {
    let Some(anchor) = target.get("anchor") else {
        return find(snapshot, target, application);
    };
    let offset: Vec<i64> = target["box_in_anchor"]
        .as_array()?
        .iter()
        .map(Value::as_i64)
        .collect::<Option<_>>()?;
    let &[dx, dy, width, height] = offset.as_slice() else {
        return None;
    };
    if width <= 0 || height <= 0 {
        return None;
    }
    let heading = find(snapshot, anchor, application)?;
    let (x, y) = (heading["box"][0].as_i64()?, heading["box"][1].as_i64()?);
    Some(json!({"ref": null, "box": [x + dx, y + dy, width, height]}))
}

/// Why a target has no [`truth`]: for a canvas target whose heading is there, its `box_in_anchor` (a generator bug,
/// not a missing element); else what there was instead of the element, or of the heading ([`not_found`]).
fn miss(snapshot: &RawSnapshot, target: &Value, application: bool, id: &str) -> String {
    match target.get("anchor") {
        Some(anchor) if find(snapshot, anchor, application).is_some() => format!(
            "{id}: its heading is there, but its box_in_anchor {} isn't \
             [x, y, width, height] in whole pixels with a size: fix the generator",
            target["box_in_anchor"]
        ),
        Some(anchor) => not_found(snapshot, anchor, id),
        None => not_found(snapshot, target, id),
    }
}

/// What there was instead of a target [`find`] missed, so a failed capture says why: each element whose name holds the
/// target's, with its role and whether it has a box.
fn not_found(snapshot: &RawSnapshot, target: &Value, id: &str) -> String {
    let (role, name) = (
        target["role"].as_str().unwrap_or_default(),
        target["name"].as_str().unwrap_or_default(),
    );
    let near: Vec<String> = snapshot
        .elements
        .iter()
        .filter(|element| element.name.contains(name))
        .map(|element| {
            format!(
                "{} {:?} (box: {})",
                element.role,
                element.name,
                element.rect.is_some()
            )
        })
        .collect();
    format!("{id}: no {role} found once with a box; elements like it: {near:?}")
}

/// The capture's desktop, the tool over it, and what it has recorded so far.
struct Capture {
    browser: Arc<Browser>,
    driver: Arc<Desktop>,
    tool: Computer,
    cx: ToolContext,
    out: PathBuf,
    screens: Vec<Value>,
    boxes: BTreeMap<String, Value>,
}

impl Capture {
    /// The runtime's browser on the Desktop's display, the accessibility tree, and the tool as a grounding route has it.
    async fn start(out: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        std::fs::create_dir_all(&out)?;
        let profile = Path::new(BROWSER_SCRATCH).join("grounding");
        let _ = std::fs::remove_dir_all(&profile);
        let browser = Arc::new(Browser::launch(Path::new("/usr/bin/chromium"), &profile).await?);
        let pid = browser.pid().ok_or("the browser has no process")?;
        let driver = Arc::new(Desktop::new(
            Some(OwnBrowser {
                driver: browser.clone(),
                pid,
            }),
            XDo::new(Arc::new(Process)),
            Arc::new(AtspiTree::connect().await?),
            Some(Framebuffer::new(Path::new(SCREEN_FILE))),
        ));
        let workspace = std::env::temp_dir().join("tqa-grounding");
        std::fs::create_dir_all(&workspace)?;
        let cx = ToolContext {
            workspace: WorkspaceRoot::open(&workspace)?,
            cancel: CancellationToken::new(),
            clock: Arc::new(RealClock),
            sleeper: Arc::new(RealSleeper),
            env_allow: vec![],
            limits: ToolLimits::RECOMMENDED,
        };
        let tool = Computer::with_view(driver.clone(), view());
        Ok(Self {
            browser,
            driver,
            tool,
            cx,
            out,
            screens: Vec::new(),
            boxes: BTreeMap::new(),
        })
    }

    /// One screen: the unmarked PNG, the marked screenshot and its text, and the boxes of its targets.
    async fn screen(&mut self, screen: &str, targets: &[Value]) -> TestResult {
        let shot = act(&self.tool, &self.cx, json!({"action": "screenshot"})).await?;
        let image = shot.image.ok_or("the screenshot has no image")?;
        std::fs::write(self.out.join(format!("{screen}.marked.png")), &image.png)?;
        std::fs::write(
            self.out.join(format!("{screen}.png")),
            self.driver.screen().await?.png(),
        )?;
        let snapshot = self.driver.marked_snapshot().await?;
        let application = screen.starts_with("mousepad");
        for target in targets.iter().filter(|target| target["page"] == screen) {
            let id = target["id"].as_str().unwrap_or_default().to_owned();
            let found = truth(&snapshot, target, application)
                .ok_or_else(|| miss(&snapshot, target, application, &id))?;
            self.boxes.insert(id, found);
        }
        self.screens
            .push(json!({"screen": screen, "png": format!("{screen}.png"),
                                 "marked_png": format!("{screen}.marked.png"), "width": image.width,
                                 "height": image.height, "text": shot.text}));
        Ok(())
    }

    /// The `computer` tool as a route is offered it for each way, the product's own schema: by ref, a route without
    /// images; by mark, one with images; by coordinates, one with images and `pixel_grounding`. The runner sends these,
    /// so the evaluation never measures a copy that has drifted from the product.
    fn tools(&self) -> Value {
        let offered = |vision, pixel_grounding| {
            let tool = Computer::with_view(
                self.driver.clone(),
                View {
                    vision,
                    pixel_grounding,
                    ..View::default()
                },
            );
            let spec = tool.spec();
            json!({"name": spec.name.as_str(), "description": spec.description, "parameters": spec.parameters})
        };
        json!({"ref": offered(false, false), "mark": offered(true, false), "coordinates": offered(true, true)})
    }

    /// Opens Mousepad's File menu by its ref, as a person's click would.
    async fn open_file_menu(&self) -> TestResult {
        let looked = act(&self.tool, &self.cx, json!({"action": "observe"}))
            .await?
            .text;
        let file = looked
            .lines()
            .find(|line| line.contains("menu \"File\""))
            .and_then(|line| line.split_whitespace().next())
            .ok_or("no File menu")?
            .to_owned();
        act(
            &self.tool,
            &self.cx,
            json!({"action": "click", "ref": file}),
        )
        .await?;
        Ok(())
    }
}

// TEST: TOOL-U-047
#[tokio::test]
#[ignore = "the grounding evaluation's capture: runs in the desktop-probe image with TQA_GROUNDING_DIR"]
async fn capture_the_grounding_screens() -> TestResult {
    let dir = PathBuf::from(std::env::var("TQA_GROUNDING_DIR")?);
    let set: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("targets.json"))?)?;
    let targets = set["targets"].as_array().ok_or("no targets")?;
    let mut capture = Capture::start(dir.join("captures")).await?;
    let mut pages: Vec<&str> = targets
        .iter()
        .filter(|target| target["kind"] != "desktop")
        .filter_map(|target| target["page"].as_str())
        .collect();
    pages.dedup();
    for page in pages {
        let url = format!(
            "file://{}",
            dir.join("pages").join(format!("{page}.html")).display()
        );
        capture.browser.navigate(&url).await?;
        act(
            &capture.tool,
            &capture.cx,
            json!({"action": "wait", "ms": 1000}),
        )
        .await?;
        capture.screen(page, targets).await?;
    }
    // Mousepad: its menu bar, then its File menu open.
    let _editor = tokio::process::Command::new("mousepad")
        .kill_on_drop(true)
        .spawn()?;
    act(
        &capture.tool,
        &capture.cx,
        json!({"action": "wait", "ms": 3000}),
    )
    .await?;
    capture.screen("mousepad-main", targets).await?;
    capture.open_file_menu().await?;
    capture.screen("mousepad-file", targets).await?;
    assert_eq!(
        capture.boxes.len(),
        targets.len(),
        "every target has its box"
    );
    std::fs::write(
        capture.out.join("captures.json"),
        serde_json::to_string_pretty(&json!({"screens": capture.screens, "boxes": capture.boxes,
                                             "tools": capture.tools()}))?,
    )?;
    Ok(())
}

// TEST: TOOL-U-047
/// An application's targets match by their whole name as a person reads it, so a short one is still found once: the
/// first dry run (12 Oct 2026) found "New" three times in Mousepad's File menu (New, New Window, New From Template)
/// when names matched by prefix, and the capture stopped before writing captures.json.
#[test]
fn an_applications_target_matches_its_whole_name_not_a_prefix() {
    let item = |name: &str, x| RawElement {
        role: "menuitem".into(),
        name: name.into(),
        states: vec![],
        value: None,
        handle: name.into(),
        rect: Some(Rect {
            x,
            y: 10,
            width: 80,
            height: 20,
        }),
    };
    let snapshot = RawSnapshot {
        source: Source::A11y,
        window: "mousepad".into(),
        title: "Untitled 1 - Mousepad".into(),
        elements: vec![
            item("_New", 0),
            item("New _Window", 100),
            item("New From Template", 200),
            item("_Open...      ", 300),
            item("Open _Recent", 400),
            item("Save As…", 500),
        ],
    };
    let found = |name: &str| find(&snapshot, &json!({"role": "menuitem", "name": name}), true);
    assert_eq!(
        found("New"),
        Some(json!({"ref": "e1", "box": [0, 10, 80, 20]}))
    );
    assert_eq!(
        found("New Window").map(|hit| hit["ref"].clone()),
        Some(json!("e2"))
    );
    assert_eq!(
        found("Open").map(|hit| hit["ref"].clone()),
        Some(json!("e4"))
    );
    assert_eq!(
        found("Open Recent").map(|hit| hit["ref"].clone()),
        Some(json!("e5"))
    );
    assert_eq!(
        found("Save As").map(|hit| hit["ref"].clone()),
        Some(json!("e6"))
    );
    assert_eq!(found("Save"), None, "no element is Save itself");
    // On a page, names are exact: "Open…" isn't "Open".
    assert_eq!(
        find(
            &snapshot,
            &json!({"role": "menuitem", "name": "Open"}),
            false
        ),
        None
    );
}

// TEST: TOOL-U-071
/// A canvas target has no element of its own: its box is its page's anchor heading's top-left plus `box_in_anchor`, and
/// it has no ref. A target with an element is found as before, and a missing or doubled heading, or a box without a
/// size, gives no truth (so the capture stops and names the heading).
#[test]
fn a_canvas_targets_box_comes_from_its_anchor_heading() {
    let element = |role: &str, name: &str, x, y| RawElement {
        role: role.into(),
        name: name.into(),
        states: vec![],
        value: None,
        handle: name.into(),
        rect: Some(Rect {
            x,
            y,
            width: 300,
            height: 24,
        }),
    };
    let snapshot = |elements| RawSnapshot {
        source: Source::Dom,
        window: "w1".into(),
        title: "Painted toolbar".into(),
        elements,
    };
    let page = snapshot(vec![
        element("heading", "Painted toolbar (canvas-01)", 8, 8),
        element("button", "Help", 900, 8),
    ]);
    let canvas = json!({"id": "canvas-01-01", "kind": "canvas", "page": "canvas-01", "name": "redo arrow",
                        "anchor": {"role": "heading", "name": "Painted toolbar (canvas-01)"},
                        "box_in_anchor": [36, 52, 36, 36]});
    assert_eq!(
        truth(&page, &canvas, false),
        Some(json!({"ref": null, "box": [44, 60, 36, 36]}))
    );
    // A target with its own element: found as before, with its ref.
    let help = json!({"id": "x", "role": "button", "name": "Help"});
    assert_eq!(
        truth(&page, &help, false),
        Some(json!({"ref": "e2", "box": [900, 8, 300, 24]}))
    );
    // No heading, two headings, a box without a size or of the wrong shape: no truth.
    let headless = snapshot(vec![element("button", "Help", 900, 8)]);
    assert_eq!(truth(&headless, &canvas, false), None);
    let missing = miss(&headless, &canvas, false, "canvas-01-01");
    assert!(
        missing.starts_with("canvas-01-01: no heading found once"),
        "{missing}"
    );
    let twice = snapshot(vec![
        element("heading", "Painted toolbar (canvas-01)", 8, 8),
        element("heading", "Painted toolbar (canvas-01)", 8, 400),
    ]);
    assert_eq!(truth(&twice, &canvas, false), None);
    for bad in [
        json!([36, 52, 0, 36]),
        json!([36, 52, 36]),
        json!("36,52,36,36"),
    ] {
        let mut odd = canvas.clone();
        odd["box_in_anchor"] = bad.clone();
        assert_eq!(truth(&page, &odd, false), None, "{bad}");
        // The heading is there: the miss names the bad box, not the heading's near-matches.
        let named = miss(&page, &odd, false, "canvas-01-01");
        assert!(
            named.starts_with("canvas-01-01: its heading is there, but its box_in_anchor"),
            "{named}"
        );
    }
}
