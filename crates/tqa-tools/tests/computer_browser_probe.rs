//! RT-I-046's browser cases (ADR-0039 slice 1): the `computer` tool driving a real headed Chromium on Xvfb. Built into
//! the `desktop-probe` image (images/runtime/Dockerfile) and run there on the spike's display, never on the host: each
//! case is ignored unless run with `--ignored`, as the image's entrypoint does. It prints the spike's measurements
//! (memory and settle times) for ADR-0039 section 10.
use std::{
    future::Future,
    io::Write as _,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use tqa_tools::{
    Tool, ToolContext, ToolError, ToolLimits, WorkspaceRoot,
    builtin::{Computer, computer::cdp::Browser},
};
use tqa_types::{Clock, Sleeper};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const PAGE: &str = r#"<!doctype html><html><head><title>Form</title></head><body>
<label>Name <input id="name"></label>
<button id="save" onclick="document.title='Saved '+document.getElementById('name').value">Save</button>
<button id="ask" onclick="alert('Sure?')">Ask</button>
<button id="open" onclick="window.open('about:blank','_blank')">Open</button>
<form onsubmit="document.title='Submitted';return false"><input id="query" aria-label="Query"></form>
<div style="height:3000px"></div>
</body></html>"#;

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

/// A delivery form with a time and a date input (A2, 17 Oct 2026: httpbin's pizza form's delivery time).
const TIME_PAGE: &str = r#"<!doctype html><html><head><title>Form</title></head><body>
<h1>Delivery</h1>
<label>Delivery time <input id="time" type="time" oninput="document.title='Time '+this.value"></label>
<label>Day <input id="day" type="date" oninput="document.title='Day '+this.value"></label>
</body></html>"#;

/// A fresh browser on the page, with its own profile, and the tool over it.
async fn open(name: &str) -> Result<(Computer, ToolContext, PathBuf), Box<dyn std::error::Error>> {
    open_on(name, PAGE).await
}

/// A fresh browser on `html`, with its own profile, and the tool over it.
async fn open_on(
    name: &str,
    html: &str,
) -> Result<(Computer, ToolContext, PathBuf), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!("tqa-probe-{name}"));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("workspace"))?;
    let page = root.join("form.html");
    std::fs::write(&page, html)?;
    // Each case its own profile, on the browser's own tmpfs as in a desktop sandbox (ADR-0039 section 10).
    let profile = Path::new(tqa_tools::builtin::computer::cdp::BROWSER_SCRATCH).join(name);
    let _ = std::fs::remove_dir_all(&profile);
    let browser = Browser::launch_at(
        Path::new("/usr/bin/chromium"),
        &profile,
        &format!("file://{}", page.display()),
    )
    .await?;
    let cx = ToolContext {
        workspace: WorkspaceRoot::open(root.join("workspace"))?,
        cancel: CancellationToken::new(),
        clock: Arc::new(RealClock),
        sleeper: Arc::new(RealSleeper),
        env_allow: vec![],
        limits: ToolLimits::RECOMMENDED,
    };
    // The page loads after the port opens: wait until its title is there.
    let tool = Computer::new(Arc::new(browser));
    for _ in 0..50 {
        if act(&tool, &cx, json!({"action": "observe"}))
            .await?
            .contains("Window: Form")
        {
            return Ok((tool, cx, root));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Err("the page didn't load".into())
}

async fn act(tool: &Computer, cx: &ToolContext, fields: Value) -> Result<String, ToolError> {
    let mut args = json!({"action": null, "ref": null, "text": null, "keys": null, "ms": null, "direction": null,
                          "amount": null});
    if let (Some(args), Some(fields)) = (args.as_object_mut(), fields.as_object()) {
        for (name, value) in fields {
            args.insert(name.clone(), value.clone());
        }
    }
    Ok(tool.call(args, cx).await?.text)
}

/// The ref of the first element whose snapshot line holds `needle` (`textbox "Name"`).
fn find(snapshot: &str, needle: &str) -> Result<String, Box<dyn std::error::Error>> {
    snapshot
        .lines()
        .find(|line| line.contains(needle))
        .and_then(|line| line.split_whitespace().next())
        .map(str::to_owned)
        .ok_or_else(|| format!("no element {needle} in:\n{snapshot}").into())
}

fn measure(what: &str, value: impl std::fmt::Display) {
    let _ = writeln!(std::io::stdout(), "MEASURE {what}: {value}");
}

/// The resident memory of every process whose name starts with one of `names`, in MiB.
fn resident_mib(names: &[&str]) -> u64 {
    let mut kib = 0;
    for entry in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        let status = std::fs::read_to_string(entry.path().join("status")).unwrap_or_default();
        let wanted_process = status
            .lines()
            .find_map(|line| line.strip_prefix("Name:"))
            .is_some_and(|name| names.iter().any(|wanted| name.trim().starts_with(wanted)));
        if wanted_process {
            kib += status
                .lines()
                .find_map(|line| line.strip_prefix("VmRSS:"))
                .and_then(|value| value.split_whitespace().next()?.parse::<u64>().ok())
                .unwrap_or(0);
        }
    }
    kib / 1024
}

