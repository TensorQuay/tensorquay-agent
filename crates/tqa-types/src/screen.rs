//! The `computer` tool's record (revision 14, ADR-0039's runtime contract). The element of an action by ref, as the
//! snapshot showed it, carried on that call's `tool.result` for a client's step line ("Clicked Save"): its role and
//! name are screen text, which a client renders as text only, and the model never receives it. Which of the action's
//! evidence screenshots were kept, on the same result. And `screen.capture`'s parts: one screenshot in the store.
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

/// The most characters of a name, as the snapshot cuts them (199 and an ellipsis).
pub const MAX_SCREEN_NAME_CHARS: usize = 200;

/// An element's role in the snapshot's words: a closed list, so a client words each one in its own language (agreed
/// with the web side, 5 Oct 2026). Both the browser's and AT-SPI's roles map onto it; anything else is `other`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ScreenRole {
    /// A button.
    Button,
    /// A link.
    Link,
    /// A text field or text area.
    TextBox,
    /// A search field.
    SearchBox,
    /// A check box.
    CheckBox,
    /// A radio button.
    Radio,
    /// An on/off switch.
    Switch,
    /// A combo box.
    ComboBox,
    /// A list box.
    ListBox,
    /// An option in a list.
    #[serde(rename = "option")]
    ListOption,
    /// A menu.
    Menu,
    /// A menu item.
    MenuItem,
    /// A menu item that checks.
    MenuItemCheckbox,
    /// A menu item that is one of a set.
    MenuItemRadio,
    /// A tab.
    Tab,
    /// A slider.
    Slider,
    /// A spin button.
    SpinButton,
    /// An item in a tree.
    TreeItem,
    /// A table cell.
    Cell,
    /// An icon (a file in a file manager, say).
    Icon,
    /// A heading.
    Heading,
    /// Any other role.
    Other,
}

impl ScreenRole {
    /// The role a snapshot's word names; a word not on the list is `other`.
    #[must_use]
    pub fn from_word(word: &str) -> Self {
        match word {
            "button" => Self::Button,
            "link" => Self::Link,
            "textbox" => Self::TextBox,
            "searchbox" => Self::SearchBox,
            "checkbox" => Self::CheckBox,
            "radio" => Self::Radio,
            "switch" => Self::Switch,
            "combobox" => Self::ComboBox,
            "listbox" => Self::ListBox,
            "option" => Self::ListOption,
            "menu" => Self::Menu,
            "menuitem" => Self::MenuItem,
            "menuitemcheckbox" => Self::MenuItemCheckbox,
            "menuitemradio" => Self::MenuItemRadio,
            "tab" => Self::Tab,
            "slider" => Self::Slider,
            "spinbutton" => Self::SpinButton,
            "treeitem" => Self::TreeItem,
            "cell" => Self::Cell,
            "icon" => Self::Icon,
            "heading" => Self::Heading,
            _ => Self::Other,
        }
    }
}

/// The element a `computer` action acted on.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, try_from = "RawScreenTarget")]
pub struct ScreenTarget {
    /// Its role, from the closed list.
    pub role: ScreenRole,
    /// Its accessible name as the snapshot showed it: at most 200 characters, one line, possibly empty.
    #[schemars(length(max = 200))]
    pub name: String,
}

/// The wire shape a [`ScreenTarget`] is read from (and the exported schema's), checked by [`ScreenTarget::new`].
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RawScreenTarget {
    /// Its role, from the closed list.
    role: ScreenRole,
    /// Its accessible name as the snapshot showed it: at most 200 characters, one line.
    #[schemars(length(max = 200))]
    name: String,
}

impl TryFrom<RawScreenTarget> for ScreenTarget {
    type Error = String;

    fn try_from(raw: RawScreenTarget) -> Result<Self, Self::Error> {
        Self::new(raw.role, raw.name)
    }
}

impl ScreenTarget {
    /// A target, checked: a name of at most 200 characters, holding no line break or other control character.
    ///
    /// # Errors
    /// The rule broken, naming the field.
    pub fn new(role: ScreenRole, name: String) -> Result<Self, String> {
        if name.chars().count() > MAX_SCREEN_NAME_CHARS {
            return Err("target.name: expected at most 200 characters".into());
        }
        if name.chars().any(char::is_control) {
            return Err("target.name: expected one line without control characters".into());
        }
        Ok(Self { role, name })
    }
}

/// Which of a `computer` call's screenshots the store kept, on that call's `tool.result`, so a client tells "not kept"
/// from "not yet" without reading the result's text. A kept one has its `screen.capture` ahead of the result. An acting
/// action has both fields; a `screenshot` call's Ok result has `after` only (its one capture), `before` absent. A
/// capture that wasn't kept is a present `false`, never an absent field.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScreenEvidence {
    /// Whether the screenshot from before the action was kept; absent (never `null`) on a `screenshot` call's result.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_bool"
    )]
    #[schemars(with = "bool")]
    pub before: Option<bool>,
    /// Whether the screenshot from after it (or the `screenshot` call's own) was kept.
    pub after: bool,
}

