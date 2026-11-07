//! A list's search text (`q`): trimmed, at most 100 characters, and matched as literal text in a case-insensitive
//! `LIKE` pattern, so `%`, `_` and `\` never act as wildcards (GW-I-119, GW-I-121, GW-I-122).
use crate::{GatewayError, InvalidReason};

/// The longest search text.
pub(crate) const MAX_CHARS: usize = 100;

/// `q` trimmed; empty means no search.
///
/// # Errors
/// `400 value_too_long` past [`MAX_CHARS`].
pub(crate) fn text(q: Option<&str>) -> Result<String, GatewayError> {
    let q = q.unwrap_or_default().trim();
    if q.chars().count() > MAX_CHARS {
        return Err(GatewayError::Invalid {
            field: "q",
            reason: InvalidReason::ValueTooLong,
        });
    }
    Ok(q.to_owned())
}

/// `q` as a case-insensitive `LIKE` pattern's literal part: `\`, `%` and `_` escaped.
pub(crate) fn literal(q: &str) -> String {
    q.chars()
        .flat_map(|character| match character {
            '\\' | '%' | '_' => vec!['\\', character],
            other => vec![other],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{literal, text};

    #[test]
    fn gw_i_119_query_text_is_a_literal_in_the_pattern() {
        assert_eq!(literal(r"50%_off\now"), r"50\%\_off\\now");
    }

    #[test]
    fn gw_i_121_search_text_is_trimmed_and_bounded() {
        assert_eq!(text(None).ok().as_deref(), Some(""));
        assert_eq!(text(Some("  csv ")).ok().as_deref(), Some("csv"));
        assert!(
            text(Some(&"é".repeat(100))).is_ok(),
            "characters, not bytes"
        );
        assert!(text(Some(&"a".repeat(101))).is_err());
    }
}
