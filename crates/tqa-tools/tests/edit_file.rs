//! TOOL-U-004: exact, atomic, mode-preserving edits with bounded diagnostics.
#[path = "support/context.rs"]
mod support;
use proptest::prelude::*;
use serde_json::{Value, json};
use std::{error::Error, fmt::Write as _, fs};
use tqa_testkit::workspace_fixture;
use tqa_tools::{Tool, ToolError, builtin::EditFile};
type TestResult = Result<(), Box<dyn Error>>;

fn request(path: &str, old: &str, new: &str) -> Value {
    json!({"path":path,"edits":[{"old_text":old,"new_text":new}]})
}
fn detail(error: ToolError) -> String {
    match error {
        ToolError::InvalidArgs { detail, .. } => detail,
        other => other.to_string(),
    }
}
#[tokio::test]
async fn tool_u_004_overlapping_matches_are_ambiguous_but_replace_all_does_not_overlap()
-> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    for (source, old, expected) in [
        ("{}}}", "}}", "{x}"),
        ("aaa", "aa", "xa"),
        ("界界界", "界界", "x界"),
    ] {
        fs::write(fixture.root().join("overlap"), source)?;
        let failure = EditFile::new()
            .call(request("overlap", old, "x"), &cx)
            .await;
        assert!(
            matches!(failure, Err(ToolError::InvalidArgs {detail, ..}) if detail.contains("ambiguous"))
        );
        assert_eq!(fs::read_to_string(fixture.root().join("overlap"))?, source);
        EditFile::new().call(json!({"path":"overlap","edits":[{"old_text":old,"new_text":"x","replace_all":true}]}), &cx).await?;
        assert_eq!(
            fs::read_to_string(fixture.root().join("overlap"))?,
            expected
        );
    }
    Ok(())
}
#[tokio::test]
async fn tool_u_004_exact_multiple_edits_are_atomic() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    let tool = EditFile::new();
    let output=tool.call(json!({"path":"hello.txt","edits":[{"old_text":"alpha","new_text":"first"},{"old_text":"beta","new_text":"second"}]}),&cx).await?;
    assert_eq!(
        fs::read_to_string(fixture.root().join("hello.txt"))?,
        "first\nsecond\n"
    );
    assert!(
        output
            .text
            .starts_with("edited hello.txt: 2 edits, +2 -2 lines\n")
    );
    assert!(output.text.contains("--- a/hello.txt\n+++ b/hello.txt\n@@"));
    assert!(output.text.contains("-alpha\n") && output.text.contains("+first\n"));
    let failed=tool.call(json!({"path":"hello.txt","edits":[{"old_text":"first","new_text":"changed"},{"old_text":"missing","new_text":"never"}]}),&cx).await;
    assert!(failed.is_err());
    assert_eq!(
        fs::read_to_string(fixture.root().join("hello.txt"))?,
        "first\nsecond\n"
    );
    Ok(())
}
#[tokio::test]
async fn tool_u_004_missing_whitespace_hint_and_ambiguity() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    fs::write(
        fixture.root().join("text"),
        "heading\n    call();  \nrepeat\nrepeat\n",
    )?;
    let tool = EditFile::new();
    let error = tool
        .call(request("text", "call();\n", "other\n"), &cx)
        .await
        .err()
        .ok_or("expected error")?;
    assert!(
        detail(error).contains("not found; a match differing only in whitespace starts at line 2")
    );
    let error = tool
        .call(request("text", "absent", "other"), &cx)
        .await
        .err()
        .ok_or("expected error")?;
    assert_eq!(detail(error), "not found");
    let error = tool
        .call(request("text", "repeat", "other"), &cx)
        .await
        .err()
        .ok_or("expected error")?;
    assert!(detail(error).contains("3, 4"));
    tool.call(
        json!({"path":"text","edits":[{"old_text":"repeat","new_text":"once","replace_all":true}]}),
        &cx,
    )
    .await?;
    assert_eq!(
        fs::read_to_string(fixture.root().join("text"))?,
        "heading\n    call();  \nonce\nonce\n"
    );
    Ok(())
}
#[tokio::test]
async fn tool_u_004_crlf_and_modes_survive_lf_edits() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    let path = fixture.root().join("script");
    fs::write(&path, "first\r\nsecond\r\n")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
    }
    EditFile::new()
        .call(request("script", "first\nsecond\n", "new\nlines\n"), &cx)
        .await?;
    assert_eq!(fs::read(&path)?, b"new\r\nlines\r\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o755);
    }
    Ok(())
}
#[tokio::test]
async fn tool_u_004_rejects_invalid_protected_large_and_binary() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    let tool = EditFile::new();
    let invalid = vec![
        json!({"path":"hello.txt","edits":[]}),
        json!({"path":"hello.txt","edits":vec![json!({"old_text":"a","new_text":"b"});51]}),
        request("hello.txt", "", "x"),
        request("hello.txt", "alpha", "alpha"),
    ];
    for args in invalid {
        assert!(matches!(
            tool.call(args, &cx).await,
            Err(ToolError::InvalidArgs { .. })
        ));
    }
    // ADR-0035: git internals are part of the workspace, so the edit goes through.
    tool.call(request(".git/config", "fixture", "new"), &cx)
        .await?;
    assert_eq!(
        fs::read_to_string(fixture.root().join(".git/config"))?,
        "new\n"
    );
    for path in ["binary.bin", "nested", "escape/secret.txt"] {
        assert!(tool.call(request(path, "a", "b"), &cx).await.is_err());
    }
    fs::write(fixture.root().join("large"), vec![b'a'; 2 * 1024 * 1024])?;
    assert!(matches!(
        tool.call(request("large", "a", "b"), &cx).await,
        Err(ToolError::OutputTooLarge { .. })
    ));
    assert!(!tool.spec().parallel_safe);
    assert!(!tool.spec().parallel_safe);
    Ok(())
}
#[tokio::test]
async fn tool_u_004_diff_and_ambiguity_are_bounded() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    let old = "old\n".repeat(300);
    fs::write(fixture.root().join("many"), &old)?;
    let error = EditFile::new()
        .call(request("many", "old", "new"), &cx)
        .await
        .err()
        .ok_or("expected error")?;
    let message = detail(error);
    assert!(message.contains("1, 2, 3, 4, 5, 6, 7, 8, 9, 10"));
    assert!(!message.contains(", 11"));
    let output = EditFile::new()
        .call(request("many", &old, &"new\n".repeat(300)), &cx)
        .await?;
    assert!(output.text.lines().count() <= 201);
    assert!(output.text.contains("[diff truncated]"));
    Ok(())
}
proptest! {
    #![proptest_config(ProptestConfig {cases:100,rng_seed:proptest::test_runner::RngSeed::Fixed(9),failure_persistence:None,..ProptestConfig::default()})]
    #[test]
    fn tool_u_004_inverse_unique_edits_restore_bytes(prefix in "[a-zé\n ]{0,64}",old in "[A-Z]{1,16}",new in "[0-9]{1,16}",suffix in "[a-zé\n ]{0,64}") {
        let fixture=workspace_fixture()?;
        let cx=support::context(&fixture);
        let text=format!("{prefix}{old}{suffix}");
        let path=fixture.root().join("roundtrip");
        fs::write(&path,&text)?;
        let runtime=tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        runtime.block_on(async {
            EditFile::new().call(request("roundtrip",&old,&new),&cx).await?;
            EditFile::new().call(request("roundtrip",&new,&old),&cx).await
        })?;
        prop_assert_eq!(fs::read(&path)?,text.as_bytes());
    }
}

