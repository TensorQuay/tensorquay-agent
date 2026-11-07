//! Size-first JSON parsing and stable diagnostic classification.

use serde::de::DeserializeOwned;

use crate::{ErrorCode, FrameError, MAX_FRAME_BYTES};

/// Every event type, so a malformed known event is `invalid_frame`, not `unknown_frame`; PROTO-U-003 checks this list
/// against the exported schema.
const EVENT_TYPES: &[&str] = &[
    "workspace.clone",
    "mcp.server",
    "decision",
    "decision.fallback",
    "budget.warning",
    "provider.retry",
    "permission.resolved",
    "permission.expired",
    "environment.changed",
    "message.user",
    "step.completed",
    "compaction",
    "route.fallback",
    "tool.result",
    "workspace.full",
    "model.completed",
    "run.started",
    "context.loaded",
    "step.started",
    "text.delta",
    "reasoning.delta",
    "reasoning.state",
    "tool.call",
    "tool.start",
    "tool.output",
    "tool.end",
    "usage",
    "run.completed",
    "run.cancelled",
    "run.failed",
    "run.resumed",
    "run.parked",
    "sandbox.state",
    "screen.capture",
];

pub(crate) fn parse<T: DeserializeOwned>(
    bytes: &[u8],
    frame_types: &[&str],
) -> Result<T, FrameError> {
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(FrameError {
            code: ErrorCode::InvalidFrame,
            path: "$".to_owned(),
            detail: "frame exceeds the 256 KiB encoded size limit".to_owned(),
        });
    }
    serde_json::from_slice(bytes).map_err(|error| {
        let detail = error.to_string();
        let unknown = serde_json::from_slice::<serde_json::Value>(bytes)
            .ok()
            .and_then(|value| unknown_type_path(&value, frame_types));
        FrameError {
            code: if unknown.is_some() {
                ErrorCode::UnknownFrame
            } else {
                ErrorCode::InvalidFrame
            },
            path: unknown.map_or_else(|| field_path(&detail), str::to_owned),
            detail,
        }
    })
}

fn unknown_type_path(value: &serde_json::Value, frame_types: &[&str]) -> Option<&'static str> {
    let name = value.get("type")?.as_str()?;
    if !frame_types.contains(&name) {
        return Some("$.type");
    }
    if name == "event" {
        let event_type = value.get("event")?.get("type")?.as_str()?;
        if !EVENT_TYPES.contains(&event_type) {
            return Some("$.event.type");
        }
    }
    None
}

fn field_path(detail: &str) -> String {
    for prefix in ["missing field `", "unknown field `", "duplicate field `"] {
        if let Some(rest) = detail.strip_prefix(prefix)
            && let Some((field, _)) = rest.split_once('`')
        {
            return format!("$.{field}");
        }
    }
    for field in [
        "text",
        "content",
        "attachments",
        "summary",
        "version",
        "fallback_routes",
        "mcp",
        "profile",
        "decision_engine",
        "ts",
        "expires_at",
    ] {
        if detail.starts_with(&format!("{field}:")) {
            return format!("$.{field}");
        }
    }
    "$".to_owned()
}
