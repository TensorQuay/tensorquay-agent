//! TOOL-U-001/003/005/015 and TOOL-S-002: filesystem tools and their contracts.
#[path = "support/context.rs"]
mod support;
use serde_json::json;
use std::{error::Error, fs};
use tqa_testkit::workspace_fixture;
use tqa_tools::{
    ProxyEndpoint, Tool, ToolError,
    builtin::{EditFile, FetchUrl, Glob, Grep, ReadFile, ReadNotes, Shell, WriteFile, WriteNotes},
};

type TestResult = Result<(), Box<dyn Error>>;

#[tokio::test]
async fn tool_u_001_reads_ranges_and_truncates() -> TestResult {
    let fixture = workspace_fixture()?;
    let mut cx = support::context(&fixture);
    let tool = ReadFile::new();
    assert_eq!(
        tool.call(json!({"path":"hello.txt"}), &cx).await?.text,
        "     1\talpha\n     2\tbeta\n"
    );
    assert_eq!(
        tool.call(json!({"path":"hello.txt","start_line":2,"end_line":2}), &cx)
            .await?
            .text,
        "     2\tbeta\n"
    );
    cx.limits.read_page_lines = 1;
    assert_eq!(
        tool.call(json!({"path":"hello.txt"}), &cx).await?.text,
        "     1\talpha\n[partial: lines 1-1 of unknown; continue with start_line=2]"
    );
    assert_eq!(
        tool.call(json!({"path":"hello.txt","start_line":100}), &cx)
            .await?
            .text,
        "[end of file: 2 lines; start_line 100 is past the end]"
    );
    Ok(())
}

#[tokio::test]
async fn tool_u_001_missing_binary_and_empty() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    let tool = ReadFile::new();
    assert!(matches!(
        tool.call(json!({"path":"missing"}), &cx).await,
        Err(ToolError::NotFound)
    ));
    for bytes in [vec![0, 1, 2], vec![0xff, 0xfe]] {
        fs::write(fixture.root().join("bytes"), &bytes)?;
        let output = tool.call(json!({"path":"bytes"}), &cx).await?.text;
        assert!(output.starts_with("[binary file,"));
        assert_eq!(output.lines().count(), 1);
    }
    fs::write(fixture.root().join("empty"), "")?;
    assert_eq!(
        tool.call(json!({"path":"empty"}), &cx).await?.text,
        "[end of file: 0 lines]"
    );
    Ok(())
}

#[tokio::test]
async fn tool_u_003_creates_parents_replaces_and_counts_lines() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    let tool = WriteFile::new();
    let args = json!({"path":"new/nested/file","content":"one\ntwo\n","create_parents":true});
    let output = tool.call(args, &cx).await?;
    assert_eq!(
        fs::read_to_string(fixture.root().join("new/nested/file"))?,
        "one\ntwo\n"
    );
    assert_eq!(
        output.text,
        "created, 8 bytes\nadded: 2 lines\nremoved: 0 lines"
    );
    let output = tool
        .call(
            json!({"path":"new/nested/file","content":"replacement","create_parents":false}),
            &cx,
        )
        .await?;
    assert_eq!(
        output.text,
        "replaced, 11 bytes\nadded: 1 lines\nremoved: 2 lines"
    );
    assert_eq!(
        fs::read_to_string(fixture.root().join("new/nested/file"))?,
        "replacement"
    );
    assert!(
        tool.call(
            json!({"path":"absent/file","content":"x","create_parents":false}),
            &cx
        )
        .await
        .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn tool_s_002_write_rejects_escapes_and_reaches_git_internals() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        fixture.root().join(".git"),
        fixture.root().join("git_alias"),
    )?;
    for path in ["../x", "escape/secret.txt"] {
        let result = WriteFile::new()
            .call(
                json!({"path":path,"content":"bad","create_parents":true}),
                &cx,
            )
            .await;
        assert!(
            matches!(result, Err(ToolError::PathOutsideWorkspace)),
            "{path}: {result:?}"
        );
    }
    assert_eq!(
        fs::read_to_string(fixture.outside().join("secret.txt"))?,
        "outside\n"
    );
    // ADR-0035: the paths mean /workspace, git internals included; that's a path contract, not a permission.
    for (path, written) in [
        (".git/config", ".git/config"),
        (".git/../hello.txt", "hello.txt"),
        ("git_alias/description", ".git/description"),
    ] {
        WriteFile::new()
            .call(
                json!({"path":path,"content":path,"create_parents":true}),
                &cx,
            )
            .await?;
        assert_eq!(fs::read_to_string(fixture.root().join(written))?, path);
    }
    Ok(())
}