#[tokio::test]
async fn tool_u_004_distant_regions_and_shifted_edits() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    let mut text = String::new();
    for number in 1..=100 {
        writeln!(text, "line {number}")?;
    }
    fs::write(fixture.root().join("regions"), &text)?;
    let output = EditFile::new()
        .call(
            json!({"path":"regions","edits":[
                {"old_text":"line 10\n","new_text":"changed ten\ninserted\n"},
                {"old_text":"line 90\n","new_text":"changed ninety\n"},
                {"old_text":"inserted\n","new_text":"final inserted\n"}
            ]}),
            &cx,
        )
        .await?;
    assert!(
        output
            .text
            .starts_with("edited regions: 3 edits, +3 -2 lines\n")
    );
    assert_eq!(
        output
            .text
            .lines()
            .filter(|line| line.starts_with("@@"))
            .count(),
        2
    );
    assert!(output.text.contains("@@ -7,7 +7,8 @@"));
    assert!(output.text.contains("@@ -87,7 +88,7 @@"));
    assert!(!output.text.contains("line 50"));
    Ok(())
}

#[tokio::test]
async fn tool_u_004_intermediate_expansion_is_bounded_and_atomic() -> TestResult {
    let fixture = workspace_fixture()?;
    let mut cx = support::context(&fixture);
    cx.limits.file_read_bytes = 12;
    let result = EditFile::new()
        .call(
            json!({"path":"hello.txt","edits":[
                {"old_text":"alpha","new_text":"123456789012345"},
                {"old_text":"123456789012345","new_text":"a"}
            ]}),
            &cx,
        )
        .await;
    assert!(matches!(result, Err(ToolError::OutputTooLarge { max: 12 })));
    assert_eq!(
        fs::read_to_string(fixture.root().join("hello.txt"))?,
        "alpha\nbeta\n"
    );
    Ok(())
}

