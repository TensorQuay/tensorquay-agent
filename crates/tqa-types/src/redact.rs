//! Diagnostic redaction without storing credentials or compiling regular expressions.

/// Masks credential header/query values and key-shaped tokens in diagnostic text.
#[must_use]
pub fn redact(input: &str) -> String {
    mask_tokens(&mask_named_values(input))
}

fn mask_named_values(input: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let mut output = String::new();
    let mut offset = 0;
    while offset < input.len() {
        let found = ["authorization", "x-api-key", "api-key"]
            .iter()
            .filter_map(|key| {
                lower[offset..]
                    .find(key)
                    .map(|index| (offset + index, key.len()))
            })
            .min_by_key(|(index, _)| *index);
        let Some((start, length)) = found else {
            output.push_str(&input[offset..]);
            break;
        };
        let mut value_start = start + length;
        while input
            .as_bytes()
            .get(value_start)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'"' | b'\''))
        {
            value_start += 1;
        }
        if !input
            .as_bytes()
            .get(value_start)
            .is_some_and(|byte| matches!(byte, b':' | b'='))
        {
            output.push_str(&input[offset..start + length]);
            offset = start + length;
            continue;
        }
        value_start += 1;
        while input
            .as_bytes()
            .get(value_start)
            .is_some_and(u8::is_ascii_whitespace)
        {
            value_start += 1;
        }
        let quote = input
            .as_bytes()
            .get(value_start)
            .copied()
            .filter(|byte| matches!(byte, b'"' | b'\''));
        if quote.is_some() {
            value_start += 1;
        }
        let end = value_start + value_length(&input[value_start..], quote);
        output.push_str(&input[offset..value_start]);
        output.push_str("[redacted]");
        offset = end;
    }
    output
}

fn value_length(input: &str, quote: Option<u8>) -> usize {
    let Some(quote) = quote else {
        return input
            .find(['\n', '\r', '&', ',', '}'])
            .unwrap_or(input.len());
    };
    let mut escaped = false;
    for (index, byte) in input.bytes().enumerate() {
        if escaped {
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if byte == quote {
            return index;
        }
    }
    input.len()
}

fn token_character(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '+' | '/' | '=' | '_' | '-')
}

fn mask_tokens(input: &str) -> String {
    let mut output = String::new();
    let mut token = String::new();
    for ch in input.chars().chain(std::iter::once('\0')) {
        if token_character(ch) {
            token.push(ch);
            continue;
        }
        if token.starts_with("sk-") || token.len() >= 32 {
            output.push_str("[redacted]");
        } else {
            output.push_str(&token);
        }
        token.clear();
        if ch != '\0' {
            output.push(ch);
        }
    }
    output
}
