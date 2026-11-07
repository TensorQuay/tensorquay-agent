//! The AT-SPI interfaces the accessibility tree calls, as zbus proxies (ADR-0039 slice 2, amended 18 Oct 2026): only
//! the methods and properties [`super::atspi_tree`] uses, written from AT-SPI2's D-Bus interfaces. Every proxy is built
//! for one object, by bus name and path, without a property cache: a snapshot reads the tree as it is now.
#![allow(
    missing_docs,
    reason = "zbus's proxy macro generates items (builders, property caches, change streams) without docs of their own"
)]

/// The session bus's pointer to the accessibility bus.
#[zbus::proxy(
    interface = "org.a11y.Bus",
    default_service = "org.a11y.Bus",
    default_path = "/org/a11y/bus"
)]
pub trait Bus {
    /// The accessibility bus's address.
    fn get_address(&self) -> zbus::Result<String>;
}

/// Any accessible object: its place in the tree, its role, states and interfaces.
#[zbus::proxy(interface = "org.a11y.atspi.Accessible", assume_defaults = false)]
pub trait Accessible {
    /// Its children, as (bus name, path); an empty name with the null path is no object.
    fn get_children(&self) -> zbus::Result<Vec<(String, zbus::OwnedObjectPath)>>;
    /// Its states, as `StateType` bits in 32-bit words.
    fn get_state(&self) -> zbus::Result<Vec<u32>>;
    /// Its role's name (`push button`, `text`, ...).
    fn get_role_name(&self) -> zbus::Result<String>;
    /// The interfaces it implements, by D-Bus name.
    fn get_interfaces(&self) -> zbus::Result<Vec<String>>;
    /// Its accessible name.
    #[zbus(property)]
    fn name(&self) -> zbus::Result<String>;
    /// Its description (a tooltip, often).
    #[zbus(property)]
    fn description(&self) -> zbus::Result<String>;
}

/// An object's own actions (`click`, `press`, `activate`, ...).
#[zbus::proxy(interface = "org.a11y.atspi.Action", assume_defaults = false)]
pub trait Action {
    /// Its actions, as (shown name, description, key binding); the shown name is localized, and may be empty.
    fn get_actions(&self) -> zbus::Result<Vec<(String, String, String)>>;
    /// The non-localized name of the action at `index` (`click`, `press`, ...).
    fn get_name(&self, index: i32) -> zbus::Result<String>;
    /// Does the action at `index`; whether it was done.
    fn do_action(&self, index: i32) -> zbus::Result<bool>;
}

/// An object whose text can be replaced.
#[zbus::proxy(interface = "org.a11y.atspi.EditableText", assume_defaults = false)]
pub trait EditableText {
    /// Replaces its text; whether it was.
    fn set_text_contents(&self, new_contents: &str) -> zbus::Result<bool>;
}

/// An object on the screen.
#[zbus::proxy(interface = "org.a11y.atspi.Component", assume_defaults = false)]
pub trait Component {
    /// Gives it the keyboard focus; whether it took it.
    fn grab_focus(&self) -> zbus::Result<bool>;
    /// Its box as (x, y, width, height), in the coordinates `coord_type` names ([`SCREEN`] for the screen's).
    fn get_extents(&self, coord_type: u32) -> zbus::Result<(i32, i32, i32, i32)>;
}

/// An object with text.
#[zbus::proxy(interface = "org.a11y.atspi.Text", assume_defaults = false)]
pub trait Text {
    /// Its text from `start_offset` up to `end_offset`, in characters.
    fn get_text(&self, start_offset: i32, end_offset: i32) -> zbus::Result<String>;
    /// How many characters it has.
    #[zbus(property)]
    fn character_count(&self) -> zbus::Result<i32>;
}

/// An object with a number (a slider, a spin button, a progress bar).
#[zbus::proxy(interface = "org.a11y.atspi.Value", assume_defaults = false)]
pub trait Value {
    /// Its number now.
    #[zbus(property)]
    fn current_value(&self) -> zbus::Result<f64>;
}

/// AT-SPI's `CoordType` for screen coordinates.
pub const SCREEN: u32 = 0;
