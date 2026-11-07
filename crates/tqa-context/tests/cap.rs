//! CTX-U-006: bounded UTF-8 previews retain the complete, private spill file.
use std::{error::Error, fs};
use tqa_context::{ContextError, cap_tool_result};

#[test]
fn ctx_u_006_spill_is_private_and_preserves_head_tail() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let spill = directory.path().join(".tqa/conversation/spill");
    let call = "call".parse()?;
    let short = cap_tool_result(&call, "small", 1024, &spill)?;
    assert_eq!(short.content, "small");
    assert!(short.spilled.is_none());
    assert!(!spill.exists());
    let text = format!("HEAD{}TAIL", "é界".repeat(1000));
    let result = cap_tool_result(&call, &text, 1024, &spill)?;
    assert!(result.content.len() <= 1024);
    assert!(result.content.starts_with("HEAD") && result.content.ends_with("TAIL"));
    assert!(result.content.contains("[output truncated: 5008 bytes total; full output saved to .tqa/conversation/spill/call.txt]"));
    assert!(!result.content.contains('\u{fffd}'));
    let path = result.spilled.ok_or("missing spill")?;
    assert_eq!(fs::read_to_string(&path)?, text);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
    }
    Ok(())
}

#[test]
fn ctx_u_006_rejects_invalid_limits_paths_and_io_errors() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let call = "call".parse()?;
    assert!(matches!(
        cap_tool_result(&call, "short", 1023, directory.path()),
        Err(ContextError::InvalidLimit)
    ));
    let text = "x".repeat(2048);
    for path in [
        directory.path().to_path_buf(),
        directory.path().join(".tqa/bad!id/spill"),
    ] {
        assert!(matches!(
            cap_tool_result(&call, &text, 1024, &path),
            Err(ContextError::InvalidSpillPath)
        ));
    }
    fs::write(directory.path().join(".tqa"), "obstruction")?;
    assert!(matches!(
        cap_tool_result(
            &call,
            &text,
            1024,
            &directory.path().join(".tqa/conversation/spill")
        ),
        Err(ContextError::Io(_))
    ));
    Ok(())
}

#[cfg(unix)]
#[test]
fn ctx_u_006_spill_replaces_existing_symlinks_without_following_them() -> Result<(), Box<dyn Error>>
{
    let directory = tempfile::tempdir()?;
    let spill = directory.path().join(".tqa/conversation/spill");
    fs::create_dir_all(&spill)?;
    let outside = directory.path().join("untouched");
    fs::write(&outside, "original")?;
    std::os::unix::fs::symlink(&outside, spill.join("call.txt"))?;
    cap_tool_result(&"call".parse()?, &"x".repeat(2048), 1024, &spill)?;
    assert_eq!(fs::read_to_string(outside)?, "original");
    assert!(!fs::symlink_metadata(spill.join("call.txt"))?.is_symlink());
    Ok(())
}

#[test]
fn ctx_u_006_an_ascii_cut_fills_the_limit_exactly() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let spill = directory.path().join(".tqa/conversation/spill");
    let result = cap_tool_result(&"call".parse()?, &"a".repeat(5000), 1024, &spill)?;
    assert_eq!(
        result.content.len(),
        1024,
        "head, marker and tail use the whole limit"
    );
    Ok(())
}

#[test]
fn ctx_u_006_a_cut_inside_a_character_moves_inward_and_stays_within_the_limit()
-> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let spill = directory.path().join(".tqa/conversation/spill");
    // The tail's cut always splits a two-byte character here; with the one-byte prefix the head's cut does too, and
    // each cut moves inward by one byte.
    for (prefix, expected) in [("", 1023), ("a", 1022)] {
        let text = format!("{prefix}{}", "é".repeat(3000));
        let result = cap_tool_result(&"call".parse()?, &text, 1024, &spill)?;
        assert_eq!(result.content.len(), expected, "{prefix:?}");
        assert!(result.content.ends_with('é') && !result.content.contains('\u{fffd}'));
    }
    Ok(())
}

#[test]
fn ctx_u_006_each_part_of_the_spill_path_is_checked() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let text = "x".repeat(2048);
    for path in [".tqa/conversation/other", "other/conversation/spill"] {
        assert!(
            matches!(
                cap_tool_result(&"call".parse()?, &text, 1024, &directory.path().join(path)),
                Err(ContextError::InvalidSpillPath)
            ),
            "{path}"
        );
    }
    Ok(())
}
