use super::Capability;
use crate::{ErrorCode, FrameError};
use schemars::{JsonSchema, Schema, generate::SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, de::Error as _};

pub(super) const DIGEST_PATTERN: &str =
    "^([A-Za-z0-9][A-Za-z0-9._:-]*(/[A-Za-z0-9][A-Za-z0-9._:-]*)*@)?sha256:[0-9a-f]{64}$";

fn invalid(field: &str, detail: &str) -> FrameError {
    FrameError {
        code: ErrorCode::InvalidFrame,
        path: format!("environment.{field}"),
        detail: detail.into(),
    }
}

pub(super) fn validate_template(value: &str) -> Result<(), FrameError> {
    if !(1..=64).contains(&value.len())
        || !value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
    {
        return Err(invalid("template", "expected [a-z][a-z0-9_-]{0,63}"));
    }
    Ok(())
}

pub(super) fn validate_capabilities(values: &[Capability]) -> Result<(), FrameError> {
    if values.len() > 7
        || values
            .iter()
            .enumerate()
            .any(|(index, value)| values[..index].contains(value))
    {
        return Err(invalid(
            "capabilities",
            "expected zero through seven unique capabilities",
        ));
    }
    Ok(())
}

pub(super) fn validate_digest(value: &str) -> Result<(), FrameError> {
    let digest = if let Some((reference, digest)) = value.split_once('@') {
        if !reference.split('/').all(|segment| {
            segment
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
        }) {
            return Err(invalid(
                "digest",
                "expected a registry reference without credentials",
            ));
        }
        digest
    } else {
        value
    };
    if value.len() > 512
        || !digest.strip_prefix("sha256:").is_some_and(|hex| {
            hex.len() == 64
                && hex
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
    {
        return Err(invalid(
            "digest",
            "expected a lowercase SHA-256 pin within 512 ASCII bytes",
        ));
    }
    Ok(())
}

pub(crate) fn template<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    validate_template(&value).map_err(D::Error::custom)?;
    Ok(value)
}

pub(crate) fn capabilities<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<Capability>, D::Error> {
    let mut values = Vec::<Capability>::deserialize(deserializer)?;
    validate_capabilities(&values).map_err(D::Error::custom)?;
    values.sort_unstable();
    Ok(values)
}

pub(crate) fn template_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type":"string", "minLength":1, "maxLength":64, "pattern":"^[a-z][a-z0-9_-]{0,63}$"})
}

pub(crate) fn capabilities_schema(generator: &mut SchemaGenerator) -> Schema {
    json_schema!({"type":"array", "maxItems":7, "uniqueItems":true,
        "items":Capability::json_schema(generator)})
}
