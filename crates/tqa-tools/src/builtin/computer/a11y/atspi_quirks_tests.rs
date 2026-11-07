//! TOOL-U-083: the accessibility reader across toolkits' quirks, as 16z's Desktop shows them (at-spi2-core 2.56,
//! `LibreOffice` 25.2): a push button's two role names, actions named only in their non-localized form, and what's under
//! `LibreOffice`'s root pane: reported above where it is drawn, and a menu bar that is never drawn.
use super::{
    Node,
    atspi_wire::{Dock, Place, action_index, dock_shift, dock_step, thumbnail_offset},
    elements,
};

use Place::{AsReported, Hidden, Moved};

/// The actions a click looks for, as the desktop driver asks for them.
const CLICK: [&str; 5] = ["click", "press", "activate", "jump", "toggle"];

fn node(role: &str, name: &str) -> Node {
    Node {
        role: role.into(),
        name: name.into(),
        states: vec!["sensitive".into()],
        value: None,
        handle: format!(":1.5/{name}"),
        extents: Some((10, 20, 30, 40)),
    }
}

#[test]
fn tool_u_083_a_push_button_is_listed_by_either_of_its_role_names() {
    // TEST: TOOL-U-083
    // at-spi2-core 2.56's ATK names ATK_ROLE_PUSH_BUTTON "button" (GTK's dialogs, LibreOffice's and Chromium's tool
    // bars); older ones, and other bridges, say "push button".
    let listed = elements(&[node("button", "Close"), node("push button", "OK")]);
    let words: Vec<(&str, &str)> = listed
        .iter()
        .map(|element| (element.role.as_str(), element.name.as_str()))
        .collect();
    assert_eq!(words, [("button", "Close"), ("button", "OK")]);
}

#[test]
fn tool_u_083_an_action_is_found_by_its_own_name_then_by_its_shown_one() {
    // TEST: TOOL-U-083
    let some = |name: &str| Some(name.to_owned());
    let shown = |names: &[&str]| {
        names
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<Vec<_>>()
    };
    // GTK's push button: shown "Click", its own name "click".
    assert_eq!(
        action_index(&[some("click")], &shown(&["Click"]), &CLICK),
        Some(0)
    );
    // LibreOffice's tool bar items: no shown names at all, their own name "press" (16z's Writer: Bold, Save).
    assert_eq!(
        action_index(&[some("press"), some("")], &shown(&["", ""]), &CLICK),
        Some(0)
    );
    // A localized desktop shows its own words; the action's own name still says what it does.
    assert_eq!(
        action_index(
            &[some("menu"), some("click")],
            &shown(&["菜单", "单击"]),
            &CLICK
        ),
        Some(1)
    );
    // A toolkit that doesn't answer GetName: the shown name still counts.
    assert_eq!(action_index(&[None], &shown(&["Press"]), &CLICK), Some(0));
    // Neither name is a click, or there are no actions.
    assert_eq!(
        action_index(&[some("expand or contract")], &shown(&["Expand"]), &CLICK),
        None
    );
    assert_eq!(action_index(&[], &[], &CLICK), None);
}

/// A node as the walk meets it: its role and the extents it reports, if it has a place.
type Met<'a> = (&'a str, Option<(i32, i32, i32, i32)>);

