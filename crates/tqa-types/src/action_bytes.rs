//! Canonical prepared-action bytes shared across the runtime trust boundary.
use crate::{ErrorCode, FrameError, MAX_FRAME_BYTES, PermissionContext};
use serde_json::{Value, json};

/// Encodes the unchanged sorted tool/arguments/context envelope for SHA-256 hashing.
/// Object keys sort recursively; arrays retain their order. No execution is authorized here.
///
/// # Errors
/// Rejects invalid tool names (builtin or MCP), non-object arguments, zero counters, excessive nesting
/// and an encoded envelope above the protocol frame cap.
pub fn canonical_action_bytes(
    tool: &str,
    arguments: &Value,
    context: &PermissionContext,
) -> Result<Vec<u8>, FrameError> {
    if !crate::valid_tool_name(tool)
        || !arguments.is_object()
        || context.step == 0
        || context.attempt == 0
        || too_deep(arguments, 0)
    {
        return Err(invalid());
    }
    let mut envelope = json!({"tool":tool,"arguments":arguments,"context":context});
    envelope.sort_all_objects();
    let bytes = serde_json::to_vec(&envelope).map_err(|_| invalid())?;
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(invalid());
    }
    Ok(bytes)
}
fn invalid() -> FrameError {
    FrameError {
        code: ErrorCode::InvalidFrame,
        path: "action".into(),
        detail: "invalid or oversized prepared action envelope".into(),
    }
}
fn too_deep(value: &Value, depth: u16) -> bool {
    match value {
        _ if depth > 128 => true,
        Value::Array(items) => items.iter().any(|item| too_deep(item, depth + 1)),
        Value::Object(fields) => fields.values().any(|item| too_deep(item, depth + 1)),
        _ => false,
    }
}
