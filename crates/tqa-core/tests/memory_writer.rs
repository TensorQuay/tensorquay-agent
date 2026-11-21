//! CORE-U-094 (ADR-0041's third amendment, A11): the runner hands the file tools the note writer only with long-term
//! memory on. On, a note the model writes is guarded and kept for one Undo (a new note is marked new); off, `.memory/`
//! is an ordinary folder and nothing is kept.
mod support;
use std::sync::Arc;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::Runner;
use tqa_tools::{ToolRegistry, builtin::WriteFile};

fn writes_a_note() -> Result<Harness, Box<dyn std::error::Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(WriteFile::new()))?;
    Harness::new(
        vec![
            calls(&[(
                "note-1",
                "write_file",
                r#"{"path":".memory/build.md","create_parents":true,"content":"about: make all builds\n"}"#,
            )]),
            answer(),
        ],
        registry,
    )
}

/// Runs the turn; the harness's folder stays until the test ends (the caller keeps it).
async fn run(deps: tqa_core::RunnerDeps) -> TestResult {
    Runner::new(deps)?
        .run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    Ok(())
}

// TEST: CORE-U-094
#[tokio::test]
async fn core_u_094_with_memory_on_a_new_note_is_kept_for_its_undo() -> TestResult {
    let mut h = writes_a_note()?;
    h.deps.memory = true;
    let root = h.directory.path().to_owned();
    let _kept = h.directory;
    run(h.deps).await?;
    assert!(root.join(".memory/build.md").is_file());
    assert!(
        root.join(".memory/.undo/build.md.new").is_file(),
        "a new note's Undo removes it: its mark is kept"
    );
    Ok(())
}

// TEST: CORE-U-094
#[tokio::test]
async fn core_u_094_with_memory_off_nothing_is_kept() -> TestResult {
    let h = writes_a_note()?;
    let root = h.directory.path().to_owned();
    let _kept = h.directory;
    run(h.deps).await?;
    assert!(root.join(".memory/build.md").is_file());
    assert!(!root.join(".memory/.undo").exists());
    Ok(())
}
