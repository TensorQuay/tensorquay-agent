//! The `computer` tool's arguments: the strict schema and each action's own rules (ADR-0039's runtime contract). The
//! schema is strict-valid (every property required, the optional ones nullable), so each action's required fields
//! are checked here, naming the field.
use serde::Deserialize;
use serde_json::{Value, json};

use super::driver::Direction;
use crate::ToolError;

/// The most bytes `type` takes.
pub const MAX_TEXT_BYTES: usize = 4_000;
/// The most keys in one combination.
pub const MAX_KEYS: usize = 8;
/// `wait`'s bounds, in milliseconds.
pub const WAIT_MS: std::ops::RangeInclusive<u64> = 100..=10_000;
/// The largest coordinate taken; the latest screenshot's own size bounds it further.
pub const MAX_POINT: u32 = tqa_types::MAX_SCREENSHOT_SIDE;
/// `scroll`'s bounds, in notches, and its default.
pub const SCROLL_NOTCHES: std::ops::RangeInclusive<u8> = 1..=20;
const DEFAULT_NOTCHES: u8 = 3;

/// The modifier and named keys `key` accepts besides a single letter or digit (xdotool's names).
const NAMED_KEYS: [&str; 30] = [
    "ctrl",
    "alt",
    "shift",
    "super",
    "Return",
    "Tab",
    "Escape",
    "BackSpace",
    "Delete",
    "Insert",
    "Home",
    "End",
    "Page_Up",
    "Page_Down",
    "Up",
    "Down",
    "Left",
    "Right",
    "space",
    "minus",
    "plus",
    "equal",
    "comma",
    "period",
    "slash",
    "backslash",
    "semicolon",
    "apostrophe",
    "bracketleft",
    "bracketright",
];

/// What the route lets the tool offer (ADR-0039 section 3).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Offer {
    /// The route takes images: `screenshot` is offered, marked with the snapshot's refs.
    pub vision: bool,
    /// The coordinate forms are offered: `x`/`y` on click, double click and scroll, and `drag`, only on a route with
    /// images that grounds well (`pixel_grounding`) or has its provider's own computer tool.
    pub pixels: bool,
}

/// Where a pointer action lands: an element by its ref, or a point in the latest screenshot's pixels.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Spot {
    /// An element of the latest snapshot.
    Ref(String),
    /// A point in the latest screenshot, in its own pixels.
    Point(u32, u32),
}

/// One validated action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    /// A snapshot of the focused window.
    Observe,
    /// A snapshot and the screen as a marked image.
    Screenshot,
    /// A click on an element or a point.
    Click(Spot),
    /// A double click.
    DoubleClick(Spot),
    /// A scroll over an element or a point.
    Scroll(Spot, Direction, u8),
    /// A drag from one point of the latest screenshot to another.
    Drag((u32, u32), (u32, u32)),
    /// Text, into an element or the focus.
    Type(Option<String>, String),
    /// One key combination.
    Key(Vec<String>),
    /// A wait, in milliseconds.
    Wait(u64),
}

impl Action {
    /// The ref it acts on, when it acts on an element.
    #[must_use]
    pub fn element(&self) -> Option<&str> {
        match self {
            Self::Click(Spot::Ref(element))
            | Self::DoubleClick(Spot::Ref(element))
            | Self::Scroll(Spot::Ref(element), ..) => Some(element),
            Self::Type(element, _) => element.as_deref(),
            _ => None,
        }
    }

