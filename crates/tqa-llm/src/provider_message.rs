//! The provider's own words from an HTTP error body, sanitised for `run.failed` and `provider.retry` (LLM-U-036):
//! the message field when the body is JSON, else the text; control characters become spaces and whitespace collapses;
//! a token after `Bearer` or `Basic` is masked whatever its length, then every other credential form `redact` knows;
//! at most `MAX_PROVIDER_MESSAGE_CHARS` characters, cut on a character boundary.
use tqa_types::MAX_PROVIDER_MESSAGE_CHARS;

/// The sanitised message, or `None` when nothing is left.
pub(crate) fn sanitise(body: &str) -> Option<String> {
    let text = message_field(body).unwrap_or_else(|| body.to_owned());
    let flat: String = text
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    let collapsed = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    // Masked before `redact`, which would otherwise mask only the word `Bearer` as a header's value.
    let text = crate::redact(&mask_schemes(&collapsed));
    if text.is_empty() {
        return None;
    }
    if text.chars().count() <= MAX_PROVIDER_MESSAGE_CHARS {
        return Some(text);
    }
    let mut cut: String = text.chars().take(MAX_PROVIDER_MESSAGE_CHARS - 1).collect();
    cut.push('…');
    Some(cut)
}

/// The provider's message field: `error.message`, then `message`, `error` or `detail` as a string, in an object or a
/// one-element list.
fn message_field(body: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(body).ok()?;
    // Some providers wrap the error object in a one-element list.
    let value = parsed.get(0).unwrap_or(&parsed);
    [
        &value["error"]["message"],
        &value["message"],
        &value["error"],
        // A flat `{"detail": "..."}`, as some hosted backends answer.
        &value["detail"],
    ]
    .into_iter()
    .find_map(|field| field.as_str().map(str::to_owned))
}

/// Masks the word after `Bearer` or `Basic` (any case), however short it is.
fn mask_schemes(text: &str) -> String {
    let mut masked = Vec::new();
    let mut after_scheme = false;
    for word in text.split(' ') {
        if after_scheme {
            masked.push("[redacted]");
            after_scheme = false;
            continue;
        }
        after_scheme = matches!(
            word.to_ascii_lowercase().trim_end_matches(':'),
            "bearer" | "basic"
        );
        masked.push(word);
    }
    masked.join(" ")
}
