//! What AT-SPI's replies say, decoded without a client library (ADR-0039 slice 2, amended 18 Oct 2026): an element's
//! states, sent as AT-SPI2's `StateType` bits in 32-bit words, the interfaces it names, and the null reference. And
//! two toolkits' quirks read around (TOOL-U-083): actions named only in their non-localized form, and what's under
//! `LibreOffice`'s root pane, reported above where it is drawn.

/// AT-SPI2's `StateType` names, by bit: bit `n` of the state words is `STATES[n]`.
pub const STATES: [&str; 44] = [
    "invalid",
    "active",
    "armed",
    "busy",
    "checked",
    "collapsed",
    "defunct",
    "editable",
    "enabled",
    "expandable",
    "expanded",
    "focusable",
    "focused",
    "has-tooltip",
    "horizontal",
    "iconified",
    "modal",
    "multi-line",
    "multiselectable",
    "opaque",
    "pressed",
    "resizable",
    "selectable",
    "selected",
    "sensitive",
    "showing",
    "single-line",
    "stale",
    "transient",
    "vertical",
    "visible",
    "manages-descendants",
    "indeterminate",
    "required",
    "truncated",
    "animated",
    "invalid-entry",
    "supports-autocompletion",
    "selectable-text",
    "is-default",
    "visited",
    "checkable",
    "has-popup",
    "read-only",
];

/// The interface whose text a node's value is read from.
pub const TEXT: &str = "org.a11y.atspi.Text";
/// The interface whose number a node's value is read from, when it has no text.
pub const VALUE: &str = "org.a11y.atspi.Value";

/// The path of AT-SPI's null reference: it names no object, whatever the bus name beside it (GTK's bridge sends its
/// own unique name with it).
const NULL_PATH: &str = "/org/a11y/atspi/null";

/// The names of the states `words` sets (`GetState`'s `au`: word `i` holds bits `32 i` to `32 i + 31`), in bit order.
/// A bit past the table, a state newer than it, is left out.
#[must_use]
pub fn states(words: &[u32]) -> Vec<String> {
    let mut names = Vec::new();
    for (index, word) in words.iter().enumerate() {
        for bit in 0..32 {
            if word & (1 << bit) != 0
                && let Some(name) = STATES.get(index * 32 + bit)
            {
                names.push((*name).to_owned());
            }
        }
    }
    names
}

/// Whether a state set has the state `name`.
#[must_use]
pub fn has(words: &[u32], name: &str) -> bool {
    STATES
        .iter()
        .position(|known| *known == name)
        .is_some_and(|bit| {
            words
                .get(bit / 32)
                .is_some_and(|word| word & (1 << (bit % 32)) != 0)
        })
}

/// The bus name of a reference (`GetChildren`'s `(so)`), or `None` for the null reference: any reference to the null
/// path, as atspi 0.30 read it.
#[must_use]
pub fn named<'a>(name: &'a str, path: &str) -> Option<&'a str> {
    (path != NULL_PATH).then_some(name)
}

/// The index of the first action named one of `wanted`: by its non-localized name (`GetName`, `None` where the toolkit
/// didn't answer), else by its shown name (`GetActions`). The shown name is localized, and `LibreOffice`'s tool bar items
/// leave it empty while their own name is `press`.
#[must_use]
pub fn action_index(names: &[Option<String>], shown: &[String], wanted: &[&str]) -> Option<usize> {
    let matches = |name: &str| wanted.contains(&name.to_lowercase().as_str());
    (0..names.len().max(shown.len())).find(|&index| {
        names
            .get(index)
            .and_then(Option::as_deref)
            .is_some_and(matches)
            || shown.get(index).is_some_and(|name| matches(name))
    })
}

/// The most a root pane can sit above its parent and still be `LibreOffice`'s quirk: a menu bar's height, with room.
const MAX_DOCK_SHIFT: i32 = 100;

/// How far a root pane, and so what's under it, is reported above where it is drawn: `LibreOffice` measures its work
/// window from the menu bar's top, so the root pane says y 19 inside a panel that starts at y 44, under the menu bar.
/// `None` when the root pane is at or below its parent's top, or further above it than any menu bar.
#[must_use]
pub fn dock_shift(parent_y: i32, root_pane_y: i32) -> Option<i32> {
    let shift = parent_y.checked_sub(root_pane_y)?;
    (1..=MAX_DOCK_SHIFT).contains(&shift).then_some(shift)
}

/// Where the walk places a node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Place {
    /// Where it reports itself.
    AsReported,
    /// This much lower than it reports itself.
    Moved(i32),
    /// Left out of the snapshot: it is never drawn.
    Hidden,
}

/// The extents a node reports: x, y, width, height.
type Extents = (i32, i32, i32, i32);

/// Where a node stands in the walk with respect to a root pane reported above its parent ([`dock_shift`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Dock {
    /// Outside any such root pane.
    Out,
    /// Inside one: what isn't a wrapper's jump moves down.
    Shifted {
        /// How far it moves down.
        shift: i32,
        /// The extents the node's parent reported.
        parent: Option<Extents>,
        /// Whether the parent is the root pane itself.
        pane_child: bool,
    },
    /// Inside a `LibreOffice` GTK wrapper, or the root pane's own drawn menu bar: as reported.
    Kept,
    /// Inside the root pane's own menu bar that has no place: never drawn.
    Hidden,
}