#[tokio::test]
async fn tool_u_005_glob_is_sorted_bounded_and_ignores_directories() -> TestResult {
    let fixture = workspace_fixture()?;
    for path in ["node_modules", "target", ".next", "ignored"] {
        fs::create_dir(fixture.root().join(path))?;
        fs::write(fixture.root().join(path).join("hidden.txt"), "hidden")?;
    }
    fs::write(fixture.root().join(".tqaignore"), "ignored\n")?;
    fs::write(
        fixture.root().join(".gitignore"),
        "node_modules/\ntarget/\n",
    )?;
    let mut cx = support::context(&fixture);
    let output = Glob::new()
        .call(json!({"pattern":"**/*.txt"}), &cx)
        .await?
        .text;
    assert_eq!(output, "hello.txt\nnested/child.txt");
    cx.limits.glob_results = 1;
    let output = Glob::new()
        .call(json!({"pattern":"**/*.txt"}), &cx)
        .await?
        .text;
    assert!(output.contains("[partial: continue with cursor="));
    assert_eq!(
        output.lines().filter(|line| !line.starts_with('[')).count(),
        1
    );
    cx.limits.glob_entries = 1;
    assert!(
        Glob::new()
            .call(json!({"pattern":"**/*"}), &cx)
            .await?
            .text
            .contains("[incomplete:")
    );
    assert!(Glob::new().call(json!({"pattern":"["}), &cx).await.is_err());
    Ok(())
}

#[test]
fn tool_u_015_parallel_flags_match_the_builtin_contract() {
    let tools: Vec<(Box<dyn Tool>, bool)> = vec![
        (Box::new(ReadFile::new()), true),
        (Box::new(Glob::new()), true),
        (Box::new(Grep::new()), true),
        (Box::new(WriteFile::new()), false),
        (Box::new(EditFile::new()), false),
        (Box::new(Shell::new()), false),
    ];
    for (tool, expected) in tools {
        assert_eq!(
            tool.spec().parallel_safe,
            expected,
            "{} parallel safety",
            tool.spec().name.as_str()
        );
    }
}

// TEST: CORE-E-041
/// Of these builtins exactly `shell`, `write_file` and `edit_file` say a success may have changed the workspace
/// (decided on 7 Oct 2026): the repeated-failure count starts again only after one of them,
/// never after a read, a note or a fetch. Of the team tools only `take_files` does, and no channel tool
/// (`team_tools.rs` and the channel tools' own test).
#[test]
fn core_e_041_only_shell_and_the_two_file_writers_change_the_workspace() -> TestResult {
    let conversation: tqa_types::ConversationId = "conversation".parse()?;
    let tools: Vec<Box<dyn Tool>> = vec![
        Box::new(ReadFile::new()),
        Box::new(WriteFile::new()),
        Box::new(EditFile::new()),
        Box::new(Glob::new()),
        Box::new(Grep::new()),
        Box::new(ReadNotes::new(&conversation)),
        Box::new(WriteNotes::new(&conversation)),
        Box::new(Shell::new()),
        Box::new(FetchUrl::new(
            ProxyEndpoint::new("http://gateway:3128")?,
            &"synthetic-proxy-token".to_owned().into(),
        )?),
    ];
    let changing: Vec<&str> = tools
        .iter()
        .filter(|tool| tool.spec().changes_workspace)
        .map(|tool| tool.spec().name.as_str())
        .collect();
    assert_eq!(changing, ["write_file", "edit_file", "shell"]);
    Ok(())
}

