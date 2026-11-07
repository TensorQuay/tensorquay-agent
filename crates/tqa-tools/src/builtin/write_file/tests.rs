use super::*;

#[test]
fn tool_u_003_failure_before_rename_preserves_original_and_cleans_stage() -> Result<(), ToolError> {
    let directory = tempfile::tempdir()?;
    let workspace = WorkspaceRoot::open(directory.path())?;
    fs::write(directory.path().join("file"), "original\n")?;
    let args = Args {
        path: "file".into(),
        content: "replacement".into(),
        create_parents: false,
    };
    let result = write(&workspace, &args, &CancellationToken::new(), || {
        Err(ToolError::Io("injected failure".into()))
    });
    assert!(matches!(result, Err(ToolError::Io(_))));
    assert_eq!(
        fs::read_to_string(directory.path().join("file"))?,
        "original\n"
    );
    assert_eq!(fs::read_dir(directory.path())?.count(), 1);
    Ok(())
}