/// Where each node of a chain is placed, from the root pane's parent down: the first node is that parent, the second the
/// root pane (or any node), each next one a child of the one before.
fn chain(nodes: &[Met<'_>]) -> Vec<Place> {
    let mut dock = Dock::Out;
    let mut parent: Option<(i32, i32, i32, i32)> = None;
    nodes
        .iter()
        .map(|&(role, extents)| {
            let pane = match (role, parent, extents) {
                ("root pane", Some((_, parent_y, _, _)), Some((_, y, _, _))) => {
                    dock_shift(parent_y, y)
                }
                _ => None,
            };
            let (place, next) = dock_step(dock, role, extents, pane);
            dock = next;
            parent = extents;
            place
        })
        .collect()
}

/// 16z's root pane, at y 19 inside the panel under the menu bar at y 44, for Writer and Calc alike.
const PANE: [Met<'static>; 2] = [
    ("panel", Some((0, 44, 1280, 756))),
    ("root pane", Some((0, 19, 1280, 756))),
];

fn under_pane(rest: &[Met<'static>]) -> Vec<Place> {
    let nodes: Vec<Met<'static>> = PANE.iter().chain(rest).copied().collect();
    chain(&nodes)[2..].to_vec()
}

#[test]
fn tool_u_083_everything_under_libreoffices_root_pane_moves_down_by_its_offset() {
    // TEST: TOOL-U-083
    // 16z's Writer and Calc: every node under a root pane reported s px above its parent is drawn s px lower.
    assert_eq!(dock_shift(44, 19), Some(25));
    // Writer's Standard tool bar and its New: drawn at 44, not 19.
    assert_eq!(
        under_pane(&[
            ("panel", Some((0, 19, 1280, 78))),
            ("tool bar", Some((0, 19, 1254, 39))),
            ("button", Some((4, 21, 44, 35))),
        ]),
        [Moved(25); 3]
    );
    // Calc's Sheet1 tab: reported at 728, drawn at 753 (its mark sat on the horizontal scroll bar).
    assert_eq!(
        under_pane(&[
            ("panel", Some((0, 97, 1280, 654))),
            ("panel", Some((0, 139, 1280, 612))),
            ("panel", Some((0, 728, 1266, 23))),
            ("page tab list", Some((133, 728, 1133, 23))),
            ("page tab", Some((133, 728, 73, 23))),
        ]),
        [Moved(25); 5]
    );
    // Calc's Function Wizard in the Formula Tool Bar: 103 reported, 128 drawn.
    assert_eq!(
        under_pane(&[
            ("panel", Some((0, 97, 1280, 654))),
            ("tool bar", Some((0, 97, 1280, 42))),
            ("button", Some((174, 103, 25, 27))),
        ]),
        [Moved(25); 3]
    );
    // Writer's status bar and a label in it: 751 reported, 776 drawn.
    assert_eq!(
        under_pane(&[
            ("statusbar", Some((0, 751, 1280, 24))),
            ("label", Some((6, 753, 23, 21))),
        ]),
        [Moved(25); 2]
    );
}

#[test]
fn tool_u_083_libreoffices_gtk_wrappers_and_hidden_menu_bar_stay_where_they_say() {
    // TEST: TOOL-U-083
    // Writer's sidebar: a panel at 99 holds one at 124, exactly 25 px lower with the same x, width and height (the
    // GTK wrapper's jump); it and everything in it, the Menu toggle at 126, report true positions.
    assert_eq!(
        under_pane(&[
            ("panel", Some((0, 97, 1280, 654))),
            ("split pane", Some((1221, 97, 59, 654))),
            ("panel", Some((1230, 99, 48, 650))),
            ("panel", Some((1230, 99, 48, 650))),
            ("panel", Some((1230, 124, 48, 650))),
            ("filler", Some((1230, 124, 44, 419))),
            ("toggle button", Some((1234, 126, 36, 34))),
        ]),
        [
            Moved(25),
            Moved(25),
            Moved(25),
            Moved(25),
            AsReported,
            AsReported,
            AsReported
        ]
    );
    // Calc's sheet navigation: Move To Home at 753, inside the jump from 728.
    assert_eq!(
        under_pane(&[
            ("panel", Some((0, 728, 133, 23))),
            ("panel", Some((0, 753, 133, 23))),
            ("filler", Some((6, 753, 121, 23))),
            ("button", Some((6, 753, 23, 23))),
        ]),
        [Moved(25), AsReported, AsReported, AsReported]
    );
    // Calc's horizontal scroll bar wrapper: 714 to 739.
    assert_eq!(
        under_pane(&[
            ("panel", Some((0, 714, 1261, 14))),
            ("panel", Some((0, 739, 1261, 14))),
            ("scroll bar", Some((0, 739, 1261, 14))),
        ]),
        [Moved(25), AsReported, AsReported]
    );
    // A child 25 px lower that isn't the same size isn't a jump: it moves.
    assert_eq!(
        under_pane(&[
            ("panel", Some((0, 728, 133, 23))),
            ("panel", Some((0, 753, 120, 23))),
        ]),
        [Moved(25), Moved(25)]
    );
    // The root pane's own menu bar, with no place, and its menus, never drawn duplicates of the native menu bar's titles
    // (File at [0, 19, 35, 19] beside the native [0, 19, 39, 25]): left out of the snapshot.
    assert_eq!(
        under_pane(&[
            ("menu bar", None),
            ("menu", Some((0, 19, 35, 19))),
            ("menu item", Some((0, 38, 120, 19))),
        ]),
        [Hidden; 3]
    );
    // A menu bar of the root pane's that has a place is drawn: it stays, as reported.
    assert_eq!(
        under_pane(&[
            ("menu bar", Some((0, 19, 1280, 25))),
            ("menu", Some((0, 19, 35, 25))),
        ]),
        [AsReported; 2]
    );
    // A menu bar deeper inside the root pane is not its own: it moves.
    assert_eq!(
        under_pane(&[
            ("panel", Some((0, 19, 1280, 25))),
            ("menu bar", Some((0, 19, 1256, 25))),
        ]),
        [Moved(25), Moved(25)]
    );
}

#[test]
fn tool_u_083_a_root_pane_where_it_belongs_moves_nothing() {
    // TEST: TOOL-U-083
    // At or below its parent's top, or further above it than any menu bar: not LibreOffice's quirk.
    assert_eq!(dock_shift(44, 44), None);
    assert_eq!(dock_shift(19, 44), None);
    assert_eq!(dock_shift(400, 19), None);
    assert_eq!(
        chain(&[
            ("panel", Some((0, 44, 1280, 756))),
            ("root pane", Some((0, 44, 1280, 756))),
            ("tool bar", Some((0, 44, 1254, 39))),
        ]),
        [AsReported; 3]
    );
    // Nodes outside a root pane keep their place.
    assert_eq!(
        chain(&[
            ("frame", Some((0, 0, 1280, 800))),
            ("panel", Some((0, 19, 1280, 78))),
            ("tool bar", Some((0, 19, 1254, 39))),
        ]),
        [AsReported; 3]
    );
}

/// A row of four Start Center tiles (202 x 232) at `y`, from x 315 every 226 px, as 16z's Desktop reports them at
/// scale 1.
fn tiles_at(y: i32) -> Vec<(i32, i32, i32, i32)> {
    (0..4)
        .map(|column| (315 + 226 * column, y, 202, 232))
        .collect()
}

/// Start Center's recent documents at scale 1 on 16z's Desktop: the list where it is drawn.
const RECENT_VIEW: (i32, i32, i32, i32) = (291, 122, 930, 627);

#[test]
fn tool_u_083_start_centers_tiles_are_placed_by_their_view() {
    // TEST: TOOL-U-083
    // 16z's Desktop at scale 1: four recent documents in one row reported at y 345, drawn from y 176 (the view at 122,
    // two rows fit its 627 px, so 54 px between rows and above the first); eight in two rows 286 px apart.
    assert_eq!(thumbnail_offset(RECENT_VIEW, &tiles_at(345)), Some(169));
    let two_rows = [tiles_at(345), tiles_at(631)].concat();
    assert_eq!(thumbnail_offset(RECENT_VIEW, &two_rows), Some(169));
    // A tile cut at the view's bottom (two pixels showing) is no tile size: the rest still place the rows.
    let cut = [two_rows.clone(), vec![(315, 917, 202, 2)]].concat();
    assert_eq!(thumbnail_offset(RECENT_VIEW, &cut), Some(169));
    // One tile alone.
    assert_eq!(
        thumbnail_offset(RECENT_VIEW, &[(315, 345, 202, 232)]),
        Some(169)
    );
    // Scale 1.25, templates: the view at [334, 125, 887, 624], 53 px between rows, columns 15 px apart from x 349.
    let tile = |column: i32, y: i32| (349 + 217 * column, y, 202, 232);
    let scaled: Vec<_> = [387, 672]
        .into_iter()
        .flat_map(|y| (0..4).map(move |column| tile(column, y)))
        .collect();
    assert_eq!(thumbnail_offset((334, 125, 887, 624), &scaled), Some(209));
}

#[test]
fn tool_u_083_a_list_not_laid_out_as_the_thumbnail_view_moves_nothing() {
    // TEST: TOOL-U-083
    // Already where the view draws them.
    assert_eq!(thumbnail_offset(RECENT_VIEW, &tiles_at(176)), None);
    // Items packed from the view's left edge with no space between (a value set's colours): not the thumbnail view.
    let packed: Vec<_> = (0..5)
        .map(|column| (291 + 40 * column, 300, 40, 40))
        .collect();
    assert_eq!(thumbnail_offset(RECENT_VIEW, &packed), None);
    // Rows further apart than the view spaces them.
    assert_eq!(
        thumbnail_offset(RECENT_VIEW, &[tiles_at(345), tiles_at(700)].concat()),
        None
    );
    // No items.
    assert_eq!(thumbnail_offset(RECENT_VIEW, &[]), None);
}
