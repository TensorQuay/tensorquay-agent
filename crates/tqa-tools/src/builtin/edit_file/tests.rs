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

// Overlapping matches are counted up to a bound, so an old_text of one repeated character in a file of it
// costs a bounded number of finds, not one per position.
#[test]
fn tool_u_004_overlapping_matches_are_counted_to_a_bound() {
    let (listed, total) = overlapping(&"a".repeat(5000), &"a".repeat(64));
    assert_eq!(listed.len(), miss::LISTED);
    assert_eq!(total, miss::COUNTED);
    let (listed, total) = overlapping("x a a a y", "a");
    assert_eq!((listed, total), (vec![2, 4, 6], 3));
}
