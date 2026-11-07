//! `channel_post`'s flat arguments to a validated [`ChannelPost`]: each type takes its own fields, requires its required
//! ones and refuses any other, so a stray argument is an error the model sees, never silently dropped.
use crate::ToolError;
use serde_json::{Map, Value};
use tqa_types::{ChannelPost, MessageBody};

/// Arguments every post takes.
const COMMON: [&str; 3] = ["channel", "type", "parent"];

/// Each postable type with its required and optional fields.
const TYPES: [(&str, &[&str], &[&str]); 6] = [
    ("text", &["text"], &[]),
    ("proposal", &["title", "text"], &["window_seconds"]),
    ("vote", &["proposal_seq", "choice"], &["reason"]),
    ("task", &["title", "spec"], &["claims"]),
    ("report", &["task_seq", "status", "summary"], &["refs"]),
    ("done", &[], &["summary"]),
];

/// The post the arguments describe. Null arguments count as absent.
///
/// # Errors
/// `invalid_args` at the offending argument: an unknown or unpostable type, a missing required field, a field the type
/// doesn't take, or a value out of bounds.
pub(crate) fn from_args(args: &Value) -> Result<ChannelPost, ToolError> {
    let args = args
        .as_object()
        .ok_or_else(|| ToolError::invalid("", "expected an object"))?;
    let present = |name: &str| args.get(name).is_some_and(|value| !value.is_null());
    let kind = args.get("type").and_then(Value::as_str).unwrap_or_default();
    let Some((_, required, optional)) = TYPES.iter().find(|(name, ..)| *name == kind) else {
        return Err(ToolError::invalid(
            "/type",
            "expected text, proposal, vote, task, report or done (decisions are the gateway's; claims use task_claim)",
        ));
    };
    for name in *required {
        if !present(name) {
            return Err(ToolError::invalid(
                &format!("/{name}"),
                &format!("required for a {kind} post"),
            ));
        }
    }
    if kind == "task" && present("claims") {
        let claims = &args["claims"];
        let valid = claims.as_str() == Some("all")
            || claims
                .as_u64()
                .is_some_and(|count| (1..=tqa_types::MAX_CHANNEL_MEMBERS as u64).contains(&count));
        if !valid {
            return Err(ToolError::invalid(
                "/claims",
                "expected 1 to 32 claimants, or \"all\"",
            ));
        }
    }
    let mut body = Map::new();
    body.insert("type".into(), Value::String(kind.into()));
    for (name, value) in args {
        if value.is_null() || COMMON.contains(&name.as_str()) {
            continue;
        }
        if !required.contains(&name.as_str()) && !optional.contains(&name.as_str()) {
            return Err(ToolError::invalid(
                &format!("/{name}"),
                &format!("not taken by a {kind} post"),
            ));
        }
        body.insert(name.clone(), value.clone());
    }
    let body: MessageBody = serde_json::from_value(Value::Object(body)).map_err(|_| {
        ToolError::invalid(
            "/type",
            &format!("a field has the wrong type for a {kind} post"),
        )
    })?;
    let mut post = ChannelPost::new(body);
    post.parent = match args.get("parent") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_u64()
                .ok_or_else(|| ToolError::invalid("/parent", "expected a message seq"))?,
        ),
    };
    post.validate().map_err(|error| {
        let path = error.path.strip_prefix("body.").unwrap_or(&error.path);
        let path = if path == "$" || path == "body" {
            String::new()
        } else {
            format!("/{path}")
        };
        ToolError::invalid(&path, &error.detail)
    })?;
    Ok(post)
}
