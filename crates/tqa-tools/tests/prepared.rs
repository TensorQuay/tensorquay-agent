//! SEC-019: immutable call bytes and execution context determine the call identity the Git relay binds one push to.
use serde_json::{Value, json};
use std::error::Error;
use tqa_tools::{PreparedAction, ToolName};
use tqa_types::PermissionContext;

type TestResult = Result<(), Box<dyn Error>>;
fn context() -> Result<PermissionContext, Box<dyn Error>> {
    Ok(serde_json::from_value(
        json!({"conversation_id":"c","run_id":"r",
        "step":1,"attempt":1,"call_id":"call"}),
    )?)
}
fn action(arguments: Value, context: PermissionContext) -> Result<PreparedAction, Box<dyn Error>> {
    Ok(PreparedAction::new(
        ToolName::new("shell")?,
        arguments,
        context,
    )?)
}

#[test]
fn sec_019_canonical_action_matches_the_fixed_digest_and_freezes_arguments() -> TestResult {
    let mut arguments = json!({"command":"echo hi","env":{"B":"two","A":"one"}});
    let prepared = action(arguments.clone(), context()?)?;
    assert_eq!(
        prepared.action_hash().as_str(),
        "19d7bc7e6c6b32425dbba990d6de7ba201c3fa6e080808e2f3f54d70de4a3b7f"
    );
    let reordered = action(
        json!({"env":{"A":"one","B":"two"},"command":"echo hi"}),
        context()?,
    )?;
    assert_eq!(prepared.action_hash(), reordered.action_hash());
    arguments["command"] = json!("changed after preparation");
    assert_eq!(prepared.arguments()["command"], "echo hi");
    assert_eq!(prepared.context(), &context()?);
    assert_eq!(prepared.tool().as_str(), "shell");
    let (name, frozen) = prepared.into_call();
    assert_eq!(name.as_str(), "shell");
    assert_eq!(frozen["command"], "echo hi");
    Ok(())
}

#[test]
fn sec_019_each_context_field_tool_and_argument_changes_the_digest() -> TestResult {
    let original = action(json!({"command":"echo hi"}), context()?)?;
    for (field, replacement) in [
        ("conversation_id", json!("other")),
        ("run_id", json!("other")),
        ("step", json!(2)),
        ("attempt", json!(2)),
        ("call_id", json!("other")),
    ] {
        let mut changed = serde_json::to_value(context()?)?;
        changed[field] = replacement;
        assert_ne!(
            original.action_hash(),
            action(
                json!({"command":"echo hi"}),
                serde_json::from_value(changed)?
            )?
            .action_hash(),
            "{field}"
        );
    }
    assert_ne!(
        original.action_hash(),
        action(json!({"command":"echo changed"}), context()?)?.action_hash()
    );
    let different_tool = PreparedAction::new(
        ToolName::new("other")?,
        json!({"command":"echo hi"}),
        context()?,
    )?;
    assert_ne!(original.action_hash(), different_tool.action_hash());
    assert_ne!(
        action(json!({"items":[1,2]}), context()?)?.action_hash(),
        action(json!({"items":[2,1]}), context()?)?.action_hash()
    );
    Ok(())
}

#[test]
fn sec_019_preparation_rejects_invalid_context_shape_and_oversized_input() -> TestResult {
    for arguments in [
        Value::Null,
        json!([]),
        json!({"command":"x".repeat(tqa_types::MAX_FRAME_BYTES)}),
    ] {
        assert!(action(arguments, context()?).is_err());
    }
    for (step, attempt) in [(0, 1), (1, 0)] {
        let mut invalid = context()?;
        invalid.step = step;
        invalid.attempt = attempt;
        assert!(action(json!({}), invalid).is_err());
    }
    let mut nested = Value::Null;
    for _ in 0..130 {
        nested = json!({"nested":nested});
    }
    assert!(action(nested, context()?).is_err());
    Ok(())
}
