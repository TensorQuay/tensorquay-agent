//! The accessibility path (ADR-0039 section 2, slice 2): another application's focused window read from its AT-SPI
//! tree, and its elements acted on by their own accessibility actions. The real tree speaks AT-SPI over the session's
//! accessibility bus ([`atspi_tree`]); a test uses a scripted one.
pub mod atspi_bus;
#[cfg(test)]
mod atspi_quirks_tests;
pub mod atspi_tree;
#[cfg(test)]
mod atspi_tree_tests;
pub mod atspi_wire;
#[cfg(test)]
mod atspi_wire_tests;

use async_trait::async_trait;

use super::{
    driver::{DriverError, RawElement},
    raster::Rect,
};

/// One node as the tree reads it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Node {
    /// AT-SPI's role name (`push button`, `text`, `check box`, ...).
    pub role: String,
    /// Its accessible name.
    pub name: String,
    /// Its states, in AT-SPI's names (`focused`, `checked`, `sensitive`, ...).
    pub states: Vec<String>,
    /// Its text or numeric value, when it has one; never read for a password field.
    pub value: Option<String>,
    /// The tree's own handle (bus name and object path).
    pub handle: String,
    /// Its extents on the screen (x, y, width, height), read during the walk, when it has a place.
    pub extents: Option<(i32, i32, i32, i32)>,
}

/// An application's focused window: its handle, title and nodes in reading order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Window {
    /// The window's own handle.
    pub handle: String,
    /// Its title (screen text, untrusted).
    pub title: String,
    /// Its nodes, walked within the tree's bounds.
    pub nodes: Vec<Node>,
}

/// The accessibility tree of the session.
#[async_trait]
pub trait A11y: Send + Sync {
    /// The active window of the application whose process is `pid`, or `None` when it exposes none.
    async fn window(&self, pid: u32) -> Result<Option<Window>, DriverError>;
    /// Performs the element's first action whose name is in `names`; `false` when it has none of them.
    async fn act(&self, handle: &str, names: &[&str]) -> Result<bool, DriverError>;
    /// Sets the element's whole text; `false` when it isn't editable.
    async fn set_text(&self, handle: &str, text: &str) -> Result<bool, DriverError>;
    /// Gives the element the focus; `false` when it can't take it.
    async fn focus(&self, handle: &str) -> Result<bool, DriverError>;
    /// The element's extents on the screen (x, y, width, height), when it has a place.
    async fn extents(&self, handle: &str) -> Result<Option<(i32, i32, i32, i32)>, DriverError>;
}

/// The tree when the accessibility bus couldn't be reached at the session's start: every call answers that, so the tool
/// reports `display_unavailable` for another application while the browser path still works.
pub struct Unreachable;

#[async_trait]
impl A11y for Unreachable {
    async fn window(&self, _: u32) -> Result<Option<Window>, DriverError> {
        Err(unreachable())
    }
    async fn act(&self, _: &str, _: &[&str]) -> Result<bool, DriverError> {
        Err(unreachable())
    }
    async fn set_text(&self, _: &str, _: &str) -> Result<bool, DriverError> {
        Err(unreachable())
    }
    async fn focus(&self, _: &str) -> Result<bool, DriverError> {
        Err(unreachable())
    }
    async fn extents(&self, _: &str) -> Result<Option<(i32, i32, i32, i32)>, DriverError> {
        Err(unreachable())
    }
}

fn unreachable() -> DriverError {
    DriverError::Unavailable("the accessibility bus isn't answering".into())
}

/// AT-SPI's role names and the words a snapshot uses for them, the same as the browser's: only what a person can act
/// on, and headings to find one's way. A push button has two names: at-spi2-core 2.56's ATK calls it `button` (GTK's
/// dialogs, `LibreOffice`'s and Chromium's tool bars on 16z's Desktop), older ones and other bridges `push button`.
pub(crate) const ROLES: [(&str, &str); 23] = [
    ("push button", "button"),
    ("button", "button"),
    ("toggle button", "button"),
    ("push button menu", "button"),
    ("check box", "checkbox"),
    ("radio button", "radio"),
    ("menu item", "menuitem"),
    ("check menu item", "menuitemcheckbox"),
    ("radio menu item", "menuitemradio"),
    ("menu", "menu"),
    ("text", "textbox"),
    ("entry", "textbox"),
    ("password text", "textbox"),
    ("combo box", "combobox"),
    ("list item", "option"),
    ("page tab", "tab"),
    ("link", "link"),
    ("spin button", "spinbutton"),
    ("slider", "slider"),
    ("tree item", "treeitem"),
    ("table cell", "cell"),
    ("icon", "icon"),
    ("heading", "heading"),
];
/// The states a snapshot says, in AT-SPI's names and the snapshot's.
const STATES: [(&str, &str); 6] = [
    ("focused", "focused"),
    ("checked", "checked"),
    ("selected", "selected"),
    ("expanded", "expanded"),
    ("pressed", "pressed"),
    ("required", "required"),
];

/// Whether AT-SPI's role is one a snapshot lists.
#[must_use]
pub fn listed(role: &str) -> bool {
    ROLES.iter().any(|(name, _)| *name == role)
}

/// The window's nodes as a snapshot's elements, in the browser's words: listed roles only, `disabled` where it isn't
/// sensitive, and a password field's value never shown.
#[must_use]
pub fn elements(nodes: &[Node]) -> Vec<RawElement> {
    nodes
        .iter()
        .filter_map(|node| {
            let role = ROLES
                .iter()
                .find(|(name, _)| *name == node.role)
                .map(|(_, word)| *word)?;
            let mut states: Vec<String> = STATES
                .iter()
                .filter(|(name, _)| node.states.iter().any(|state| state == name))
                .map(|(_, word)| (*word).to_owned())
                .collect();
            if !node.states.iter().any(|state| state == "sensitive") {
                states.push("disabled".into());
            }
            let password = node.role == "password text";
            if password {
                states.push("password".into());
            }
            Some(RawElement {
                role: role.to_owned(),
                name: node.name.clone(),
                states,
                value: if password {
                    None
                } else {
                    node.value.clone().filter(|value| !value.is_empty())
                },
                handle: node.handle.clone(),
                rect: node.extents.and_then(|(x, y, width, height)| {
                    Rect::new(
                        x,
                        y,
                        u32::try_from(width).ok()?,
                        u32::try_from(height).ok()?,
                    )
                }),
            })
        })
        .collect()
}
