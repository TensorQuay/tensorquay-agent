use super::*;
use std::{
    fs::{self, File},
    time::{Duration, UNIX_EPOCH},
};

#[test]
fn tool_u_004_concurrent_length_or_mtime_change_is_not_overwritten() -> Result<(), ToolError> {
    let directory = tempfile::tempdir()?;
    let workspace = WorkspaceRoot::open(directory.path())?;
    let path = directory.path().join("file");
    for changed in ["concurrent replacement", "racing"] {
        fs::write(&path, "before")?;
        File::options()
            .write(true)
            .open(&path)?
            .set_modified(UNIX_EPOCH + Duration::from_secs(1000))?;
        let args = Args {
            path: "file".into(),
            edits: vec![Edit {
                old_text: "before".into(),
                new_text: "edited".into(),
                replace_all: false,
            }],
        };
        let error = apply(&workspace, &args, 1024, &CancellationToken::new(), || {
            fs::write(&path, changed)?;
            File::options()
                .write(true)
                .open(&path)?
                .set_modified(UNIX_EPOCH + Duration::from_secs(2000))?;
            Ok(())
        })
        .err()
        .ok_or_else(|| ToolError::Io("expected conflict".into()))?;
        assert!(
            error
                .model_message()
                .contains("file changed while editing; read it again")
        );
        assert_eq!(fs::read_to_string(&path)?, changed);
        assert_eq!(fs::read_dir(directory.path())?.count(), 1);
    }
    Ok(())
}