#[tokio::test]
async fn tool_u_001_unicode_boundaries_do_not_become_binary() -> TestResult {
    let fixture = workspace_fixture()?;
    let mut cx = support::context(&fixture);
    fs::write(fixture.root().join("unicode"), "ééé")?;
    cx.limits.file_read_bytes = 3;
    assert_eq!(
        ReadFile::new()
            .call(json!({"path":"unicode"}), &cx)
            .await?
            .text,
        "     1\tééé\n"
    );
    for args in [
        json!({"path":"nested"}),
        json!({"path":"hello.txt","limit_bytes":0}),
        json!({"path":false}),
    ] {
        assert!(ReadFile::new().call(args, &cx).await.is_err());
    }
    Ok(())
}

#[tokio::test]
async fn tool_u_005_ignore_file_bounds_and_errors_fail_closed() -> TestResult {
    let fixture = workspace_fixture()?;
    let mut cx = support::context(&fixture);
    cx.limits.ignore_file_bytes = 4;
    fs::write(fixture.root().join(".tqaignore"), "too long")?;
    assert!(matches!(
        Glob::new().call(json!({"pattern":"*"}), &cx).await,
        Err(ToolError::OutputTooLarge { max: 4 })
    ));
    fs::write(fixture.root().join(".tqaignore"), [0xff])?;
    assert!(matches!(
        Glob::new().call(json!({"pattern":"*"}), &cx).await,
        Err(ToolError::InvalidArgs { .. })
    ));
    fs::write(fixture.root().join(".tqaignore"), "{a")?;
    assert!(Glob::new().call(json!({"pattern":"*"}), &cx).await.is_err());
    fs::remove_file(fixture.root().join(".tqaignore"))?;
    fs::create_dir(fixture.root().join(".tqaignore"))?;
    assert!(Glob::new().call(json!({"pattern":"*"}), &cx).await.is_err());
    assert!(
        Glob::new()
            .call(json!({"pattern":"*","max_results":0}), &cx)
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn tool_u_003_empty_file_directory_errors_and_cancellation() -> TestResult {
    let fixture = workspace_fixture()?;
    let mut cx = support::context(&fixture);
    let args = json!({"path":"empty","content":"","create_parents":false});
    assert_eq!(
        WriteFile::new().call(args.clone(), &cx).await?.text,
        "created, 0 bytes\nadded: 0 lines\nremoved: 0 lines"
    );
    assert_eq!(
        WriteFile::new().call(args, &cx).await?.text,
        "replaced, 0 bytes\nadded: 0 lines\nremoved: 0 lines"
    );
    assert!(
        WriteFile::new()
            .call(
                json!({"path":"nested","content":"x","create_parents":false}),
                &cx
            )
            .await
            .is_err()
    );
    cx.limits.file_read_bytes = 0;
    assert!(
        ReadFile::new()
            .call(json!({"path":"hello.txt"}), &cx)
            .await
            .is_err()
    );
    cx.limits.file_read_bytes = 1;
    cx.cancel.cancel();
    assert!(matches!(
        ReadFile::new().call(json!({"path":"hello.txt"}), &cx).await,
        Err(ToolError::Cancelled)
    ));
    Ok(())
}

#[tokio::test]
async fn tool_u_003_unrepresentable_deadlines_reject_before_write() -> TestResult {
    let fixture = workspace_fixture()?;
    let mut cx = support::context(&fixture);
    cx.limits.file_timeout = std::time::Duration::MAX;
    let error = WriteFile::new()
        .call(
            json!({"path":"unwanted","content":"text","create_parents":false}),
            &cx,
        )
        .await;
    assert!(matches!(error,Err(ToolError::InvalidArgs {ref path,..}) if path == "/limits"));
    assert!(!fixture.root().join("unwanted").exists());
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn tool_u_003_replace_preserves_executable_and_private_modes() -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    let path = fixture.root().join("build.sh");
    for mode in [0o755, 0o600] {
        fs::write(&path, "old")?;
        fs::set_permissions(&path, fs::Permissions::from_mode(mode))?;
        WriteFile::new()
            .call(
                json!({"path":"build.sh","content":"new","create_parents":false}),
                &cx,
            )
            .await?;
        assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o7777, mode);
        assert_eq!(fs::read_to_string(&path)?, "new");
    }
    Ok(())
}
