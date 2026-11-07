//! TOOL-U-020: gateway and runtime share the call envelope the Git relay binds one push to, byte for byte.
use serde_json::json;
use std::error::Error;
use tqa_types::{PermissionContext, canonical_action_bytes};

fn context() -> Result<PermissionContext, serde_json::Error> {
    serde_json::from_value(json!({"conversation_id":"c", "run_id":"r",
        "step":1, "attempt":1, "call_id":"call"}))
}

#[test]
fn tool_u_020_canonical_envelope_keeps_the_existing_byte_contract() -> Result<(), Box<dyn Error>> {
    let arguments = json!({"env":{"B":"two", "A":"one"}, "command":"echo hi"});
    let bytes = canonical_action_bytes("shell", &arguments, &context()?)?;
    assert_eq!(
        String::from_utf8(bytes)?,
        r#"{"arguments":{"command":"echo hi","env":{"A":"one","B":"two"}},"context":{"attempt":1,"call_id":"call","conversation_id":"c","run_id":"r","step":1},"tool":"shell"}"#
    );
    let tuple = json!({"remote":"https://git.example/repo", "branch":"feature/example", "commit":"a".repeat(40)});
    let original = canonical_action_bytes("git_push", &tuple, &context()?)?;
    for field in ["remote", "branch", "commit"] {
        let mut changed = tuple.clone();
        changed[field] = json!("changed");
        assert_ne!(
            original,
            canonical_action_bytes("git_push", &changed, &context()?)?
        );
    }
    Ok(())
}

#[test]
fn tool_u_020_canonical_envelope_rejects_unbounded_or_invalid_inputs() -> Result<(), Box<dyn Error>>
{
    for tool in ["", "Bad", "git-push", &"a".repeat(65)] {
        assert!(canonical_action_bytes(tool, &json!({}), &context()?).is_err());
    }
    for value in [
        json!(null),
        json!([]),
        json!({"large":"x".repeat(tqa_types::MAX_FRAME_BYTES)}),
    ] {
        assert!(canonical_action_bytes("git_push", &value, &context()?).is_err());
    }
    let mut nested = json!(null);
    for _ in 0..130 {
        nested = json!({"nested":nested});
    }
    assert!(canonical_action_bytes("git_push", &nested, &context()?).is_err());
    let mut invalid = context()?;
    invalid.attempt = 0;
    assert!(canonical_action_bytes("git_push", &json!({}), &invalid).is_err());
    Ok(())
}