/// A node's place and its children's `Dock`, from the `Dock` its parent gave it, its role and the extents it reports.
/// `root_pane_shift` is [`dock_shift`] for a root pane (ignored for any other role).
///
/// `LibreOffice` reports everything under its root pane from the menu bar's top, so all of it moves down by the root
/// pane's shift, with two exceptions. Its embedded GTK widgets sit in a wrapper that reports itself exactly the shift
/// below its parent, with the same x, width and height: that wrapper and what's inside it report true positions. And
/// the root pane's own menu bar, with no place, holds never drawn duplicates of the native menu bar's titles: it is left
/// out, with everything in it (one that has a place is drawn, and stays as reported).
#[must_use]
pub fn dock_step(
    dock: Dock,
    role: &str,
    extents: Option<Extents>,
    root_pane_shift: Option<i32>,
) -> (Place, Dock) {
    match dock {
        Dock::Hidden => (Place::Hidden, Dock::Hidden),
        Dock::Kept => (Place::AsReported, Dock::Kept),
        Dock::Shifted {
            pane_child: true, ..
        } if role == "menu bar" => {
            if extents.is_some() {
                (Place::AsReported, Dock::Kept)
            } else {
                (Place::Hidden, Dock::Hidden)
            }
        }
        Dock::Shifted { shift, parent, .. } => {
            let jump = parent.zip(extents).is_some_and(|(above, own)| {
                own.0 == above.0 && own.1 == above.1 + shift && own.2 == above.2 && own.3 == above.3
            });
            if jump {
                (Place::AsReported, Dock::Kept)
            } else {
                (
                    Place::Moved(shift),
                    Dock::Shifted {
                        shift,
                        parent: extents,
                        pane_child: false,
                    },
                )
            }
        }
        Dock::Out => match root_pane_shift {
            Some(shift) if role == "root pane" => (
                Place::AsReported,
                Dock::Shifted {
                    shift,
                    parent: extents,
                    pane_child: true,
                },
            ),
            _ => (Place::AsReported, Dock::Out),
        },
    }
}

/// How far off a thumbnail view's layout a column or row may sit and still be it: rounding, and a border.
const THUMBNAIL_SLACK: i32 = 2;

/// How far the items of `LibreOffice`'s thumbnail view (Start Center's recent documents and templates: `list item`s in
/// a `list` under a `viewport` under a `scroll pane`) are reported below where they are drawn, from the list's `view`
/// and the items' extents. `None` when they aren't laid out as that view lays them out.
///
/// The list reports its own place right; its items' x, width and height are right too, but their y is too low by an
/// offset that changes with the scale (169 px at 1, 209 at 1.25 on 16z's Desktop). The view spaces its items evenly:
/// as many rows of the item's height as fit the list, the rest split into equal gaps above, between and below them,
/// and its columns the same way. So the first row is drawn one gap below the list's top, and the offset is how far
/// below that it is reported. The item size is the most common one (a tile cut at the list's edge is smaller); the
/// first column one column gap in from the list's left, both gaps over a few pixels and, with two rows or more, a row
/// stride of the height and a gap check the layout.
#[must_use]
pub fn thumbnail_offset(view: Extents, items: &[Extents]) -> Option<i32> {
    let (view_x, view_y, _, view_height) = view;
    let (width, height) = most_common_size(items)?;
    let rows = view_height.checked_div(height)?;
    if width <= 0 || rows < 1 {
        return None;
    }
    let gap = (view_height - rows * height) / (rows + 1);
    let sorted = |pick: fn(&Extents) -> i32| {
        let mut values: Vec<i32> = items
            .iter()
            .filter(|item| (item.2, item.3) == (width, height))
            .map(pick)
            .collect();
        values.sort_unstable();
        values.dedup();
        values
    };
    let (xs, ys) = (sorted(|item| item.0), sorted(|item| item.1));
    let (first_x, first_y) = (*xs.first()?, *ys.first()?);
    let column_gap = xs
        .get(1)
        .map_or(first_x - view_x, |next| next - first_x - width);
    // Items packed edge to edge are some other list: the view leaves real gaps (24 and 54 px at scale 1).
    if column_gap <= THUMBNAIL_SLACK
        || gap <= THUMBNAIL_SLACK
        || (first_x - view_x - column_gap).abs() > THUMBNAIL_SLACK
    {
        return None;
    }
    if ys
        .get(1)
        .is_some_and(|next| (next - first_y - height - gap).abs() > THUMBNAIL_SLACK)
    {
        return None;
    }
    let offset = first_y - (view_y + gap);
    (offset.abs() > THUMBNAIL_SLACK).then_some(offset)
}

/// The size most of `items` have, the first such in order on a tie.
fn most_common_size(items: &[Extents]) -> Option<(i32, i32)> {
    let mut counts: Vec<((i32, i32), usize)> = Vec::new();
    for item in items {
        let size = (item.2, item.3);
        match counts.iter_mut().find(|(seen, _)| *seen == size) {
            Some((_, count)) => *count += 1,
            None => counts.push((size, 1)),
        }
    }
    counts
        .iter()
        .rev()
        .max_by_key(|(_, count)| *count)
        .map(|(size, _)| *size)
}