// TEST: RT-I-046
#[tokio::test]
#[ignore = "runs in the desktop-probe image, on its display"]
async fn rt_i_046_refs_type_and_click_on_a_real_page() -> TestResult {
    let (tool, cx, _root) = open("type-click").await?;
    let snapshot = act(&tool, &cx, json!({"action": "observe"})).await?;
    assert!(
        snapshot.contains("<untrusted-content source=\"screen\""),
        "{snapshot}"
    );
    let name = find(&snapshot, "textbox \"Name\"")?;
    let save = find(&snapshot, "button \"Save\"")?;
    let started = Instant::now();
    let typed = act(
        &tool,
        &cx,
        json!({"action": "type", "ref": name, "text": "Ada"}),
    )
    .await?;
    measure("settle after type (ms)", started.elapsed().as_millis());
    assert!(typed.starts_with("Typed 3 characters into"), "{typed}");
    let started = Instant::now();
    let clicked = act(&tool, &cx, json!({"action": "click", "ref": save})).await?;
    measure(
        "settle after a click that retitles (ms)",
        started.elapsed().as_millis(),
    );
    assert!(
        clicked.contains("The window's title is now \"Saved Ada\""),
        "{clicked}"
    );
    measure(
        "resident memory, Xvfb and Chromium (MiB)",
        resident_mib(&["Xvfb", "chromium", "chrome"]),
    );
    Ok(())
}

// TEST: RT-I-046
#[tokio::test]
#[ignore = "runs in the desktop-probe image, on its display"]
async fn rt_i_046_a_dialog_is_shown_answered_and_ends_the_snapshot() -> TestResult {
    let (tool, cx, _root) = open("dialog").await?;
    let snapshot = act(&tool, &cx, json!({"action": "observe"})).await?;
    let ask = find(&snapshot, "button \"Ask\"")?;
    let save = find(&snapshot, "button \"Save\"")?;
    let opened = act(&tool, &cx, json!({"action": "click", "ref": ask})).await?;
    assert!(opened.contains("A dialog opened: \"Sure?\""), "{opened}");
    let stale = act(&tool, &cx, json!({"action": "click", "ref": save})).await;
    assert!(
        matches!(&stale, Err(error) if error.model_message().starts_with("stale_ref")),
        "{stale:?}"
    );
    let dialog = act(&tool, &cx, json!({"action": "observe"})).await?;
    assert!(
        dialog.contains("button \"OK\"") && dialog.contains("Window: Sure?"),
        "{dialog}"
    );
    let answered = act(&tool, &cx, json!({"action": "key", "keys": "Return"})).await?;
    assert!(answered.starts_with("Pressed Return"), "{answered}");
    let back = act(&tool, &cx, json!({"action": "observe"})).await?;
    assert!(back.contains("Window: Form"), "{back}");
    Ok(())
}