    /// The screenshot points it names, in its own pixels (a drag's start first).
    #[must_use]
    pub fn points(&self) -> Vec<(u32, u32)> {
        match self {
            Self::Click(Spot::Point(x, y))
            | Self::DoubleClick(Spot::Point(x, y))
            | Self::Scroll(Spot::Point(x, y), ..) => vec![(*x, *y)],
            Self::Drag(from, to) => vec![*from, *to],
            _ => Vec::new(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPoint {
    x: Option<i64>,
    y: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    action: String,
    #[serde(rename = "ref")]
    element: Option<String>,
    text: Option<String>,
    keys: Option<String>,
    ms: Option<u64>,
    direction: Option<String>,
    amount: Option<u8>,
    #[serde(default)]
    x: Option<i64>,
    #[serde(default)]
    y: Option<i64>,
    #[serde(default)]
    to: Option<RawPoint>,
}

/// The schema the model is offered: the structure-first actions always, and `screenshot` on a route with images.
#[must_use]
pub fn schema(offer: Offer) -> Value {
    let mut actions = vec!["observe"];
    if offer.vision {
        actions.push("screenshot");
    }
    actions.extend(["click", "double_click", "scroll"]);
    if offer.pixels {
        actions.push("drag");
    }
    actions.extend(["type", "key", "wait"]);
    let action_words = if offer.vision {
        "observe or screenshot first, then act on an element by its ref from the latest snapshot; a screenshot's \
         marks are the refs' numbers"
    } else {
        "observe first, then act on an element by its ref from the latest snapshot"
    };
    let string =
        |description: &str| json!({"type": ["string", "null"], "description": description});
    let mut schema = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["action", "ref", "text", "keys", "ms", "direction", "amount"],
        "properties": {
            "action": {"type": "string", "enum": actions, "description": action_words},
            "ref": string("An element's ref from the latest snapshot, such as e7, or a screenshot mark's number, such \
                           as 7: for click, double_click and scroll; optional for type (without it, the text goes \
                           to the focus)"),
            "text": string("For type: the text, 1 to 4000 bytes"),
            "keys": string("For key: one combination joined by +, such as ctrl+s, Return or alt+Tab (at most 8 keys)"),
            "ms": {"type": ["integer", "null"], "minimum": 100, "maximum": 10_000,
                   "description": "For wait: how long, in milliseconds"},
            "direction": {"type": ["string", "null"], "enum": ["up", "down", "left", "right", null],
                          "description": "For scroll"},
            "amount": {"type": ["integer", "null"], "minimum": 1, "maximum": 20,
                       "description": "For scroll: notches, 3 when left out"},
        }
    });
    if offer.pixels {
        add_points(&mut schema);
    }
    schema
}

/// The coordinate fields, for a route that grounds well: a point in the latest screenshot's own pixels.
fn add_points(schema: &mut Value) {
    let coordinate = |axis: &str| {
        json!({"type": ["integer", "null"], "minimum": 0, "maximum": MAX_POINT,
               "description": format!("For click, double_click, scroll and drag: {axis} in the latest screenshot's \
                                       pixels, instead of a ref; use a ref or a mark whenever the element has one")})
    };
    let point = json!({"type": "object", "additionalProperties": false, "required": ["x", "y"],
                       "properties": {"x": {"type": "integer", "minimum": 0, "maximum": MAX_POINT},
                                      "y": {"type": "integer", "minimum": 0, "maximum": MAX_POINT}}});
    if let Some(required) = schema["required"].as_array_mut() {
        required.extend([json!("x"), json!("y"), json!("to")]);
    }
    schema["properties"]["x"] = coordinate("the point's x");
    schema["properties"]["y"] = coordinate("the point's y");
    schema["properties"]["to"] = json!({"anyOf": [point, {"type": "null"}],
                                        "description": "For drag: where to, in the latest screenshot's pixels"});
}

/// Parses and checks a call's arguments, refusing a field that breaks its action's rule by name.
///
/// # Errors
/// `invalid_args` naming the field.
pub fn parse(args: Value, offer: Offer) -> Result<Action, ToolError> {
    let mut raw: Raw = super::super::arguments(args)?;
    // A ref is e7 or a mark's number, never a null-word: like a cursor, a blank or null-word one ("null", "\nnull",
    // " None ") is no ref (TOOL-U-034; c16k A7: a type meant for the focus came with ref "\nnull" and was refused).
    raw.element = super::super::absent(raw.element.take());
    if !offer.pixels && (raw.x.is_some() || raw.y.is_some() || raw.to.is_some()) {
        return Err(ToolError::invalid(
            "/x",
            "coordinates aren't offered on this route; use a ref or a screenshot's mark",
        ));
    }
    let spot = || spot(&raw);
    let action = match raw.action.as_str() {
        "observe" => Action::Observe,
        "screenshot" if offer.vision => Action::Screenshot,
        "click" => Action::Click(spot()?),
        "double_click" => Action::DoubleClick(spot()?),
        "scroll" => Action::Scroll(
            spot()?,
            direction(raw.direction.as_deref())?,
            notches(raw.amount)?,
        ),
        "drag" if offer.pixels => drag(&raw)?,
        "type" => typed(&raw)?,
        "key" => Action::Key(keys(raw.keys.as_deref())?),
        "wait" => Action::Wait(wait(raw.ms)?),
        _ => {
            return Err(ToolError::invalid(
                "/action",
                "expected one of the listed actions",
            ));
        }
    };
    Ok(action)
}

/// A pointer action's spot: a ref, or `x` and `y`, never both and never neither.
fn spot(raw: &Raw) -> Result<Spot, ToolError> {
    match (raw.element.as_deref(), point(raw.x, raw.y)?) {
        (Some(_), Some(_)) => Err(ToolError::invalid(
            "/ref",
            "give a ref or x and y, not both",
        )),
        (Some(element), None) => Ok(Spot::Ref(valid_ref(element)?)),
        (None, Some((x, y))) => Ok(Spot::Point(x, y)),
        (None, None) => required_ref(None).map(Spot::Ref),
    }
}

/// `x` and `y` together, each within [`MAX_POINT`], or neither.
fn point(x: Option<i64>, y: Option<i64>) -> Result<Option<(u32, u32)>, ToolError> {
    let axis = |value: i64, field: &str| {
        u32::try_from(value)
            .ok()
            .filter(|value| *value <= MAX_POINT)
            .ok_or_else(|| {
                ToolError::invalid(field, "expected a pixel of the latest screenshot, from 0")
            })
    };
    match (x, y) {
        (None, None) => Ok(None),
        (Some(x), Some(y)) => Ok(Some((axis(x, "/x")?, axis(y, "/y")?))),
        (Some(_), None) => Err(ToolError::invalid("/y", "x needs y")),
        (None, Some(_)) => Err(ToolError::invalid("/x", "y needs x")),
    }
}

/// A drag: from `x` and `y` to `to`, both points, with no ref.
fn drag(raw: &Raw) -> Result<Action, ToolError> {
    if raw.element.is_some() {
        return Err(ToolError::invalid(
            "/ref",
            "drag goes from point to point, without a ref",
        ));
    }
    let from = point(raw.x, raw.y)?
        .ok_or_else(|| ToolError::invalid("/x", "drag needs x and y to start from"))?;
    let to = raw
        .to
        .as_ref()
        .ok_or_else(|| ToolError::invalid("/to", "drag needs where to"))?;
    let to = point(to.x, to.y)
        .map_err(|_| {
            ToolError::invalid("/to", "expected x and y in the latest screenshot's pixels")
        })?
        .ok_or_else(|| {
            ToolError::invalid("/to", "expected x and y in the latest screenshot's pixels")
        })?;
    Ok(Action::Drag(from, to))
}

fn required_ref(value: Option<&str>) -> Result<String, ToolError> {
    valid_ref(value.ok_or_else(|| {
        ToolError::invalid("/ref", "this action needs an element's ref from observe")
    })?)
}

/// A ref as the snapshot writes them (`e` and a number from 1, without leading zeros), or a screenshot mark's bare
/// number (`"7"`, the same element as `e7` while that screenshot's snapshot is the latest). Kept as the model wrote
/// it, so the tool can tell a mark read off a picture from a ref read from the text.
fn valid_ref(value: &str) -> Result<String, ToolError> {
    let digits = value.strip_prefix('e').unwrap_or(value);
    let plain = (1..=6).contains(&digits.len())
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && !digits.starts_with('0');
    if plain {
        Ok(value.to_owned())
    } else {
        Err(ToolError::invalid(
            "/ref",
            "expected a ref from the latest snapshot, such as e7, or a mark's number, such as 7",
        ))
    }
}

/// What a `type` refused for its ref adds: the way that needs no ref (TOOL-U-066).
pub(super) const TYPE_NO_REF: &str =
    "leave out ref to type into the focus (after ctrl+l for an address)";

/// A `type`: an optional ref and its text. A refusal says what to do instead (TOOL-U-066; 16o live W11, A5: the address
/// sat in `keys` beside text "None" and a ref carried over from an earlier call, and three refusals named only a ref
/// from the snapshot). The text counts as missing when it is none or blank, or a null-word while `keys` holds a real
/// string; with `keys` empty, "None" is typed as given, since a person may mean the word.
fn typed(raw: &Raw) -> Result<Action, ToolError> {
    let element = raw.element.as_deref().map(valid_ref).transpose().map_err(|_| {
        ToolError::invalid(
            "/ref",
            &format!("expected a ref from the latest snapshot, such as e7, or a mark's number, such as 7; or {TYPE_NO_REF}"),
        )
    });
    let in_keys = super::super::absent(raw.keys.clone()).is_some();
    let missing = raw
        .text
        .as_deref()
        .is_none_or(|text| blank(text) || (in_keys && super::super::null_word(text)));
    if in_keys && missing {
        let also = if element.is_err() {
            format!("; and {TYPE_NO_REF}")
        } else {
            String::new()
        };
        return Err(ToolError::invalid(
            "/text",
            &format!("the text to type goes in text, not keys{also}"),
        ));
    }
    Ok(Action::Type(element?, text(raw.text.as_deref())?))
}

/// No text to type: empty, or plain spaces alone (TOOL-U-066). Never a key a model sends on purpose: a text of "\n"
/// (Enter, to submit) or "\t" (Tab, to the next field) types (lead 03:07).
fn blank(text: &str) -> bool {
    text.chars().all(|character| character == ' ')
}

fn text(value: Option<&str>) -> Result<String, ToolError> {
    let text = value.ok_or_else(|| ToolError::invalid("/text", "type needs the text"))?;
    if blank(text) {
        return Err(ToolError::invalid("/text", "the text is empty"));
    }
    if text.len() > MAX_TEXT_BYTES {
        return Err(ToolError::invalid(
            "/text",
            "the text is longer than 4000 bytes",
        ));
    }
    if text.contains('\0') {
        return Err(ToolError::invalid("/text", "the text holds a NUL"));
    }
    Ok(text.to_owned())
}

fn keys(value: Option<&str>) -> Result<Vec<String>, ToolError> {
    let combination = value
        .ok_or_else(|| ToolError::invalid("/keys", "key needs a combination, such as ctrl+s"))?;
    let keys: Vec<&str> = combination.split('+').collect();
    if keys.len() > MAX_KEYS {
        return Err(ToolError::invalid(
            "/keys",
            "at most 8 keys in one combination",
        ));
    }
    if let Some(unknown) = keys.iter().find(|key| !known_key(key)) {
        let unknown = if unknown.is_empty() {
            "an empty key"
        } else {
            "a key name it doesn't know"
        };
        return Err(ToolError::invalid(
            "/keys",
            &format!("{unknown}; use xdotool's names, such as ctrl, Return, Tab"),
        ));
    }
    Ok(keys.into_iter().map(str::to_owned).collect())
}

/// A single letter or digit, a function key F1 to F24, or one of the named keys.
fn known_key(key: &str) -> bool {
    let single = key.len() == 1 && key.bytes().all(|byte| byte.is_ascii_alphanumeric());
    let function = key
        .strip_prefix('F')
        .and_then(|number| number.parse::<u8>().ok())
        .is_some_and(|number| (1..=24).contains(&number) && !key[1..].starts_with('0'));
    single || function || NAMED_KEYS.contains(&key)
}

fn direction(value: Option<&str>) -> Result<Direction, ToolError> {
    match value {
        Some("up") => Ok(Direction::Up),
        Some("down") | None => Ok(Direction::Down),
        Some("left") => Ok(Direction::Left),
        Some("right") => Ok(Direction::Right),
        Some(_) => Err(ToolError::invalid(
            "/direction",
            "expected up, down, left or right",
        )),
    }
}

fn notches(value: Option<u8>) -> Result<u8, ToolError> {
    let notches = value.unwrap_or(DEFAULT_NOTCHES);
    if SCROLL_NOTCHES.contains(&notches) {
        Ok(notches)
    } else {
        Err(ToolError::invalid("/amount", "expected 1 to 20 notches"))
    }
}

fn wait(value: Option<u64>) -> Result<u64, ToolError> {
    let ms =
        value.ok_or_else(|| ToolError::invalid("/ms", "wait needs how long, in milliseconds"))?;
    if WAIT_MS.contains(&ms) {
        Ok(ms)
    } else {
        Err(ToolError::invalid(
            "/ms",
            "expected 100 to 10000 milliseconds",
        ))
    }
}
