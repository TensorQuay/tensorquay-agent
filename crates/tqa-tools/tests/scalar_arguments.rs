//! TOOL-U-146: a model server that types a tool call's arguments by the schema may leave a value as text where the
//! schema's `type` is a list. One gave shell's `["integer", "null"]` `timeout_ms` as `"30000"` (18 Nov 2026, the 18.0
//! soak's rehearsal on our own engine: refused four times, the model's own fix `"null"` too, then the repeat guard
//! ended the run); the same request with a plain `"integer"` came back as `30000`. For our own tools the
//! registry takes an exact literal written as a string as the value it spells, where the property's schema takes no
//! string; a near one stays as sent and is refused, and the refusal says what was sent. An external tool's arguments
//! stay exactly as sent (its schema is its own, as TOOL-U-034's nulls).
use serde_json::json;
use std::{error::Error, sync::Arc};
use tqa_tools::{ToolError, ToolRegistry, builtin};

type TestResult = Result<(), Box<dyn Error>>;

fn registry() -> Result<ToolRegistry, Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(builtin::Shell::new()))?;
    registry.register(Arc::new(builtin::ReadFile::new()))?;
    registry.register(Arc::new(builtin::EditFile::new()))?;
    registry.register(Arc::new(builtin::Grep::new()))?;
    Ok(registry)
}

#[test]
fn tool_u_146_exact_literals_written_as_text_are_their_values() -> TestResult {
    let registry = registry()?;
    // The soak's call, as the engine gave it.
    let mut shell = json!({"command": "printf soak-0 > soak.txt; sleep 20; cat soak.txt", "cwd": "/workspace",
        "timeout_ms": "30000"});
    registry.normalize_arguments("shell", &mut shell);
    assert_eq!(shell["timeout_ms"], json!(30000));
    registry.prepare("shell", &shell)?;
    // "null" where the property takes null and no string is the model's null (the soak's third try), and so absent,
    // as TOOL-U-034's nulls are.
    let mut shell = json!({"command": "true", "cwd": null, "timeout_ms": "null"});
    registry.normalize_arguments("shell", &mut shell);
    assert_eq!(shell, json!({"command": "true"}));
    registry.prepare("shell", &shell)?;
    // A boolean, at the depth the schema describes it.
    let mut edit =
        json!({"path": "a", "edits": [{"old_text": "x", "new_text": "y", "replace_all": "true"}]});
    registry.normalize_arguments("edit_file", &mut edit);
    assert_eq!(edit["edits"][0]["replace_all"], json!(true));
    registry.prepare("edit_file", &edit)?;
    // Several in one call.
    let mut read = json!({"path": "a", "start_line": "3", "end_line": "40"});
    registry.normalize_arguments("read_file", &mut read);
    assert_eq!(read, json!({"path": "a", "start_line": 3, "end_line": 40}));
    registry.prepare("read_file", &read)?;
    // A literal that is a value of the type still meets the property's other rules: below the minimum is refused.
    let mut grep = json!({"pattern": "alpha", "max_results": "0"});
    registry.normalize_arguments("grep", &mut grep);
    assert_eq!(grep["max_results"], json!(0));
    assert!(registry.prepare("grep", &grep).is_err());
    Ok(())
}

#[test]
fn tool_u_146_a_property_that_takes_text_keeps_it() -> TestResult {
    let registry = registry()?;
    // A command of digits and a folder named "null" are text to their schemas, never a number or the null.
    let mut shell = json!({"command": "42", "cwd": "null", "timeout_ms": null});
    registry.normalize_arguments("shell", &mut shell);
    assert_eq!(shell, json!({"command": "42", "cwd": "null"}));
    let mut grep = json!({"pattern": "30000"});
    registry.normalize_arguments("grep", &mut grep);
    assert_eq!(grep, json!({"pattern": "30000"}));
    Ok(())
}

#[test]
fn tool_u_146_a_near_literal_stays_as_sent_and_its_refusal_says_what_was_sent() -> TestResult {
    let registry = registry()?;
    for near in [
        " 30000",
        "30000 ",
        "30000.5",
        "3e4",
        "30_000",
        "thirty",
        "NULL",
        "True",
        "[30000]",
        "{\"ms\":1}",
    ] {
        let mut shell = json!({"command": "true", "cwd": null, "timeout_ms": near});
        registry.normalize_arguments("shell", &mut shell);
        assert_eq!(shell["timeout_ms"], json!(near), "{near:?} stays as sent");
        let Err(ToolError::InvalidArgs { path, detail }) = registry.prepare("shell", &shell) else {
            return Err(format!("{near:?} was taken").into());
        };
        assert_eq!(path, "/timeout_ms");
        assert_eq!(detail, "expected null or integer, got a string", "{near:?}");
    }
    // Every other type a model can send is named too.
    for (sent, word) in [
        (json!(true), "a boolean"),
        (json!([1]), "an array"),
        (json!({"ms": 1}), "an object"),
    ] {
        let shell = json!({"command": "true", "cwd": null, "timeout_ms": sent});
        let Err(ToolError::InvalidArgs { detail, .. }) = registry.prepare("shell", &shell) else {
            return Err(format!("{word} was taken").into());
        };
        assert_eq!(detail, format!("expected null or integer, got {word}"));
    }
    Ok(())
}

/// An external (MCP-style) tool's schema is its own: a scalar written as text is sent as the model wrote it.
#[test]
fn tool_u_146_an_external_tools_arguments_stay_exactly_as_sent() -> TestResult {
    let spec = tqa_tools::ToolSpec {
        name: tqa_tools::ToolName::new("file_issue")?,
        description: "File an issue".into(),
        parameters: json!({"type":"object","required":["title","priority"],
            "properties":{"title":{"type":"string"},"priority":{"type":["integer","null"]}}}),
        parallel_safe: false,
        changes_workspace: false,
    };
    let mut registry = registry()?;
    registry.register(Arc::new(tqa_testkit::FakeTool::new(spec, vec![])))?;
    let mut args = json!({"title": "t", "priority": "2"});
    registry.normalize_arguments("file_issue", &mut args);
    assert_eq!(
        args,
        json!({"title": "t", "priority": "2"}),
        "sent exactly as the model wrote it"
    );
    Ok(())
}
