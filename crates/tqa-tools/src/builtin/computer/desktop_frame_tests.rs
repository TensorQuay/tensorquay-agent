//! The frame rule of the desktop driver (TOOL-U-139, TOOL-U-142): a tree box outside the focused window draws no
//! mark, and the frame is the window's real client area, so a menu at the top of a managed window keeps its box.
use super::{
    a11y::{Node, Window},
    desktop_support::{Display, TestResult, Tree, desktop, node},
    driver::ScreenDriver,
};

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

// TEST: TOOL-U-142
#[tokio::test]
async fn tool_u_142_a_menu_at_the_top_of_a_managed_window_keeps_its_box() -> TestResult {
    // Under the Desktop's window manager xdotool reports a window at its frame's origin plus its inner offset: Mousepad's
    // client area at (320, 167) read as (321, 187) (14 Nov 2026, the desktop probe: "menu File has no box"). The
    // frame rule (TOOL-U-139) then cut every element in the top 20 px, the menu bar and the tool bar. The frame is the
    // window's real client area, so a top-edge menu keeps its box and a box far outside the window still loses it.
    let display = Display::managed(
        "16777219",
        311,
        "Mousepad",
        (321, 187, 640, 480),
        (320, 167, 640, 480),
    );
    let placed = |role: &str, handle: &str, extents| Node {
        extents: Some(extents),
        ..node(role, handle, &["sensitive"], None, handle)
    };
    let tree = Tree {
        window: Some(Window {
            handle: ":1.5/frame".into(),
            title: "Mousepad".into(),
            nodes: vec![
                placed("menu", ":1.5/file", (328, 169, 44, 22)),
                placed("push button", ":1.5/save", (340, 200, 32, 32)),
                placed("push button", ":1.5/stray", (0, 12, 32, 32)),
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
            (":1.5/file", true),
            (":1.5/save", true),
            (":1.5/stray", false)
        ],
        "the menu at the window's top edge keeps its box; a box in the screen's corner still loses it"
    );
    Ok(())
}
