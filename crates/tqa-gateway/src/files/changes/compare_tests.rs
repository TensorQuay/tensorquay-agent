//! GW-I-048 run changes: statuses, line counts and a bounded unified diff between two manifests.
use super::super::manifest::tests::{limits, scan_at, tree};
use super::*;
use std::fs;

type Result = std::result::Result<(), Box<dyn std::error::Error>>;

// TEST: GW-I-013
#[test]
fn gw_i_048_lists_added_modified_and_deleted_files_with_line_counts() -> Result {
    let (_temporary, root, blobs) = tree()?;
    let before = scan_at(&root, None, &limits(), &blobs)?;
    fs::write(root.join("README.md"), "# Title\nMore\n")?;
    fs::remove_file(root.join("src/app.py"))?;
    fs::write(root.join("notes.txt"), "a\nb\nc\n")?;
    fs::write(root.join("src/data.bin"), [0_u8, 9])?;
    fs::write(root.join("src/big.txt"), "y".repeat(70))?;
    let after = scan_at(&root, Some(&before), &limits(), &blobs)?;
    let changes = changes(&before, &after, &blobs);
    let summary: Vec<_> = changes
        .iter()
        .map(|change| {
            (
                change.path.as_str(),
                change.status,
                change.added,
                change.removed,
                change.binary,
                change.too_large,
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("README.md", Status::Modified, 1, 0, false, false),
            ("notes.txt", Status::Added, 3, 0, false, false),
            ("src/app.py", Status::Deleted, 0, 1, false, false),
            ("src/big.txt", Status::Modified, 0, 0, false, true),
            ("src/data.bin", Status::Modified, 0, 0, true, false),
        ]
    );
    Ok(())
}

// TEST: GW-I-013
#[test]
fn gw_i_048_unchanged_workspaces_have_no_changes() -> Result {
    let (_temporary, root, blobs) = tree()?;
    let before = scan_at(&root, None, &limits(), &blobs)?;
    let after = scan_at(&root, Some(&before), &limits(), &blobs)?;
    assert!(changes(&before, &after, &blobs).is_empty());
    Ok(())
}

// TEST: GW-I-013
#[test]
fn gw_i_048_unified_diff_of_one_file_is_bounded() -> Result {
    let (_temporary, root, blobs) = tree()?;
    let before = scan_at(&root, None, &limits(), &blobs)?;
    fs::write(root.join("README.md"), "# New title\n")?;
    let after = scan_at(&root, Some(&before), &limits(), &blobs)?;
    let diff = unified(&before, &after, &blobs, "README.md", 4096).ok_or("diff")?;
    assert_eq!(diff.status, Status::Modified);
    assert_eq!(
        diff.unified.as_deref(),
        Some("--- a/README.md\n+++ b/README.md\n@@ -1 +1 @@\n-# Title\n+# New title\n")
    );
    assert!(!diff.truncated);
    let short = unified(&before, &after, &blobs, "README.md", 20).ok_or("diff")?;
    assert!(short.truncated);
    assert!(
        short
            .unified
            .as_deref()
            .is_some_and(|text| text.len() <= 20)
    );
    // Only a changed file has a diff.
    assert!(unified(&before, &after, &blobs, "src/app.py", 4096).is_none());
    assert!(unified(&before, &after, &blobs, "nothing.txt", 4096).is_none());
    Ok(())
}

// TEST: GW-I-013
#[test]
fn gw_i_048_binary_and_uncopied_files_have_no_diff_text() -> Result {
    let (_temporary, root, blobs) = tree()?;
    let before = scan_at(&root, None, &limits(), &blobs)?;
    fs::write(root.join("src/data.bin"), [0_u8, 7])?;
    fs::write(root.join("src/big.txt"), "z".repeat(70))?;
    let after = scan_at(&root, Some(&before), &limits(), &blobs)?;
    let binary = unified(&before, &after, &blobs, "src/data.bin", 4096).ok_or("binary")?;
    assert!(binary.binary && binary.unified.is_none());
    let big = unified(&before, &after, &blobs, "src/big.txt", 4096).ok_or("big")?;
    assert!(big.too_large && big.unified.is_none());
    Ok(())
}

// TEST: GW-I-048
// The person's uploads: an edit to one, or its deletion, is the run's change; a file added under uploads/ is the
// person's own upload (they are never overwritten), even when it lands after the run, so it is never listed. A folder
// named uploads deeper in the tree is ordinary work.
#[test]
fn gw_i_048_an_edited_upload_is_a_change_and_a_new_upload_is_not() -> Result {
    let (_temporary, root, blobs) = tree()?;
    fs::create_dir_all(root.join("uploads"))?;
    fs::write(root.join("uploads/prices.py"), "PRICE = 1\n")?;
    fs::write(root.join("uploads/old.csv"), "a\n")?;
    let before = scan_at(&root, None, &limits(), &blobs)?;
    fs::write(root.join("uploads/prices.py"), "PRICE = 2\n")?;
    fs::remove_file(root.join("uploads/old.csv"))?;
    fs::write(root.join("uploads/later.csv"), "b\n")?;
    fs::create_dir_all(root.join("src/uploads"))?;
    fs::write(root.join("src/uploads/made.txt"), "c\n")?;
    let after = scan_at(&root, Some(&before), &limits(), &blobs)?;
    let listed: Vec<_> = changes(&before, &after, &blobs)
        .into_iter()
        .map(|change| (change.path, change.status))
        .collect();
    assert_eq!(
        listed,
        [
            ("src/uploads/made.txt".to_owned(), Status::Added),
            ("uploads/old.csv".to_owned(), Status::Deleted),
            ("uploads/prices.py".to_owned(), Status::Modified),
        ]
    );
    assert!(unified(&before, &after, &blobs, "uploads/later.csv", 4096).is_none());
    assert!(unified(&before, &after, &blobs, "uploads/prices.py", 4096).is_some());
    Ok(())
}
