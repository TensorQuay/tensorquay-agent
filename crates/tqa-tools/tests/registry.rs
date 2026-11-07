//! TOOL-U-011: dispatch only after schema validation.
#[path = "support/context.rs"]
mod support;
use serde_json::json;
use std::{error::Error, sync::Arc, time::Duration};
use tqa_testkit::{FakeTool, FakeToolReply, workspace_fixture};
use tqa_tools::{ToolError, ToolName, ToolOutput, ToolRegistry, ToolSpec};

fn spec() -> Result<ToolSpec, ToolError> {
    Ok(ToolSpec {
        name: ToolName::new("record")?,
        description: "Test tool".into(),
        parameters: json!({"type":"object","required":["path"],"properties":{"path":{"type":"string"}},"additionalProperties":false}),
        parallel_safe: true,
        changes_workspace: false,
    })
}

#[test]
fn tool_u_011_prepare_validates_without_dispatch() -> Result<(), Box<dyn Error>> {
    let fake = Arc::new(FakeTool::new(spec()?, vec![]));
    let mut registry = ToolRegistry::new();
    registry.register(fake.clone())?;
    assert_eq!(
        registry
            .prepare("record", &json!({"path":"ok"}))?
            .name
            .as_str(),
        "record"
    );
    assert!(matches!(
        registry.prepare("record", &json!({"path":42})),
        Err(ToolError::InvalidArgs { .. })
    ));
    let error = registry
        .prepare("missing", &json!({}))
        .err()
        .ok_or("expected missing tool")?;
    assert!(error.model_message().contains("record"));
    assert!(error.model_message().chars().count() <= 500);
    assert!(fake.calls().is_empty());
    Ok(())
}

#[tokio::test]
async fn tool_u_011_validates_before_dispatch_and_rejects_unknown() -> Result<(), Box<dyn Error>> {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    let fake = Arc::new(FakeTool::new(
        spec()?,
        vec![FakeToolReply {
            delay: Duration::ZERO,
            result: Ok(ToolOutput::text("called")),
        }],
    ));
    let mut registry = ToolRegistry::new();
    registry.register(fake.clone())?;
    assert!(registry.get("record").is_some());
    assert_eq!(registry.specs().count(), 1);
    let error = registry.call("record", json!({"path":42}), &cx).await;
    assert!(matches!(error, Err(ToolError::InvalidArgs { ref path, .. }) if path == "/path"));
    assert!(fake.calls().is_empty());
    assert!(matches!(
        registry.call("missing", json!({}), &cx).await,
        Err(ToolError::Unsupported(_))
    ));
    assert_eq!(
        registry
            .call("record", json!({"path":"ok"}), &cx)
            .await?
            .text,
        "called"
    );
    assert_eq!(fake.calls(), vec![json!({"path":"ok"})]);
    assert!(registry.register(fake).is_err());
    Ok(())
}

#[test]
fn tool_u_011_names_and_invalid_schema_are_rejected() -> Result<(), Box<dyn Error>> {
    for name in ["", "A", "a-b", "mcp.server.tool", "mcp.", "a.b", "1a", "é"] {
        assert!(ToolName::new(name).is_err(), "{name}");
    }
    assert!(ToolName::new("a".repeat(64)).is_ok());
    assert!(ToolName::new("a".repeat(65)).is_err());
    for schema in [
        json!(true),
        json!({"type":"invalid"}),
        json!({"$ref":"https://schemas.example/external.json"}),
        json!({"$ref":"file:///workspace/schema.json"}),
    ] {
        let mut specification = spec()?;
        specification.parameters = schema;
        assert!(
            ToolRegistry::new()
                .register(Arc::new(FakeTool::new(specification, vec![])))
                .is_err()
        );
    }
    Ok(())
}

#[tokio::test]
async fn core_u_018_schema_errors_name_properties_without_argument_values()
-> Result<(), Box<dyn Error>> {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(tqa_tools::builtin::EditFile::new()))?;
    registry.register(Arc::new(tqa_tools::builtin::ReadFile::new()))?;
    let args = json!({"path":"private-path-marker","old_string":"private-old-marker","new_string":"private-new-marker"});
    let error = registry
        .call("edit_file", args.clone(), &cx)
        .await
        .err()
        .ok_or("expected invalid schema")?;
    let message = error.model_message();
    assert!(
        message.contains("edits") && message.contains("old_string"),
        "{message}"
    );
    assert!(message.starts_with("invalid_args: /:"), "{message}");
    assert!(!message.contains("private-"));
    assert_eq!(registry.prepare("edit_file", &args).err(), Some(error));
    let message = registry
        .prepare("read_file", &json!({"path":42}))
        .err()
        .ok_or("expected type error")?
        .model_message();
    assert!(message.contains("expected string"), "{message}");
    assert!(!message.contains("42"));
    Ok(())
}

