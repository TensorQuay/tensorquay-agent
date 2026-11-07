//! A key combination in xdotool's names (`ctrl+s`, `Return`, `shift+Tab`) as the `DevTools` key events a person's
//! typing makes: the modifiers held, the key pressed (with its text when it types one), and released.
use serde_json::{Value, json};

use super::super::driver::DriverError;

/// The `DevTools` modifier bit of a modifier key.
fn modifier(name: &str) -> Option<u8> {
    match name {
        "alt" => Some(1),
        "ctrl" => Some(2),
        "super" => Some(4),
        "shift" => Some(8),
        _ => None,
    }
}

/// A named key's `key`, `code` and Windows key code, and the text it types.
fn named(name: &str) -> Option<(&'static str, &'static str, u32, Option<&'static str>)> {
    Some(match name {
        "Return" => ("Enter", "Enter", 13, Some("\r")),
        "Tab" => ("Tab", "Tab", 9, None),
        "Escape" => ("Escape", "Escape", 27, None),
        "BackSpace" => ("Backspace", "Backspace", 8, None),
        "Delete" => ("Delete", "Delete", 46, None),
        "Insert" => ("Insert", "Insert", 45, None),
        "Home" => ("Home", "Home", 36, None),
        "End" => ("End", "End", 35, None),
        "Page_Up" => ("PageUp", "PageUp", 33, None),
        "Page_Down" => ("PageDown", "PageDown", 34, None),
        "Up" => ("ArrowUp", "ArrowUp", 38, None),
        "Down" => ("ArrowDown", "ArrowDown", 40, None),
        "Left" => ("ArrowLeft", "ArrowLeft", 37, None),
        "Right" => ("ArrowRight", "ArrowRight", 39, None),
        "space" => (" ", "Space", 32, Some(" ")),
        "minus" => ("-", "Minus", 189, Some("-")),
        "plus" => ("+", "Equal", 187, Some("+")),
        "equal" => ("=", "Equal", 187, Some("=")),
        "comma" => (",", "Comma", 188, Some(",")),
        "period" => (".", "Period", 190, Some(".")),
        "slash" => ("/", "Slash", 191, Some("/")),
        "backslash" => ("\\", "Backslash", 220, Some("\\")),
        "semicolon" => (";", "Semicolon", 186, Some(";")),
        "apostrophe" => ("'", "Quote", 222, Some("'")),
        "bracketleft" => ("[", "BracketLeft", 219, Some("[")),
        "bracketright" => ("]", "BracketRight", 221, Some("]")),
        _ => return None,
    })
}

/// The key events for one combination, in order: each modifier down, the key down and up, each modifier up.
///
/// # Errors
/// `Failed` for a combination without a key (only modifiers) or a key the browser has no event for.
pub fn events(keys: &[String]) -> Result<Vec<Value>, DriverError> {
    let (modifiers, rest): (Vec<&String>, Vec<&String>) =
        keys.iter().partition(|key| modifier(key).is_some());
    let [key] = rest.as_slice() else {
        return Err(DriverError::Failed(
            "a combination needs exactly one key besides its modifiers".into(),
        ));
    };
    let mut held = 0_u8;
    let mut events = Vec::new();
    for name in &modifiers {
        let (key_name, code, number) = modifier_key(name);
        held |= modifier(name).unwrap_or(0);
        events.push(json!({"type": "rawKeyDown", "key": key_name, "code": code, "windowsVirtualKeyCode": number,
                           "modifiers": held}));
    }
    let (key_name, code, number, text) = key_event(key, held)?;
    let mut down = json!({"type": if text.is_some() { "keyDown" } else { "rawKeyDown" }, "key": key_name, "code": code,
                          "windowsVirtualKeyCode": number, "modifiers": held});
    if let Some(text) = &text {
        down["text"] = json!(text);
    }
    events.push(down);
    events.push(
        json!({"type": "keyUp", "key": key_name, "code": code, "windowsVirtualKeyCode": number,
                       "modifiers": held}),
    );
    for name in modifiers.iter().rev() {
        let (key_name, code, number) = modifier_key(name);
        held &= !modifier(name).unwrap_or(0);
        events.push(
            json!({"type": "keyUp", "key": key_name, "code": code, "windowsVirtualKeyCode": number,
                           "modifiers": held}),
        );
    }
    Ok(events)
}

