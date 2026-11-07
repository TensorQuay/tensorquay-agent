//! TOOL-U-032 (calls): with every property required and optional ones nullable, the registry fills an omitted nullable
//! property with null before validating (at any depth the schema describes), so routes that don't enforce the schema
//! keep working; a null behaves exactly as an absent argument; and `read_file` checks its own cursor rule.
#[path = "support/context.rs"]
mod support;
use serde_json::json;
use std::{error::Error, fs, sync::Arc};
use tqa_testkit::workspace_fixture;
use tqa_tools::{ToolError, ToolRegistry, builtin};

type TestResult = Result<(), Box<dyn Error>>;

fn registry() -> Result<ToolRegistry, Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(builtin::ReadFile::new()))?;
    registry.register(Arc::new(builtin::EditFile::new()))?;
    registry.register(Arc::new(builtin::Glob::new()))?;
    registry.register(Arc::new(builtin::Grep::new()))?;
    registry.register(Arc::new(builtin::Shell::new()))?;
    Ok(registry)
}

#[test]
fn tool_u_032_omitted_nullable_arguments_are_filled_with_null() -> TestResult {
    let registry = registry()?;
    let mut grep = json!({"pattern": "alpha"});
    registry.fill_nulls("grep", &mut grep);
    assert_eq!(
        grep,
        json!({"pattern": "alpha", "path": null, "glob": null, "file_type": null, "case_insensitive": null,
            "fixed_string": null, "output_mode": null, "context_lines": null, "max_results": null, "cursor": null})
    );
    // Into the edits array, as the schema describes it; given values stay as they are.
    let mut edit = json!({"path": "a", "edits": [{"old_text": "x", "new_text": "y"}, {"old_text": "p", "new_text": "q", "replace_all": true}]});
    registry.fill_nulls("edit_file", &mut edit);
    assert_eq!(
        edit["edits"],
        json!([{"old_text": "x", "new_text": "y", "replace_all": null}, {"old_text": "p", "new_text": "q", "replace_all": true}])
    );
    // A required, non-nullable property is never invented; an unknown tool is left alone.
    let mut missing = json!({});
    registry.fill_nulls("read_file", &mut missing);
    assert_eq!(
        missing,
        json!({"start_line": null, "end_line": null, "cursor": null})
    );
    let mut other = json!({"a": 1});
    registry.fill_nulls("nope", &mut other);
    assert_eq!(other, json!({"a": 1}));
    Ok(())
}

#[test]
fn tool_u_032_calls_that_omit_or_null_optional_arguments_both_validate() -> TestResult {
    let registry = registry()?;
    for (name, args) in [
        ("grep", json!({"pattern": "alpha"})),
        (
            "grep",
            json!({"pattern": "alpha", "path": null, "output_mode": null}),
        ),
        ("read_file", json!({"path": "hello.txt"})),
        (
            "read_file",
            json!({"path": "hello.txt", "start_line": null, "end_line": null, "cursor": null}),
        ),
        ("glob", json!({"pattern": "*.txt", "max_results": null})),
        ("shell", json!({"command": "true", "cwd": null})),
        (
            "edit_file",
            json!({"path": "hello.txt", "edits": [{"old_text": "alpha", "new_text": "first"}]}),
        ),
    ] {
        assert!(registry.prepare(name, &args).is_ok(), "{name}: {args}");
    }
    // A wrong type is still refused.
    assert!(
        registry
            .prepare("grep", &json!({"pattern": "alpha", "max_results": "ten"}))
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn tool_u_032_a_null_behaves_as_absent() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    let registry = registry()?;
    let absent = registry
        .call("read_file", json!({"path": "hello.txt"}), &cx)
        .await?;
    let null = registry
        .call(
            "read_file",
            json!({"path": "hello.txt", "start_line": null, "end_line": null, "cursor": null}),
            &cx,
        )
        .await?;
    assert_eq!(absent.text, null.text);
    registry
        .call(
            "edit_file",
            json!({"path": "hello.txt", "edits": [{"old_text": "alpha", "new_text": "first", "replace_all": null}]}),
            &cx,
        )
        .await?;
    assert_eq!(
        fs::read_to_string(fixture.root().join("hello.txt"))?,
        "first\nbeta\n"
    );
    Ok(())
}

#[tokio::test]
async fn tool_u_032_read_file_refuses_a_cursor_with_line_arguments_itself() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    let registry = registry()?;
    // A cursor the tool issued (one it didn't, next to a line argument, is a guess and reads the lines: TOOL-U-051).
    fs::write(fixture.root().join("long"), "a".repeat(10000))?;
    let page = registry
        .call(
            "read_file",
            json!({"path": "long", "start_line": null, "end_line": null, "cursor": null}),
            &cx,
        )
        .await?
        .text;
    let cursor = page
        .lines()
        .last()
        .and_then(|notice| notice.split_once("continue with cursor="))
        .and_then(|(_, cursor)| cursor.strip_suffix(']'))
        .ok_or("no cursor")?
        .to_owned();
    for args in [
        json!({"path": "long", "cursor": cursor, "start_line": 1, "end_line": null}),
        json!({"path": "long", "cursor": cursor, "start_line": null, "end_line": 2}),
    ] {
        let error = registry.call("read_file", args.clone(), &cx).await.err();
        assert!(
            matches!(&error, Some(ToolError::InvalidArgs { path, .. }) if path == "/cursor"),
            "{args}: {error:?}"
        );
    }
    Ok(())
}
