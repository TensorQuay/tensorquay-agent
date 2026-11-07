//! An administrator's one-line description of a model route or an environment, shown where people pick one (GW-I-132;
//! display only). 1 through 160 characters on one line: no control characters, no line or paragraph separators, no
//! invisible formatting characters (Unicode's Cf: bidi overrides and isolates, zero-width spaces, the byte order mark,
//! which could make a label read as something else; the zero-width non-joiner and joiner that scripts need are fine),
//! and no surrounding spaces; refused rather than trimmed, like
//! a display name. Absent or `null` means none.
use crate::{GatewayError, InvalidReason};
use serde_json::Value;

/// The longest description, in characters: the columns' check (migration 0060).
pub(crate) const MAX_CHARS: usize = 160;

/// Whether `text` is a valid description.
pub(crate) fn valid(text: &str) -> bool {
    let count = text.chars().count();
    (1..=MAX_CHARS).contains(&count)
        && !text
            .chars()
            .any(|character| character.is_control() || invisible(character))
        && text.trim() == text
}

/// A line or paragraph separator (Unicode Zl, Zp) or a format character (Cf), which `char::is_control` (Cc) misses.
/// The standard library has no category lookup, so this is the Cf list of Unicode 15.
const fn invisible(character: char) -> bool {
    matches!(
        character as u32,
        0x2028 | 0x2029
            | 0xAD
            | 0x600..=0x605
            | 0x61C
            | 0x6DD
            | 0x70F
            | 0x890..=0x891
            | 0x8E2
            | 0x180E
            // U+200C and U+200D (zero-width non-joiner and joiner) are allowed: Persian, several Indic scripts and
            // emoji sequences need them, and they don't reorder text the way bidi controls do.
            | 0x200B
            | 0x200E..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x2064
            | 0x2066..=0x206F
            | 0xFEFF
            | 0xFFF9..=0xFFFB
            | 0x110BD
            | 0x110CD
            | 0x13430..=0x1343F
            | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A
            | 0xE0001
            | 0xE0020..=0xE007F
    )
}

/// A request's `description`: `None` when absent or `null`.
///
/// # Errors
/// `400 description_invalid` for anything but a valid string or `null`.
pub(crate) fn from_json(value: Option<Value>) -> Result<Option<String>, GatewayError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) if valid(&text) => Ok(Some(text)),
        Some(_) => Err(GatewayError::Invalid {
            field: "description",
            reason: InvalidReason::DescriptionInvalid,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_CHARS, from_json, valid};
    use crate::{GatewayError, InvalidReason};
    use serde_json::{Value, json};

    // TEST: GW-U-080
    #[test]
    fn gw_u_080_a_description_is_one_line_of_1_to_160_characters() {
        assert!(valid("Everyday work: documents, data and code"));
        assert!(
            valid(&"é".repeat(MAX_CHARS)),
            "counted in characters, not bytes"
        );
        for bad in [
            String::new(),
            " padded".into(),
            "padded ".into(),
            "two\nlines".into(),
            "tab\there".into(),
            "x".repeat(MAX_CHARS + 1),
            // Unicode's line and paragraph separators are line breaks too.
            "two\u{2028}lines".into(),
            "two\u{2029}paragraphs".into(),
            // Invisible formatting (bidi overrides and isolates, zero-width characters, the byte order mark) could make
            // a picker label read as something else.
            "safe\u{202E}txt.exe".into(),
            "iso\u{2066}late\u{2069}".into(),
            "zero\u{200B}width".into(),
            "\u{FEFF}bom".into(),
            "soft\u{AD}hyphen".into(),
        ] {
            assert!(!valid(&bad), "{bad:?}");
        }
        // Ordinary non-ASCII text stays fine.
        assert!(valid("Données, calcul et rédaction — 数据分析"));
        // The zero-width non-joiner and joiner that Persian, several Indic scripts and emoji sequences need.
        assert!(valid("می\u{200C}خواهم"));
        assert!(valid("data \u{1F469}\u{200D}\u{1F4BB}"));
    }

    // TEST: GW-U-080
    #[test]
    fn gw_u_080_absent_or_null_is_none_and_anything_else_is_refused_by_name() {
        assert_eq!(from_json(None).ok(), Some(None));
        assert_eq!(from_json(Some(Value::Null)).ok(), Some(None));
        assert_eq!(
            from_json(Some(json!("For quick lookups"))).ok(),
            Some(Some("For quick lookups".to_owned()))
        );
        for bad in [json!(""), json!(5), json!(["a"]), json!("a\nb")] {
            assert!(
                matches!(
                    from_json(Some(bad.clone())),
                    Err(GatewayError::Invalid {
                        field: "description",
                        reason: InvalidReason::DescriptionInvalid
                    })
                ),
                "{bad}"
            );
        }
    }
}
