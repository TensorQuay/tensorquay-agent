//! CTX-U-014: run notes are a conversation's own bounded file, written atomically and read back.
#[path = "support/context.rs"]
mod context;
use serde_json::json;
use std::error::Error;
use tqa_tools::{
    MAX_NOTES_BYTES, Tool, ToolError, ToolRegistry,
    builtin::{ReadNotes, WriteNotes},
};
use tqa_types::ConversationId;

type TestResult = Result<(), Box<dyn Error>>;

fn conversation() -> Result<ConversationId, Box<dyn Error>> {
    Ok("conversation-1".parse()?)
}

#[tokio::test]
async fn ctx_u_014_notes_are_written_atomically_and_read_back() -> TestResult {
    let fixture = tqa_testkit::workspace_fixture()?;
    let cx = context::context(&fixture);
    let (write, read) = (
        WriteNotes::new(&conversation()?),
        ReadNotes::new(&conversation()?),
    );
    assert_eq!(read.call(json!({}), &cx).await?.text, "No notes yet.");
    write.call(json!({"text": "goal: ship\n"}), &cx).await?;
    write
        .call(json!({"text": "goal: ship\ndone: parser\n"}), &cx)
        .await?;
    let path = fixture.root().join(".tqa/conversation-1/notes.md");
    assert_eq!(
        std::fs::read_to_string(&path)?,
        "goal: ship\ndone: parser\n"
    );
    assert_eq!(
        read.call(json!({}), &cx).await?.text,
        "goal: ship\ndone: parser\n"
    );
    // A new sandbox is a new process with new tool instances over the same workspace.
    let later = ReadNotes::new(&conversation()?);
    assert_eq!(
        later.call(json!({}), &cx).await?.text,
        "goal: ship\ndone: parser\n"
    );
    let other = ReadNotes::new(&"conversation-2".parse()?);
    assert_eq!(other.call(json!({}), &cx).await?.text, "No notes yet.");
    Ok(())
}

#[tokio::test]
async fn ctx_u_014_notes_are_bounded_inclusively() -> TestResult {
    let fixture = tqa_testkit::workspace_fixture()?;
    let cx = context::context(&fixture);
    let write = WriteNotes::new(&conversation()?);
    let exact = "é".repeat(MAX_NOTES_BYTES / 2);
    write.call(json!({"text": exact}), &cx).await?;
    let over = format!("{exact}a");
    let error = write
        .call(json!({"text": over}), &cx)
        .await
        .err()
        .ok_or("oversized notes accepted")?;
    assert_eq!(error.code(), "invalid_args");
    let read = ReadNotes::new(&conversation()?);
    assert_eq!(read.call(json!({}), &cx).await?.text, exact);
    assert_eq!(MAX_NOTES_BYTES, 16 * 1024);
    Ok(())
}

#[tokio::test]
async fn ctx_u_014_notes_stay_inside_the_workspace() -> TestResult {
    let fixture = tqa_testkit::workspace_fixture()?;
    let cx = context::context(&fixture);
    std::fs::create_dir_all(fixture.root().join(".tqa"))?;
    std::os::unix::fs::symlink(
        fixture.outside(),
        fixture.root().join(".tqa/conversation-1"),
    )?;
    let error = WriteNotes::new(&conversation()?)
        .call(json!({"text": "escape"}), &cx)
        .await
        .err()
        .ok_or("notes followed a symlink out of the workspace")?;
    assert!(matches!(
        error,
        ToolError::PathOutsideWorkspace | ToolError::PathInvalid(_)
    ));
    assert!(!fixture.outside().join("notes.md").exists());
    let read = ReadNotes::new(&conversation()?).call(json!({}), &cx).await;
    assert!(read.is_err(), "reading through the symlink must fail");
    Ok(())
}

#[test]
fn ctx_u_014_specs_and_schemas_are_strict() -> TestResult {
    let (write, read) = (
        WriteNotes::new(&conversation()?),
        ReadNotes::new(&conversation()?),
    );
    assert_eq!(write.spec().name.as_str(), "write_notes");
    assert!(!write.spec().parallel_safe);
    assert_eq!(read.spec().name.as_str(), "read_notes");
    assert!(read.spec().parallel_safe);
    let mut registry = ToolRegistry::new();
    registry.register(std::sync::Arc::new(write))?;
    registry.register(std::sync::Arc::new(read))?;
    assert!(
        registry
            .prepare("write_notes", &json!({"text": "x"}))
            .is_ok()
    );
    for invalid in [
        json!({}),
        json!({"text": 1}),
        json!({"text": "x", "path": "y"}),
    ] {
        assert!(
            registry.prepare("write_notes", &invalid).is_err(),
            "{invalid}"
        );
    }
    assert!(registry.prepare("read_notes", &json!({})).is_ok());
    assert!(
        registry
            .prepare("read_notes", &json!({"path": "x"}))
            .is_err()
    );
    Ok(())
}