// TEST: RT-I-046
#[tokio::test]
#[ignore = "runs in the desktop-probe image, on its display"]
async fn rt_i_046_a_new_window_makes_old_refs_stale() -> TestResult {
    let (tool, cx, _root) = open("window").await?;
    let snapshot = act(&tool, &cx, json!({"action": "observe"})).await?;
    let open_button = find(&snapshot, "button \"Open\"")?;
    let save = find(&snapshot, "button \"Save\"")?;
    let opened = act(&tool, &cx, json!({"action": "click", "ref": open_button})).await?;
    assert!(opened.contains("A new window has the focus"), "{opened}");
    let stale = act(&tool, &cx, json!({"action": "click", "ref": save})).await;
    assert!(
        matches!(&stale, Err(error) if error.model_message()
            == "stale_ref: the focused window changed since the snapshot; observe again"),
        "{stale:?}"
    );
    Ok(())
}

// TEST: RT-I-046
#[tokio::test]
#[ignore = "runs in the desktop-probe image, on its display"]
async fn rt_i_046_scroll_and_return_reach_the_page() -> TestResult {
    let (tool, cx, _root) = open("scroll-key").await?;
    let snapshot = act(&tool, &cx, json!({"action": "observe"})).await?;
    let save = find(&snapshot, "button \"Save\"")?;
    let query = find(&snapshot, "textbox \"Query\"")?;
    let scrolled = act(
        &tool,
        &cx,
        json!({"action": "scroll", "ref": save, "direction": "down", "amount": 5}),
    )
    .await?;
    assert!(
        scrolled.ends_with("the screen changed."),
        "the page scrolled under the wheel: {scrolled}"
    );
    act(
        &tool,
        &cx,
        json!({"action": "type", "ref": query, "text": "tensor"}),
    )
    .await?;
    let submitted = act(&tool, &cx, json!({"action": "key", "keys": "Return"})).await?;
    assert!(
        submitted.contains("The window's title is now \"Submitted\""),
        "{submitted}"
    );
    Ok(())
}

// TEST: TOOL-U-059
#[tokio::test]
#[ignore = "runs in the desktop-probe image, on its display"]
async fn tool_u_059_a_time_and_a_date_are_typed_into_their_segments() -> TestResult {
    // A2 (17 Oct 2026): each `type` into the Hours, Minutes and AM/PM segments said "ok" and the field stayed --:-- --.
    let (tool, cx, _root) = open_on("time-segments", TIME_PAGE).await?;
    let snapshot = act(&tool, &cx, json!({"action": "observe"})).await?;
    // A5 (17 Oct 2026): a URL typed into the page's heading said ok six times. A heading takes no text: an error.
    let heading = find(&snapshot, "heading \"Delivery")?;
    let into_heading = act(
        &tool,
        &cx,
        json!({"action": "type", "ref": heading, "text": "https://httpbin.org/ip"}),
    )
    .await;
    assert!(
        matches!(&into_heading, Err(error) if error.model_message().contains("didn't change")),
        "{into_heading:?}"
    );
    for (segment, text) in [("Hours", "7"), ("Minutes", "30"), ("AM/PM", "P")] {
        let field = find(&snapshot, &format!("spinbutton \"{segment}"))?;
        let typed = act(
            &tool,
            &cx,
            json!({"action": "type", "ref": field, "text": text}),
        )
        .await?;
        assert!(typed.starts_with("Typed"), "{segment}: {typed}");
    }
    let shown = act(&tool, &cx, json!({"action": "observe"})).await?;
    assert!(
        shown.contains("Window: Time 19:30"),
        "the time input's value: {shown}"
    );
    let month = find(&shown, "spinbutton \"Month")?;
    let typed = act(
        &tool,
        &cx,
        json!({"action": "type", "ref": month, "text": "10172026"}),
    )
    .await?;
    assert!(typed.starts_with("Typed 8 characters"), "{typed}");
    let shown = act(&tool, &cx, json!({"action": "observe"})).await?;
    assert!(
        shown.contains("Window: Day 2026-10-17"),
        "the date input's value: {shown}"
    );
    Ok(())
}
