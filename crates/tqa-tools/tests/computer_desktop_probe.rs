//! RT-I-046's AT-SPI cases (ADR-0039 slice 2): the `computer` tool over the desktop driver, on the Desktop image's
//! display with its window manager, session bus and accessibility service, driving Mousepad (GTK 3) beside the
//! runtime's browser, a GTK dialog and `LibreOffice` Writer's tool bars. And RT-I-047's set-of-marks cases (slice 3):
//! the boxes the marks are drawn on land where the elements are on the real screen, in the browser, in Mousepad and in
//! Writer and Calc, and a mark acts on its own element. Built into the `desktop-probe` image and run there, never on the host:
//! each case is ignored unless run with `--ignored`, as the image's entrypoint does.
use std::{
    future::Future,
    path::Path,
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use tqa_tools::{
    Tool, ToolContext, ToolError, ToolLimits, WorkspaceRoot,
    builtin::{
        Computer,
        computer::{
            View,
            a11y::atspi_tree::AtspiTree,
            cdp::{BROWSER_SCRATCH, Browser},
            desktop::{Desktop, OwnBrowser},
            driver::ScreenDriver,
            framebuffer::{Framebuffer, SCREEN_FILE},
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

/// The browser on a blank page, Mousepad opened after it (so it has the focus), and the tool over the whole desktop.
async fn desktop(
    name: &str,
) -> Result<(Computer, ToolContext, tokio::process::Child), Box<dyn std::error::Error>> {
    let (tool, cx, editor, _) = desktop_with(name, View::default()).await?;
    Ok((tool, cx, editor))
}

/// [`desktop`] with the route's `view`, and the driver itself, to read the boxes the marks are drawn on.
async fn desktop_with(
    name: &str,
    view: View,
) -> Result<(Computer, ToolContext, tokio::process::Child, Arc<Desktop>), Box<dyn std::error::Error>>
{
    let root = std::env::temp_dir().join(format!("tqa-desktop-{name}"));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("workspace"))?;
    let profile = Path::new(BROWSER_SCRATCH).join(name);
    let _ = std::fs::remove_dir_all(&profile);
    let browser = Browser::launch(Path::new("/usr/bin/chromium"), &profile).await?;
    let pid = browser.pid().ok_or("the browser has no process")?;
    let editor = tokio::process::Command::new("mousepad")
        .kill_on_drop(true)
        .spawn()?;
    let tree = AtspiTree::connect().await?;
    let driver = Desktop::new(
        Some(OwnBrowser {
            driver: Arc::new(browser),
            pid,
        }),
        XDo::new(Arc::new(Process)),
        Arc::new(tree),
        Some(Framebuffer::new(Path::new(SCREEN_FILE))),
    );
    let cx = ToolContext {
        workspace: WorkspaceRoot::open(root.join("workspace"))?,
        cancel: CancellationToken::new(),
        clock: Arc::new(RealClock),
        sleeper: Arc::new(RealSleeper),
        env_allow: vec![],
        limits: ToolLimits::RECOMMENDED,
    };
    let driver = Arc::new(driver);
    let tool = Computer::with_view(driver.clone(), view);
    for _ in 0..75 {
        if let Ok(snapshot) = act(&tool, &cx, json!({"action": "observe"})).await
            && snapshot.contains("Mousepad")
            && snapshot.contains("the window's accessibility tree")
        {
            return Ok((tool, cx, editor, driver));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Err("Mousepad's window didn't take the focus".into())
}

/// `LibreOffice`'s `app` (`--writer`, `--calc`) alone on the desktop, in its own profile so no other instance answers
/// for it, once its snapshot holds `ready`, and the tool over the desktop with the route's `view`. Its process is
/// `LibreOffice`'s own (`soffice.bin`), so dropping it ends the app. Its first start in a new profile ends with 81, asking
/// to be started again, as its launcher would: it is started once more.
async fn office(
    name: &str,
    app: &str,
    ready: &str,
    view: View,
) -> Result<(Computer, ToolContext, tokio::process::Child, Arc<Desktop>), Box<dyn std::error::Error>>
{
    let root = std::env::temp_dir().join(format!("tqa-desktop-{name}"));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("workspace"))?;
    let profile = format!(
        "-env:UserInstallation=file://{}",
        root.join("office-profile").display()
    );
    let start = || {
        tokio::process::Command::new("/usr/lib/libreoffice/program/soffice.bin")
            .args([app, "--norestore", "--nologo", &profile])
            .kill_on_drop(true)
            .spawn()
    };
    let mut office = start()?;
    let mut restarted = false;
    let driver = Arc::new(Desktop::new(
        None,
        XDo::new(Arc::new(Process)),
        Arc::new(AtspiTree::connect().await?),
        Some(Framebuffer::new(Path::new(SCREEN_FILE))),
    ));
    let cx = ToolContext {
        workspace: WorkspaceRoot::open(root.join("workspace"))?,
        cancel: CancellationToken::new(),
        clock: Arc::new(RealClock),
        sleeper: Arc::new(RealSleeper),
        env_allow: vec![],
        limits: ToolLimits::RECOMMENDED,
    };
    let tool = Computer::with_view(driver.clone(), view);
    let mut last = String::new();
    for _ in 0..300 {
        if !restarted
            && office
                .try_wait()?
                .is_some_and(|status| status.code() == Some(81))
        {
            office = start()?;
            restarted = true;
        }
        match act(&tool, &cx, json!({"action": "observe"})).await {
            Ok(snapshot) if snapshot.contains(ready) => return Ok((tool, cx, office, driver)),
            Ok(snapshot) => last = snapshot,
            Err(error) => last = error.model_message(),
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Err(format!("{app}'s window didn't show {ready}; the last look:\n{last}").into())
}

/// The snapshot line of the element `needle` names, as `find` matches it.
fn line_of<'a>(snapshot: &'a str, needle: &str) -> Result<&'a str, Box<dyn std::error::Error>> {
    snapshot
        .lines()
        .find(|line| line.contains(needle))
        .ok_or_else(|| format!("no element {needle} in:\n{snapshot}").into())
}

/// Whether the snapshot shows the element `needle` names as on (a toggle button pressed or checked).
fn is_on(snapshot: &str, needle: &str) -> bool {
    line_of(snapshot, needle).is_ok_and(|line| line.contains("pressed") || line.contains("checked"))
}

async fn act(tool: &Computer, cx: &ToolContext, fields: Value) -> Result<String, ToolError> {
    let mut args = json!({"action": null, "ref": null, "text": null, "keys": null, "ms": null, "direction": null,
                          "amount": null, "x": null, "y": null, "to": null});
    if let (Some(args), Some(fields)) = (args.as_object_mut(), fields.as_object()) {
        for (name, value) in fields {
            args.insert(name.clone(), value.clone());
        }
    }
    Ok(tool.call(args, cx).await?.text)
}

fn find(snapshot: &str, needle: &str) -> Result<String, Box<dyn std::error::Error>> {
    snapshot
        .lines()
        .find(|line| line.contains(needle))
        .and_then(|line| line.split_whitespace().next())
        .map(str::to_owned)
        .ok_or_else(|| format!("no element {needle} in:\n{snapshot}").into())
}

// TEST: RT-I-046
#[tokio::test]
#[ignore = "runs in the desktop-probe image, on its desktop"]
async fn rt_i_046_an_applications_text_is_set_through_its_tree() -> TestResult {
    let (tool, cx, _editor) = desktop("text").await?;
    let snapshot = act(&tool, &cx, json!({"action": "observe"})).await?;
    let text = find(&snapshot, "textbox ")?;
    let typed = act(
        &tool,
        &cx,
        json!({"action": "type", "ref": text, "text": "Ada Lovelace"}),
    )
    .await?;
    assert!(typed.starts_with("Typed 12 characters into"), "{typed}");
    let after = act(&tool, &cx, json!({"action": "observe"})).await?;
    assert!(
        after.contains("= \"Ada Lovelace\""),
        "the editor holds the text:\n{after}"
    );
    Ok(())
}

// TEST: RT-I-046
#[tokio::test]
#[ignore = "runs in the desktop-probe image, on its desktop"]
async fn rt_i_046_a_menu_opens_by_its_own_action() -> TestResult {
    let (tool, cx, _editor) = desktop("menu").await?;
    let snapshot = act(&tool, &cx, json!({"action": "observe"})).await?;
    let file = find(&snapshot, "menu \"File\"")?;
    act(&tool, &cx, json!({"action": "click", "ref": file})).await?;
    let opened = act(&tool, &cx, json!({"action": "observe"})).await?;
    assert!(
        opened.contains("menuitem \"Save"),
        "the File menu's items are listed:\n{opened}"
    );
    act(&tool, &cx, json!({"action": "key", "keys": "Escape"})).await?;
    Ok(())
}

// TEST: RT-I-046
#[tokio::test]
#[ignore = "runs in the desktop-probe image, on its desktop"]
async fn rt_i_046_switching_windows_routes_and_makes_old_refs_stale() -> TestResult {
    let (tool, cx, _editor) = desktop("switch").await?;
    let snapshot = act(&tool, &cx, json!({"action": "observe"})).await?;
    let text = find(&snapshot, "textbox ")?;
    let switched = act(&tool, &cx, json!({"action": "key", "keys": "alt+Tab"})).await?;
    assert!(
        switched.contains("A new window has the focus"),
        "{switched}"
    );
    let page = act(&tool, &cx, json!({"action": "observe"})).await?;
    assert!(
        page.contains("the page's structure"),
        "the browser's window goes to its driver:\n{page}"
    );
    let stale = act(
        &tool,
        &cx,
        json!({"action": "type", "ref": text, "text": "x"}),
    )
    .await;
    assert!(
        matches!(&stale, Err(error) if error.model_message().starts_with("stale_ref")),
        "{stale:?}"
    );
    Ok(())
}

// TEST: RT-I-046
#[tokio::test]
#[ignore = "runs in the desktop-probe image, on its desktop"]
async fn rt_i_046_a_dialogs_plain_buttons_are_listed_and_act_by_their_refs() -> TestResult {
    // GTK's push buttons are role "button" on at-spi2-core 2.56: Mousepad's Open dialog shows Cancel and Open.
    let (tool, cx, _editor) = desktop("dialog").await?;
    act(&tool, &cx, json!({"action": "key", "keys": "ctrl+o"})).await?;
    let mut dialog = String::new();
    for _ in 0..50 {
        dialog = act(&tool, &cx, json!({"action": "observe"})).await?;
        if dialog.contains("Open File") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let cancel = find(&dialog, "button \"Cancel\"")?;
    act(&tool, &cx, json!({"action": "click", "ref": cancel})).await?;
    let mut back = String::new();
    for _ in 0..50 {
        back = act(&tool, &cx, json!({"action": "observe"})).await?;
        if !back.contains("Open File") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(
        back.contains("Mousepad") && !back.contains("button \"Cancel\""),
        "Cancel's ref closed the dialog:\n{back}"
    );
    Ok(())
}

// TEST: RT-I-046
#[tokio::test]
#[ignore = "runs in the desktop-probe image, on its desktop"]
async fn rt_i_046_an_office_tool_bar_button_acts_by_its_ref() -> TestResult {
    // LibreOffice's tool bar items name their action only "press", for GetName; GetActions shows no names.
    let (tool, cx, _office, _driver) =
        office("office-ref", "--writer", "button \"Bold\"", View::default()).await?;
    let snapshot = act(&tool, &cx, json!({"action": "observe"})).await?;
    assert!(!is_on(&snapshot, "button \"Bold\""), "{snapshot}");
    // The root pane's hidden menu bar repeats every title and is never drawn: only the native one is listed.
    assert_eq!(
        snapshot.matches("menu \"File\"").count(),
        1,
        "File is listed once:\n{snapshot}"
    );
    let bold = find(&snapshot, "button \"Bold\"")?;
    act(&tool, &cx, json!({"action": "click", "ref": bold})).await?;
    let after = act(&tool, &cx, json!({"action": "observe"})).await?;
    assert!(
        is_on(&after, "button \"Bold\""),
        "Bold's ref turned Bold on:\n{after}"
    );
    Ok(())
}

// TEST: RT-I-046
#[tokio::test]
#[ignore = "runs in the desktop-probe image, on its desktop"]
async fn rt_i_046_an_applications_button_clicked_twice_by_coordinates_answers_each_time()
-> TestResult {
    // A coordinate click on any window goes through XTEST: Writer's Bold, twice at its box centre, turns on then off.
    let (tool, cx, _office, driver) = office(
        "repeat-app-click",
        "--writer",
        "button \"Bold\"",
        grounded(),
    )
    .await?;
    act(&tool, &cx, json!({"action": "screenshot"})).await?;
    let (x, y) = centre(&driver, "button \"Bold\"").await?;
    let mut states = Vec::new();
    for n in 1..=2 {
        let started = Instant::now();
        act(&tool, &cx, json!({"action": "click", "x": x, "y": y})).await?;
        let spent = started.elapsed();
        assert!(spent < Duration::from_secs(2), "click {n} took {spent:?}");
        let looked = act(&tool, &cx, json!({"action": "observe"})).await?;
        states.push(is_on(&looked, "button \"Bold\""));
    }
    assert_eq!(states, [true, false], "Bold turned on, then off");
    Ok(())
}

// TEST: RT-I-046
#[tokio::test]
#[ignore = "runs in the desktop-probe image, on its desktop"]
async fn rt_i_046_an_applications_element_scrolled_twice_answers_each_time() -> TestResult {
    // An application element's scroll is the wheel at its centre, through XTEST: Mousepad's text, twice.
    let (tool, cx, _editor) = desktop("repeat-app-wheel").await?;
    let snapshot = act(&tool, &cx, json!({"action": "observe"})).await?;
    let text = find(&snapshot, "textbox ")?;
    for n in 1..=2 {
        let started = Instant::now();
        act(
            &tool,
            &cx,
            json!({"action": "scroll", "ref": text, "direction": "down", "amount": 1}),
        )
        .await?;
        let spent = started.elapsed();
        assert!(spent < Duration::from_secs(2), "scroll {n} took {spent:?}");
    }
    Ok(())
}

/// A route with images that grounds well, so the probe can click a box's centre by its pixels.
fn grounded() -> View {
    View {
        vision: true,
        pixel_grounding: true,
        ..View::default()
    }
}

/// The centre of the element whose snapshot line holds `needle`, from the driver's own marked snapshot.
async fn centre(driver: &Desktop, needle: &str) -> Result<(i64, i64), Box<dyn std::error::Error>> {
    let snapshot = driver.marked_snapshot().await?;
    let element = snapshot
        .elements
        .iter()
        .find(|element| format!("{} \"{}\"", element.role, element.name).contains(needle))
        .ok_or_else(|| format!("no element {needle}"))?;
    Ok(element
        .rect
        .ok_or_else(|| format!("{needle} has no box"))?
        .centre())
}

/// The browser alone on `page`, so its window has the focus, with the tool over the desktop.
async fn page_in_front(
    name: &str,
    page: &str,
) -> Result<(Computer, ToolContext, Arc<Desktop>), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!("tqa-desktop-{name}"));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("workspace"))?;
    let profile = Path::new(BROWSER_SCRATCH).join(name);
    let _ = std::fs::remove_dir_all(&profile);
    let url = format!("data:text/html,{}", urlencode(page));
    let browser = Browser::launch_at(Path::new("/usr/bin/chromium"), &profile, &url).await?;
    let pid = browser.pid().ok_or("the browser has no process")?;
    let driver = Arc::new(Desktop::new(
        Some(OwnBrowser {
            driver: Arc::new(browser),
            pid,
        }),
        XDo::new(Arc::new(Process)),
        Arc::new(AtspiTree::connect().await?),
        Some(Framebuffer::new(Path::new(SCREEN_FILE))),
    ));
    let cx = ToolContext {
        workspace: WorkspaceRoot::open(root.join("workspace"))?,
        cancel: CancellationToken::new(),
        clock: Arc::new(RealClock),
        sleeper: Arc::new(RealSleeper),
        env_allow: vec![],
        limits: ToolLimits::RECOMMENDED,
    };
    let tool = Computer::with_view(driver.clone(), grounded());
    for _ in 0..50 {
        if let Ok(looked) = act(&tool, &cx, json!({"action": "observe"})).await
            && looked.contains("Marks probe")
        {
            return Ok((tool, cx, driver));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Err("the probe page didn't load".into())
}

fn urlencode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

const PAGE: &str = "<title>Marks probe</title><body style='margin:40px'>\
    <button id=a style='margin:30px' onclick=\"document.title='clicked a'\">Alpha</button>\
    <button id=b style='margin:30px' onclick=\"document.title='clicked b'\">Bravo</button>\
    <span style='position:relative;left:-90px;top:-20px;background:#c00;color:#fff'>1</span></body>";

/// A button that counts its clicks in the title, and a scrolling box below it that writes its scroll offset there.
const REPEAT: &str = "<title>Marks probe</title><body style='margin:40px'>\
    <button style='width:160px;height:60px' onclick=\"window.n=(window.n||0)+1;document.title='Marks probe clicks '+window.n\">Again</button>\
    <div style='margin-top:40px;width:300px;height:200px;overflow:auto' \
    onscroll=\"document.title='Marks probe scrolled '+this.scrollTop\"><div style='height:4000px'>Rows</div></div></body>";

// TEST: RT-I-046
#[tokio::test]
#[ignore = "runs in the desktop-probe image, on its desktop"]
async fn rt_i_046_the_same_point_clicked_twice_answers_each_time() -> TestResult {
    // xdotool's synced move to where the pointer already is never returns (16z: the 10 s bound, display_unavailable).
    let (tool, cx, driver) = page_in_front("repeat-click", REPEAT).await?;
    act(&tool, &cx, json!({"action": "screenshot"})).await?;
    let (x, y) = centre(&driver, "button \"Again\"").await?;
    for n in 1..=2 {
        let started = Instant::now();
        act(&tool, &cx, json!({"action": "click", "x": x, "y": y})).await?;
        let spent = started.elapsed();
        assert!(spent < Duration::from_secs(2), "click {n} took {spent:?}");
    }
    let after = act(&tool, &cx, json!({"action": "observe"})).await?;
    assert!(
        after.contains("Marks probe clicks 2"),
        "both clicks reached the button:\n{after}"
    );
    Ok(())
}

// TEST: RT-I-046
#[tokio::test]
#[ignore = "runs in the desktop-probe image, on its desktop"]
async fn rt_i_046_the_wheel_turned_twice_at_one_point_answers_each_time() -> TestResult {
    let (tool, cx, driver) = page_in_front("repeat-wheel", REPEAT).await?;
    act(&tool, &cx, json!({"action": "screenshot"})).await?;
    // The box sits 40 px under the 60 px button: 170 px below the button's centre is inside it.
    let (x, button_y) = centre(&driver, "button \"Again\"").await?;
    let y = button_y + 170;
    let mut titles = Vec::new();
    for n in 1..=2 {
        let started = Instant::now();
        act(
            &tool,
            &cx,
            json!({"action": "scroll", "x": x, "y": y, "direction": "down", "amount": 1}),
        )
        .await?;
        let spent = started.elapsed();
        assert!(spent < Duration::from_secs(2), "turn {n} took {spent:?}");
        titles.push(
            line_of(
                &act(&tool, &cx, json!({"action": "observe"})).await?,
                "Marks probe",
            )?
            .to_owned(),
        );
    }
    assert!(
        titles[0].contains("scrolled") && titles[1].contains("scrolled") && titles[0] != titles[1],
        "both turns scrolled the box: {titles:?}"
    );
    Ok(())
}

// TEST: RT-I-047
#[tokio::test]
#[ignore = "runs in the desktop-probe image, on its desktop"]
async fn rt_i_047_a_pages_boxes_land_on_its_elements_on_the_screen() -> TestResult {
    let (tool, cx, driver) = page_in_front("marks-page", PAGE).await?;
    let shot = act(&tool, &cx, json!({"action": "screenshot"})).await?;
    assert!(shot.starts_with("Screenshot 1280×800"), "{shot}");
    let (x, y) = centre(&driver, "button \"Bravo\"").await?;
    act(&tool, &cx, json!({"action": "click", "x": x, "y": y})).await?;
    let after = act(&tool, &cx, json!({"action": "observe"})).await?;
    assert!(
        after.contains("clicked b"),
        "a click at Bravo's box centre lands on Bravo:\n{after}"
    );
    Ok(())
}

// TEST: RT-I-047
#[tokio::test]
#[ignore = "runs in the desktop-probe image, on its desktop"]
async fn rt_i_047_a_mark_acts_on_its_element_whatever_the_page_paints() -> TestResult {
    // The page paints its own "1" beside Bravo; mark 1 is still Alpha, the snapshot's e1.
    let (tool, cx, _driver) = page_in_front("marks-decoy", PAGE).await?;
    let shot = act(&tool, &cx, json!({"action": "screenshot"})).await?;
    assert!(shot.contains("e1 button \"Alpha\""), "{shot}");
    act(&tool, &cx, json!({"action": "click", "ref": "1"})).await?;
    let after = act(&tool, &cx, json!({"action": "observe"})).await?;
    assert!(
        after.contains("clicked a"),
        "mark 1 clicked Alpha, not the painted decoy:\n{after}"
    );
    Ok(())
}

// TEST: RT-I-047
#[tokio::test]
#[ignore = "runs in the desktop-probe image, on its desktop"]
async fn rt_i_047_an_applications_boxes_are_its_screen_extents() -> TestResult {
    let (tool, cx, _editor, driver) = desktop_with("marks-app", grounded()).await?;
    let shot = act(&tool, &cx, json!({"action": "screenshot"})).await?;
    assert!(shot.contains("the window's accessibility tree"), "{shot}");
    let (x, y) = centre(&driver, "menu \"File\"").await?;
    act(&tool, &cx, json!({"action": "click", "x": x, "y": y})).await?;
    let opened = act(&tool, &cx, json!({"action": "observe"})).await?;
    assert!(
        opened.contains("menuitem \"Save"),
        "a click at File's extents opened it:\n{opened}"
    );
    act(&tool, &cx, json!({"action": "key", "keys": "Escape"})).await?;
    Ok(())
}

// TEST: RT-I-047
#[tokio::test]
#[ignore = "runs in the desktop-probe image, on its desktop"]
async fn rt_i_047_an_office_tool_bars_boxes_sit_on_its_buttons() -> TestResult {
    // LibreOffice reports its docked tool bars from the menu bar's top, 25 px above where they are drawn.
    let (tool, cx, _office, driver) =
        office("office-box", "--writer", "button \"Bold\"", grounded()).await?;
    let shot = act(&tool, &cx, json!({"action": "screenshot"})).await?;
    assert!(shot.contains("the window's accessibility tree"), "{shot}");
    let snapshot = driver.marked_snapshot().await?;
    let rect = |needle: &str| {
        snapshot
            .elements
            .iter()
            .find(|element| format!("{} \"{}\"", element.role, element.name) == needle)
            .and_then(|element| element.rect)
            .ok_or_else(|| format!("no box for {needle}"))
    };
    let menu_bottom = rect("menu \"File\"")?.bottom();
    for needle in ["button \"Save\"", "button \"Bold\""] {
        let found = rect(needle)?;
        assert!(
            i64::from(found.y) >= menu_bottom,
            "{needle}'s box {found:?} starts under the menu bar, which ends at {menu_bottom}"
        );
    }
    let (x, y) = centre(&driver, "button \"Italic\"").await?;
    act(&tool, &cx, json!({"action": "click", "x": x, "y": y})).await?;
    let after = act(&tool, &cx, json!({"action": "observe"})).await?;
    assert!(
        is_on(&after, "button \"Italic\""),
        "a click at Italic's box centre turned Italic on:\n{after}"
    );
    Ok(())
}

// TEST: RT-I-047
#[tokio::test]
#[ignore = "runs in the desktop-probe image, on its desktop"]
async fn rt_i_047_an_office_sheet_tab_is_double_clicked_where_it_is_drawn() -> TestResult {
    // A double-click goes to the box's centre (no action does it). Calc reports Sheet1's tab 25 px above where it is
    // drawn, on the horizontal scroll bar; on the tab itself, a double-click opens Rename Sheet.
    let (tool, cx, _office, _driver) =
        office("office-tab", "--calc", "tab \"Sheet1\"", View::default()).await?;
    let snapshot = act(&tool, &cx, json!({"action": "observe"})).await?;
    let tab = find(&snapshot, "tab \"Sheet1\"")?;
    act(&tool, &cx, json!({"action": "double_click", "ref": tab})).await?;
    let mut after = String::new();
    for _ in 0..25 {
        after = act(&tool, &cx, json!({"action": "observe"})).await?;
        if after.contains("Rename Sheet") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(
        after.contains("Rename Sheet"),
        "a double-click on Sheet1's ref opened Rename Sheet:\n{after}"
    );
    act(&tool, &cx, json!({"action": "key", "keys": "Escape"})).await?;
    Ok(())
}

/// How `LibreOffice` starts: its profile (`-env:UserInstallation=...`) and GTK's scale (`GDK_DPI_SCALE`).
struct Launch<'a> {
    profile: &'a str,
    scale: &'a str,
}

/// `LibreOffice` started with `args` as `launch` says, once the tool's snapshot holds `ready`. Its first start in a new
/// profile ends with 81, asking to be started again: it is started once more.
async fn office_until(
    tool: &Computer,
    cx: &ToolContext,
    launch: &Launch<'_>,
    args: &[String],
    ready: &str,
) -> Result<tokio::process::Child, Box<dyn std::error::Error>> {
    let start = || {
        tokio::process::Command::new("/usr/lib/libreoffice/program/soffice.bin")
            .args(["--norestore", "--nologo", launch.profile])
            .args(args)
            .env("GDK_DPI_SCALE", launch.scale)
            .kill_on_drop(true)
            .spawn()
    };
    let mut office = start()?;
    let mut restarted = false;
    let mut last = String::new();
    for _ in 0..300 {
        if !restarted
            && office
                .try_wait()?
                .is_some_and(|status| status.code() == Some(81))
        {
            office = start()?;
            restarted = true;
        }
        match act(tool, cx, json!({"action": "observe"})).await {
            Ok(snapshot) if snapshot.contains(ready) => return Ok(office),
            Ok(snapshot) => last = snapshot,
            Err(error) => last = error.model_message(),
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Err(format!("LibreOffice didn't show {ready}; the last look:\n{last}").into())
}

/// `LibreOffice`'s Start Center at GTK's `scale` with `docs` (plain text files) as its recent documents, the last one
/// first: each opened and closed in the session's own profile, then the Start Center started alone. The tool sees the
/// whole desktop.
async fn start_center(
    name: &str,
    scale: &str,
    docs: &[&str],
) -> Result<(Computer, ToolContext, tokio::process::Child), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!("tqa-desktop-{name}"));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("workspace"))?;
    let profile = format!(
        "-env:UserInstallation=file://{}",
        root.join("office-profile").display()
    );
    let driver = Arc::new(Desktop::new(
        None,
        XDo::new(Arc::new(Process)),
        Arc::new(AtspiTree::connect().await?),
        Some(Framebuffer::new(Path::new(SCREEN_FILE))),
    ));
    let cx = ToolContext {
        workspace: WorkspaceRoot::open(root.join("workspace"))?,
        cancel: CancellationToken::new(),
        clock: Arc::new(RealClock),
        sleeper: Arc::new(RealSleeper),
        env_allow: vec![],
        limits: ToolLimits::RECOMMENDED,
    };
    let tool = Computer::with_view(driver, View::default());
    let launch = Launch {
        profile: &profile,
        scale,
    };
    for (index, doc) in docs.iter().enumerate() {
        let file = root.join("workspace").join(format!("{doc}.txt"));
        std::fs::write(
            &file,
            format!("{}\n", doc.to_uppercase()).repeat(5 + 10 * index),
        )?;
        let args = [file.display().to_string()];
        let mut office = office_until(&tool, &cx, &launch, &args, &format!("{doc}.txt")).await?;
        act(&tool, &cx, json!({"action": "key", "keys": "ctrl+q"})).await?;
        tokio::time::timeout(Duration::from_secs(30), office.wait()).await??;
    }
    let last = docs.last().ok_or("no documents")?;
    let office = office_until(&tool, &cx, &launch, &[], &format!("option \"{last}\"")).await?;
    Ok((tool, cx, office))
}

/// Eight recent documents in two rows of Start Center at `scale`, and the second tile of the first row (golf) clicked
/// by its ref: its document opens. A tile has no action of its own, so the click goes to its box's centre.
async fn a_start_center_tile_opens_its_document(name: &str, scale: &str) -> TestResult {
    let docs = [
        "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel",
    ];
    let (tool, cx, _office) = start_center(name, scale, &docs).await?;
    let snapshot = act(&tool, &cx, json!({"action": "observe"})).await?;
    let tile = find(&snapshot, "option \"golf\"")?;
    act(&tool, &cx, json!({"action": "click", "ref": tile})).await?;
    let mut after = String::new();
    for _ in 0..50 {
        after = act(&tool, &cx, json!({"action": "observe"})).await?;
        if after.contains(".txt") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(
        after.contains("golf.txt"),
        "a click on golf's tile by its ref opened golf.txt (scale {scale}):\n{after}"
    );
    act(&tool, &cx, json!({"action": "key", "keys": "ctrl+q"})).await?;
    Ok(())
}

// TEST: RT-I-047
#[tokio::test]
#[ignore = "runs in the desktop-probe image, on its desktop"]
async fn rt_i_047_a_start_center_tile_opens_its_document_by_ref() -> TestResult {
    // LibreOffice reports Start Center's tiles 169 px below where they are drawn at scale 1, 209 px at 1.25.
    a_start_center_tile_opens_its_document("start-center-1", "1").await
}

// TEST: RT-I-047
#[tokio::test]
#[ignore = "runs in the desktop-probe image, on its desktop"]
async fn rt_i_047_a_start_center_tile_opens_its_document_by_ref_at_scale_1_25() -> TestResult {
    a_start_center_tile_opens_its_document("start-center-125", "1.25").await
}

/// x11vnc on the Desktop's display, as the image's init starts it (`-localhost -rfbport 5900`, no password, cut text
/// received only).
const VNC: &str = "127.0.0.1:5900";
/// The positive control's x11vnc: the same display, its clipboard flags left at their defaults.
const CONTROL_PORT: &str = "5901";

/// Sets an X selection (`--clipboard` or `--primary`) with the image's test-only `xsel`; it keeps serving it.
fn set_selection(which: &str, text: &str) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write as _;
    let mut child = std::process::Command::new("xsel")
        .args([which, "--input"])
        .stdin(std::process::Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("no stdin")?
        .write_all(text.as_bytes())?;
    child.wait()?;
    Ok(())
}

fn selection(which: &str) -> Result<String, Box<dyn std::error::Error>> {
    let out = std::process::Command::new("xsel")
        .args([which, "--output"])
        .output()?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// An RFB 3.8 session with no security, shared, past `ServerInit`, with its encodings set (Raw). It never asks for a
/// framebuffer update, so the only messages the server may send are a bell, a colour map or cut text.
fn rfb(address: &str) -> Result<std::net::TcpStream, Box<dyn std::error::Error>> {
    use std::io::{Read as _, Write as _};
    let mut stream = std::net::TcpStream::connect(address)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut version = [0_u8; 12];
    stream.read_exact(&mut version)?;
    assert!(version.starts_with(b"RFB 003."), "{version:?}");
    stream.write_all(b"RFB 003.008\n")?;
    let mut count = [0_u8; 1];
    stream.read_exact(&mut count)?;
    let mut types = vec![0_u8; usize::from(count[0])];
    stream.read_exact(&mut types)?;
    assert!(types.contains(&1), "no-security offered: {types:?}");
    stream.write_all(&[1])?;
    let mut result = [0_u8; 4];
    stream.read_exact(&mut result)?;
    assert_eq!(u32::from_be_bytes(result), 0, "security accepted");
    stream.write_all(&[1])?; // ClientInit: shared
    let mut init = [0_u8; 24];
    stream.read_exact(&mut init)?;
    let name = u32::from_be_bytes([init[20], init[21], init[22], init[23]]);
    let mut name_bytes = vec![0_u8; usize::try_from(name)?];
    stream.read_exact(&mut name_bytes)?;
    // SetEncodings: type 2, one padding byte, one encoding, Raw (0).
    stream.write_all(&[2, 0, 0, 1, 0, 0, 0, 0])?;
    Ok(stream)
}

/// How long a session's selections keep changing for cut text to come: x11vnc 0.9.17 makes the window it owns the
/// selections with about 10 s into a viewer's connection, and sends cut text only from about 15 s (measured in c17's
/// desktop-probe image: set at 10 s and 13 s, nothing; set at 15 s, at once).
const CUT_TEXT_BOUND: Duration = Duration::from_secs(30);

/// Whether one session to `address` receives cut text within [`CUT_TEXT_BOUND`], the selections set anew each second
/// while it is connected: the control must, and the product's x11vnc must not over the same span.
fn receives_cut_text(address: &str, tag: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let mut stream = rfb(address)?;
    let deadline = Instant::now() + CUT_TEXT_BOUND;
    let mut round = 0;
    while Instant::now() < deadline {
        set_selection("--clipboard", &format!("clipboard during {tag} {round}"))?;
        set_selection("--primary", &format!("primary during {tag} {round}"))?;
        if cut_text_within(&mut stream, Duration::from_secs(1))? {
            return Ok(true);
        }
        round += 1;
    }
    Ok(false)
}

/// A second x11vnc on the same display with its clipboard flags at their defaults, for the positive control; it ends
/// when dropped.
fn control_server() -> Result<std::process::Child, Box<dyn std::error::Error>> {
    let display = std::env::var("DISPLAY")?;
    let child = std::process::Command::new("x11vnc")
        .args([
            "-display",
            &display,
            "-localhost",
            "-rfbport",
            CONTROL_PORT,
            "-shared",
            "-forever",
            "-nopw",
            "-quiet",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let address = format!("127.0.0.1:{CONTROL_PORT}");
    for _ in 0..50 {
        if std::net::TcpStream::connect(&address).is_ok() {
            return Ok(child);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err("the control x11vnc didn't start listening".into())
}

/// Reads server messages until `quiet` passes without one; answers whether any was cut text (type 3).
fn cut_text_within(
    stream: &mut std::net::TcpStream,
    quiet: Duration,
) -> Result<bool, Box<dyn std::error::Error>> {
    use std::io::Read as _;
    stream.set_read_timeout(Some(quiet))?;
    loop {
        let mut kind = [0_u8; 1];
        match stream.read(&mut kind) {
            Ok(0) => return Err("the server closed the connection".into()),
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return Ok(false);
            }
            Err(error) => return Err(error.into()),
        }
        match kind[0] {
            3 => return Ok(true),
            2 => {} // bell
            1 => {
                let mut header = [0_u8; 5];
                stream.read_exact(&mut header)?;
                let colours = u16::from_be_bytes([header[3], header[4]]);
                let mut entries = vec![0_u8; usize::from(colours) * 6];
                stream.read_exact(&mut entries)?;
            }
            other => {
                return Err(
                    format!("unexpected server message {other} without an update request").into(),
                );
            }
        }
    }
}

/// Pastes `text` into the product's x11vnc with a `ClientCutText` (type 6, 3 padding bytes, the length, Latin-1 text)
/// and waits up to 5 s for the X clipboard to hold it.
fn paste_in(text: &str) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write as _;
    let mut stream = rfb(VNC)?;
    let mut message = vec![6_u8, 0, 0, 0];
    message.extend_from_slice(&u32::try_from(text.len())?.to_be_bytes());
    message.extend_from_slice(text.as_bytes());
    stream.write_all(&message)?;
    for _ in 0..25 {
        std::thread::sleep(Duration::from_millis(200));
        if selection("--clipboard")? == text {
            return Ok(());
        }
    }
    Err(format!(
        "the paste never reached the X clipboard (it holds {:?})",
        selection("--clipboard")?
    )
    .into())
}

// TEST: RT-I-046
/// The image runs this case last and alone (images/runtime/desktop-probe-run.sh): its paste in leaves x11vnc owning
/// the selections, and a `LibreOffice` case after it fails about 1 time in 3, a known limit in c17 that c18's RT-I-058
/// removes (after a paste in, applications never wait on x11vnc's selection).
#[tokio::test]
#[ignore = "runs in the desktop-probe image, on its desktop"]
async fn rt_i_046_nothing_is_copied_out_and_a_paste_reaches_the_x_clipboard() -> TestResult {
    tokio::task::spawn_blocking(|| -> Result<(), String> {
        let fail = |error: Box<dyn std::error::Error>| error.to_string();
        // The positive control first: the same session against an x11vnc with default clipboard flags must receive
        // the selection, or this probe couldn't see a copy-out at all.
        let mut control = control_server().map_err(fail)?;
        let control_address = format!("127.0.0.1:{CONTROL_PORT}");
        let seen = receives_cut_text(&control_address, "control").map_err(fail);
        let _ = control.kill();
        let _ = control.wait();
        if !seen? {
            return Err("the positive control received no ServerCutText: the probe's session can't see a copy-out".into());
        }
        // The product's x11vnc: selections set before the viewer connects, and changed while it is connected.
        set_selection("--clipboard", "secret before").map_err(fail)?;
        set_selection("--primary", "primary before").map_err(fail)?;
        if receives_cut_text(VNC, "product").map_err(fail)? {
            return Err("x11vnc sent the sandbox's selection out (ServerCutText)".into());
        }
        paste_in("pasted from the person").map_err(fail)
    })
    .await??;
    Ok(())
}