#[test]
fn core_u_018_schema_errors_bound_details_and_list_schema_constraints() -> Result<(), Box<dyn Error>>
{
    for (schema, args, detail) in [
        (
            json!({"type":"string","minLength":3}),
            json!("x"),
            "at least 3 characters",
        ),
        (
            json!({"type":"string","maxLength":2}),
            json!("private-value"),
            "at most 2 characters",
        ),
        (
            json!({"type":"array","minItems":2}),
            json!([]),
            "at least 2 items",
        ),
        (
            json!({"type":"array","maxItems":1}),
            json!([1, 2]),
            "at most 1 items",
        ),
        (json!({"minimum":2}), json!(1), "at least 2"),
        (json!({"exclusiveMaximum":2}), json!(2), "less than 2"),
        (
            json!({"enum":["small","large"]}),
            json!("private-value"),
            "small",
        ),
    ] {
        let mut spec = spec()?;
        spec.parameters = schema;
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(FakeTool::new(spec, vec![])))?;
        let text = registry
            .prepare("record", &args)
            .err()
            .ok_or("expected invalid schema")?
            .model_message();
        assert!(text.contains(detail), "{text}");
        assert!(!text.contains("private-value"));
        assert!(text.chars().count() <= 500);
    }
    Ok(())
}

#[tokio::test]
async fn tool_u_011_a_prepared_call_is_validated_before_dispatch_and_offered_tools_are_listed()
-> Result<(), Box<dyn Error>> {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    let fake = Arc::new(FakeTool::new(
        spec()?,
        vec![FakeToolReply {
            delay: Duration::ZERO,
            result: Ok(ToolOutput::text("prepared")),
        }],
    ));
    let mut registry = ToolRegistry::new();
    registry.register(fake.clone())?;
    let names: Vec<&str> = registry
        .offered_specs()
        .map(|spec| spec.name.as_str())
        .collect();
    assert_eq!(names, ["record"], "a tool is offered unless it declines");
    let context: tqa_types::PermissionContext = serde_json::from_value(
        json!({"conversation_id": "c", "run_id": "r", "step": 1, "attempt": 1, "call_id": "call"}),
    )?;
    let action = |arguments| -> Result<tqa_tools::PreparedAction, Box<dyn Error>> {
        Ok(tqa_tools::PreparedAction::new(
            ToolName::new("record")?,
            arguments,
            context.clone(),
        )?)
    };
    let refused = registry
        .call_prepared(action(json!({"path": 42}))?, &cx)
        .await;
    assert!(matches!(refused, Err(ToolError::InvalidArgs { ref path, .. }) if path == "/path"));
    assert!(
        fake.prepared_calls().is_empty(),
        "nothing invalid is dispatched"
    );
    let output = registry
        .call_prepared(action(json!({"path": "ok"}))?, &cx)
        .await?;
    assert_eq!(output.text, "prepared");
    assert_eq!(
        fake.prepared_calls().len(),
        1,
        "the valid call is dispatched once"
    );
    // A cancelled run's prepared call is refused before validation or dispatch.
    cx.cancel.cancel();
    let cancelled = registry
        .call_prepared(action(json!({"path": "ok"}))?, &cx)
        .await;
    assert!(
        matches!(cancelled, Err(ToolError::Cancelled)),
        "{cancelled:?}"
    );
    assert_eq!(fake.prepared_calls().len(), 1);
    Ok(())
}

#[test]
fn tool_u_011_a_rule_without_its_own_words_still_names_the_field() -> Result<(), Box<dyn Error>> {
    let spec = ToolSpec {
        name: ToolName::new("tagged")?,
        description: "Test tool".into(),
        parameters: json!({"type": "object", "properties": {"tag": {"type": "string", "pattern": "^[a-z]+$"}}}),
        parallel_safe: true,
        changes_workspace: false,
    };
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(FakeTool::new(spec, vec![])))?;
    let refused = registry.prepare("tagged", &json!({"tag": "NOT LOWER"}));
    assert!(
        matches!(refused, Err(ToolError::InvalidArgs { ref path, ref detail }) if path == "/tag" && detail == "does not satisfy the schema"),
        "{refused:?}"
    );
    Ok(())
}