/// The key events that type `text` one character at a time, as a person's keyboard does: for each character its key
/// pressed with the character as its text, and released (an upper-case letter with shift held). For fields that don't
/// take inserted text, such as Chromium's date and time segments (TOOL-U-059). A character with no key of its own
/// (outside ASCII) is sent as its text alone, which the page receives as that character typed.
#[must_use]
pub fn typed(text: &str) -> Vec<Value> {
    let mut events = Vec::new();
    for character in text.chars() {
        let (key_name, code, number, shown, held) = character_key(character);
        // A key that types nothing (Tab) is pressed as a raw key, as a person's Tab moves the focus.
        let mut down = if shown.is_empty() {
            json!({"type": "rawKeyDown", "key": key_name, "modifiers": held})
        } else {
            json!({"type": "keyDown", "key": key_name, "modifiers": held, "text": shown, "unmodifiedText": shown})
        };
        let mut up = json!({"type": "keyUp", "key": key_name, "modifiers": held});
        if let Some((code, number)) = code.zip(number) {
            for event in [&mut down, &mut up] {
                event["code"] = json!(code);
                event["windowsVirtualKeyCode"] = json!(number);
            }
        }
        events.extend([down, up]);
    }
    events
}

/// One character's key: its `key`, its `code` and Windows key code when it has a key of its own, the text it types,
/// and the modifiers held.
fn character_key(character: char) -> (String, Option<String>, Option<u32>, String, u8) {
    let named_key = match character {
        '\n' | '\r' => Some("Return"),
        '\t' => Some("Tab"),
        ' ' => Some("space"),
        _ => NAMED_TEXT
            .iter()
            .find(|(_, text)| text.starts_with(character) && text.len() == character.len_utf8())
            .map(|(name, _)| *name),
    };
    if let Some((key_name, code, number, text)) = named_key.and_then(named) {
        let shown = text.map(str::to_owned).unwrap_or_default();
        return (
            key_name.to_owned(),
            Some(code.to_owned()),
            Some(number),
            shown,
            0,
        );
    }
    if character.is_ascii_alphanumeric() {
        let upper = character.to_ascii_uppercase();
        let code = if character.is_ascii_digit() {
            format!("Digit{character}")
        } else {
            format!("Key{upper}")
        };
        let held = if character.is_ascii_uppercase() { 8 } else { 0 };
        return (
            character.to_string(),
            Some(code),
            Some(u32::from(upper)),
            character.to_string(),
            held,
        );
    }
    (character.to_string(), None, None, character.to_string(), 0)
}

/// The punctuation keys [`named`] knows, by the text each types.
const NAMED_TEXT: [(&str, &str); 11] = [
    ("minus", "-"),
    ("plus", "+"),
    ("equal", "="),
    ("comma", ","),
    ("period", "."),
    ("slash", "/"),
    ("backslash", "\\"),
    ("semicolon", ";"),
    ("apostrophe", "'"),
    ("bracketleft", "["),
    ("bracketright", "]"),
];

fn modifier_key(name: &str) -> (&'static str, &'static str, u32) {
    match name {
        "alt" => ("Alt", "AltLeft", 18),
        "ctrl" => ("Control", "ControlLeft", 17),
        "super" => ("Meta", "MetaLeft", 91),
        _ => ("Shift", "ShiftLeft", 16),
    }
}

/// A key's event fields; a letter or digit types its character only without ctrl, alt or super held.
fn key_event(key: &str, held: u8) -> Result<(String, String, u32, Option<String>), DriverError> {
    let commands = held & 0b0111 != 0;
    if let Some((key_name, code, number, text)) = named(key) {
        let text = text.filter(|_| !commands).map(str::to_owned);
        return Ok((key_name.to_owned(), code.to_owned(), number, text));
    }
    if let Some(number) = key
        .strip_prefix('F')
        .and_then(|digits| digits.parse::<u32>().ok())
    {
        return Ok((key.to_owned(), key.to_owned(), 111 + number, None));
    }
    let mut characters = key.chars();
    if let (Some(character), None) = (characters.next(), characters.next())
        && character.is_ascii_alphanumeric()
    {
        let shifted = held & 8 != 0;
        let shown = if shifted {
            character.to_ascii_uppercase()
        } else {
            character
        };
        let code = if character.is_ascii_digit() {
            format!("Digit{character}")
        } else {
            format!("Key{}", character.to_ascii_uppercase())
        };
        let number = u32::from(character.to_ascii_uppercase());
        let text = (!commands).then(|| shown.to_string());
        return Ok((shown.to_string(), code, number, text));
    }
    Err(DriverError::Failed(format!(
        "the browser has no key event for {key}"
    )))
}
