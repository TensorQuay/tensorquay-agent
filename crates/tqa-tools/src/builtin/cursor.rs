//! Bounded cursor encoding shared by read and search pages.
use crate::{READ_PAGE_BYTES, ToolError};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Serialize, de::DeserializeOwned};

pub(super) fn encode(record: &impl Serialize) -> Result<String, ToolError> {
    let bytes =
        serde_json::to_vec(record).map_err(|_| ToolError::Io("cursor encoding failed".into()))?;
    let token = URL_SAFE_NO_PAD.encode(bytes);
    if token.len() > READ_PAGE_BYTES {
        return Err(ToolError::OutputTooLarge {
            max: READ_PAGE_BYTES,
        });
    }
    Ok(token)
}
pub(super) fn decode<T: DeserializeOwned>(token: &str) -> Result<T, ToolError> {
    if token.len() > READ_PAGE_BYTES {
        return Err(ToolError::invalid(
            "/cursor",
            "cursor is too large; start again",
        ));
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(token)
        .map_err(|_| ToolError::invalid(
            "/cursor",
            "not a cursor from an earlier call of this tool; to start from the beginning, leave cursor out",
        ))?;
    serde_json::from_slice(&bytes)
        .map_err(|_| ToolError::invalid(
            "/cursor",
            "not a cursor from an earlier call of this tool; to start from the beginning, leave cursor out",
        ))
}
