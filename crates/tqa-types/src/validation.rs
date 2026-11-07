//! Validation of the bounded string fields required by protocol v1.

mod attachments;

use schemars::{Schema, generate::SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, de::Error as _};

pub use attachments::{InvalidAttachments, validate_attachments};

use crate::{MAX_ATTACHMENTS, MAX_TOOL_RESULT_BYTES, MAX_USER_MESSAGE_BYTES};

pub(crate) fn tool_result<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let content = String::deserialize(deserializer)?;
    if content.len() > MAX_TOOL_RESULT_BYTES {
        return Err(D::Error::custom(
            "content: expected at most 262144 UTF-8 bytes",
        ));
    }
    Ok(content)
}

pub(crate) fn retry_reason<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    const MAX_REASON_BYTES: usize = 128;
    let reason = String::deserialize(deserializer)?;
    if reason.len() > MAX_REASON_BYTES {
        return Err(D::Error::custom("reason: expected at most 128 UTF-8 bytes"));
    }
    Ok(reason)
}

/// Whether `text` may be a provider's words on an event: 1 to 300 characters, none of them a C0, DEL or C1 control.
#[must_use]
pub fn valid_provider_message(text: &str) -> bool {
    let characters = text.chars().count();
    (1..=crate::MAX_PROVIDER_MESSAGE_CHARS).contains(&characters)
        && !text.chars().any(|character| {
            let code = u32::from(character);
            code <= 0x1f || (0x7f..=0x9f).contains(&code)
        })
}

/// Parses a present provider message; `null` is refused (leave the field out instead).
pub(crate) fn provider_message<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    let text = String::deserialize(deserializer)?;
    if !valid_provider_message(&text) {
        return Err(D::Error::custom(
            "provider_message: expected 1 through 300 characters without control characters",
        ));
    }
    Ok(Some(text))
}

/// The schema of a provider message: the same bound and the control-character class written plainly, so every
/// validator reads it the same way.
pub(crate) fn provider_message_schema(
    _: &mut schemars::generate::SchemaGenerator,
) -> schemars::Schema {
    schemars::json_schema!({"type":"string","minLength":1,"maxLength":crate::MAX_PROVIDER_MESSAGE_CHARS,
        "pattern":"^[^\\u0000-\\u001f\\u007f-\\u009f]+$"})
}

pub(crate) fn message<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let text = String::deserialize(deserializer)?;
    if text.is_empty() || text.len() > MAX_USER_MESSAGE_BYTES {
        return Err(D::Error::custom(
            "text: expected 1 through 65536 UTF-8 bytes",
        ));
    }
    Ok(text)
}

pub(crate) fn attachments<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<String>, D::Error> {
    let paths = Vec::<String>::deserialize(deserializer)?;
    validate_attachments(&paths)
        .map_err(|error| D::Error::custom(format!("attachments: {error}")))?;
    Ok(paths)
}

pub(crate) fn fallback_routes<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<crate::RouteSnapshot>, D::Error> {
    let routes = Vec::<crate::RouteSnapshot>::deserialize(deserializer)?;
    if routes.len() > crate::MAX_FALLBACK_ROUTES {
        return Err(D::Error::custom(
            "fallback_routes: expected at most 3 routes",
        ));
    }
    for (index, route) in routes.iter().enumerate() {
        if routes[..index]
            .iter()
            .any(|earlier| earlier.name == route.name)
        {
            return Err(D::Error::custom(
                "fallback_routes: route names must be unique",
            ));
        }
    }
    Ok(routes)
}

pub(crate) fn route_name<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let name = String::deserialize(deserializer)?;
    if name.is_empty() || name.len() > crate::MAX_ROUTE_NAME_BYTES {
        return Err(D::Error::custom(
            "route: expected 1 through 256 UTF-8 bytes",
        ));
    }
    Ok(name)
}

pub(crate) fn model_name<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let name = String::deserialize(deserializer)?;
    if name.is_empty() || name.len() > crate::MAX_ROUTE_NAME_BYTES {
        return Err(D::Error::custom(
            "model: expected 1 through 256 UTF-8 bytes",
        ));
    }
    Ok(name)
}

pub(crate) fn timestamp<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    timestamp_field(deserializer, "ts")
}

/// A screen notice's `at`, as an event's `ts`.
pub(crate) fn notice_at<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    timestamp_field(deserializer, "at")
}

fn timestamp_field<'de, D: Deserializer<'de>>(
    deserializer: D,
    field: &str,
) -> Result<String, D::Error> {
    let text = String::deserialize(deserializer)?;
    if valid_timestamp(&text).is_none() {
        return Err(D::Error::custom(format!(
            "{field}: expected an RFC 3339 timestamp"
        )));
    }
    Ok(text)
}

fn number(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn valid_timestamp(text: &str) -> Option<()> {
    if !text.is_ascii() || text.len() < 20 {
        return None;
    }
    let bytes = text.as_bytes();
    if bytes[4] != b'-'
        || bytes[7] != b'-'
        || !matches!(bytes[10], b'T' | b't')
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    let year = number(&text[..4])?;
    let month = number(&text[5..7])?;
    let day = number(&text[8..10])?;
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = match month {
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => return None,
    };
    if !(1..=days).contains(&day)
        || number(&text[11..13])? > 23
        || number(&text[14..16])? > 59
        || number(&text[17..19])? > 60
    {
        return None;
    }
    let suffix = &text[19..];
    let fraction = if let Some(fraction) = suffix
        .strip_suffix('Z')
        .or_else(|| suffix.strip_suffix('z'))
    {
        fraction
    } else {
        let zone_start = suffix.len().checked_sub(6)?;
        let zone = &suffix[zone_start..];
        if !matches!(zone.as_bytes()[0], b'+' | b'-')
            || zone.as_bytes()[3] != b':'
            || number(&zone[1..3])? > 23
            || number(&zone[4..6])? > 59
        {
            return None;
        }
        &suffix[..zone_start]
    };
    if !fraction.is_empty() {
        let digits = fraction.strip_prefix('.')?;
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
    }
    Some(())
}

pub(crate) fn timestamp_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "format": "date-time"})
}

pub(crate) fn attachment_schema(generator: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "array", "maxItems": MAX_ATTACHMENTS, "uniqueItems": true,
        "items": generator.subschema_for::<attachments::AttachmentPath>()})
}