/// A field that may be absent but, when present, is a boolean: `null` is refused.
fn present_bool<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<bool>, D::Error> {
    bool::deserialize(deserializer).map(Some)
}

/// When a `screen.capture`'s screenshot was taken.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CapturePhase {
    /// Before the action, journaled ahead of its call's `tool.start`.
    Before,
    /// After it settled, journaled ahead of its call's `tool.result`.
    After,
    /// The model's own `screenshot` action, journaled ahead of its call's `tool.result`.
    Screenshot,
}

/// The most bytes of a stored screenshot (ADR-0039 section 9).
pub const MAX_SCREENSHOT_BYTES: u64 = 524_288;
/// The widest or tallest screenshot.
pub const MAX_SCREENSHOT_SIDE: u32 = 8192;

/// A screenshot store id: 1 to 128 ASCII letters, digits, `-` and `_`, so a client puts it in a URL path as it is.
pub(crate) fn screenshot_id<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    let id = String::deserialize(deserializer)?;
    let valid = (1..=128).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    if valid {
        Ok(id)
    } else {
        Err(serde::de::Error::custom(
            "id: expected 1 to 128 ASCII letters, digits, '-' or '_'",
        ))
    }
}

/// A SHA-256 in 64 lowercase hex digits.
pub(crate) fn sha256_hex<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let hex = String::deserialize(deserializer)?;
    if hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(hex)
    } else {
        Err(serde::de::Error::custom(
            "sha256: expected 64 lowercase hex digits",
        ))
    }
}

/// A screenshot's width or height: 1 to [`MAX_SCREENSHOT_SIDE`] pixels.
pub(crate) fn screenshot_side<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let side = u32::deserialize(deserializer)?;
    if (1..=MAX_SCREENSHOT_SIDE).contains(&side) {
        Ok(side)
    } else {
        Err(serde::de::Error::custom("expected 1 to 8192 pixels"))
    }
}

/// A stored screenshot's size: 1 to [`MAX_SCREENSHOT_BYTES`] bytes.
pub(crate) fn screenshot_bytes<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<u64, D::Error> {
    let bytes = u64::deserialize(deserializer)?;
    if (1..=MAX_SCREENSHOT_BYTES).contains(&bytes) {
        Ok(bytes)
    } else {
        Err(serde::de::Error::custom("bytes: expected 1 to 524288"))
    }
}

/// The default of `computer.max_screenshots_per_run` (ADR-0039 section 4).
pub const DEFAULT_MAX_SCREENSHOTS_PER_RUN: u32 = 200;
/// The default of `computer.max_actions_per_run`.
pub const DEFAULT_MAX_ACTIONS_PER_RUN: u32 = 1_000;

/// The `computer` tool's per-run caps, the administrator's settings as `session.config` carries them (revision 14,
/// ADR-0039 section 4). Absent means these defaults.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, try_from = "RawComputerLimits")]
pub struct ComputerLimits {
    /// The most screenshots a run sends the model, 1 to 1,000; past it `screenshot` is refused.
    #[schemars(range(min = 1, max = 1000))]
    pub max_screenshots_per_run: u32,
    /// The most actions a run takes, 1 to 5,000; past it every action is refused.
    #[schemars(range(min = 1, max = 5000))]
    pub max_actions_per_run: u32,
}

impl Default for ComputerLimits {
    fn default() -> Self {
        Self {
            max_screenshots_per_run: DEFAULT_MAX_SCREENSHOTS_PER_RUN,
            max_actions_per_run: DEFAULT_MAX_ACTIONS_PER_RUN,
        }
    }
}

/// The wire shape a [`ComputerLimits`] is read from, checked by [`ComputerLimits::new`].
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RawComputerLimits {
    #[schemars(range(min = 1, max = 1000))]
    max_screenshots_per_run: u32,
    #[schemars(range(min = 1, max = 5000))]
    max_actions_per_run: u32,
}

impl TryFrom<RawComputerLimits> for ComputerLimits {
    type Error = String;

    fn try_from(raw: RawComputerLimits) -> Result<Self, Self::Error> {
        Self::new(raw.max_screenshots_per_run, raw.max_actions_per_run)
    }
}

impl ComputerLimits {
    /// The caps, checked against their bounds.
    ///
    /// # Errors
    /// The cap out of its bounds, by name.
    pub fn new(max_screenshots_per_run: u32, max_actions_per_run: u32) -> Result<Self, String> {
        if !(1..=1000).contains(&max_screenshots_per_run) {
            return Err("computer_limits.max_screenshots_per_run: expected 1 to 1000".into());
        }
        if !(1..=5000).contains(&max_actions_per_run) {
            return Err("computer_limits.max_actions_per_run: expected 1 to 5000".into());
        }
        Ok(Self {
            max_screenshots_per_run,
            max_actions_per_run,
        })
    }
}