#[tokio::test]
async fn tool_u_004_diff_reconstructs_deletions_insertions_and_overlapping_regions() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    for (before, replacements) in [
        ("first\nlast\n", vec![("last\n", "")]),
        ("first\nlast\n", vec![("first\nlast\n", "")]),
        ("first\nlast", vec![("first", "leading\nline")]),
        (
            "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n",
            vec![
                ("h", "changed"),
                ("b", "insert\nnew"),
                ("new\nc\nd", "joined"),
            ],
        ),
        (
            "first\nlast\n",
            vec![("first", "other"), ("other", "first")],
        ),
        ("a\nb\nc\n", vec![("\nb", "joined")]),
        ("hello", vec![("hello", "hello\n")]),
        ("hello", vec![("o", "")]),
        ("hello\n", vec![("hello\n", "hello")]),
    ] {
        fs::write(fixture.root().join("patch"), before)?;
        let edits = replacements
            .iter()
            .map(|(old, new)| json!({"old_text":old,"new_text":new}))
            .collect::<Vec<_>>();
        let output = EditFile::new()
            .call(json!({"path":"patch","edits":edits}), &cx)
            .await?;
        let after = fs::read_to_string(fixture.root().join("patch"))?;
        assert_eq!(apply_diff(before, &output.text)?, after, "{}", output.text);
    }
    Ok(())
}

fn apply_diff(before: &str, output: &str) -> Result<String, Box<dyn Error>> {
    let before: Vec<_> = before.split_inclusive('\n').collect();
    let (mut cursor, mut after, mut previous) = (0, String::new(), ' ');
    for line in output.lines().skip(3) {
        if line.starts_with("@@") {
            let old = line
                .split_whitespace()
                .nth(1)
                .ok_or("missing old range")?
                .trim_start_matches('-');
            let (start, count) = old.split_once(',').ok_or("missing count")?;
            let start: usize = start.parse()?;
            let next = start - usize::from(count != "0");
            for line in &before[cursor..next] {
                after.push_str(line);
            }
            cursor = next;
        } else if line.starts_with('\\') {
            if previous != '-' {
                assert_eq!(after.pop(), Some('\n'));
            }
        } else {
            let prefix = line.chars().next().ok_or("missing prefix")?;
            if prefix != '+' {
                assert_eq!(
                    before
                        .get(cursor)
                        .ok_or("past old end")?
                        .trim_end_matches('\n'),
                    &line[1..]
                );
                cursor += 1;
            }
            if prefix != '-' {
                after.push_str(&line[1..]);
                after.push('\n');
            }
            previous = prefix;
        }
    }
    for line in &before[cursor..] {
        after.push_str(line);
    }
    Ok(after)
}
