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

// 18.1 item F: refusals a local model recovers from by itself (the 18.0 Qwen waves' analysis, proposals A, B, D, E).

/// Twenty lines; `if done` on lines 10 and 15 only.
fn twenty_lines() -> Result<String, std::fmt::Error> {
    let mut source = String::new();
    for number in 1..=20 {
        if number == 10 || number == 15 {
            writeln!(source, "    if done {{ return; }} // {number}")?;
        } else {
            writeln!(source, "let line_{number} = {number};")?;
        }
    }
    Ok(source)
}

// A: an ambiguous refusal shows each matching line with its number, then how to match once.
#[tokio::test]
async fn tool_u_004_an_ambiguous_refusal_lists_its_lines() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    fs::write(fixture.root().join("loop.rs"), twenty_lines()?)?;
    let error = EditFile::new()
        .call(request("loop.rs", "if done", "if finished"), &cx)
        .await
        .err()
        .ok_or("expected error")?;
    assert_eq!(
        detail(error),
        "ambiguous; matches at lines 10, 15\n\
         line 10:     if done { return; } // 10\n\
         line 15:     if done { return; } // 15\n\
         Send old_text with enough of its line to match once, or replace_all: true."
    );
    Ok(())
}

// A: past the tenth match the rest are counted, not listed; a CRLF file's lines are shown without the carriage return.
#[tokio::test]
async fn tool_u_004_an_ambiguous_refusal_counts_past_ten_and_drops_carriage_returns() -> TestResult
{
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    fs::write(fixture.root().join("many"), "same\r\n".repeat(12))?;
    let error = EditFile::new()
        .call(request("many", "same", "other"), &cx)
        .await
        .err()
        .ok_or("expected error")?;
    let detail = detail(error);
    let mut expected =
        String::from("ambiguous; matches at lines 1, 2, 3, 4, 5, 6, 7, 8, 9, 10 and 2 more\n");
    for number in 1..=10 {
        writeln!(expected, "line {number}: same")?;
    }
    expected.push_str("Send old_text with enough of its line to match once, or replace_all: true.");
    assert_eq!(detail, expected);
    assert!(!detail.contains('\r'));
    assert_eq!(
        fs::read_to_string(fixture.root().join("many"))?,
        "same\r\n".repeat(12)
    );
    Ok(())
}

const IMPORTS: &str = "import { slugify } from \"./slug\";\nimport { validate } from \"./validate\";\n\nexport const ready = true;\n";

// B: a not-found refusal names the nearest line, its number and its exact text (the quote style shows).
#[tokio::test]
async fn tool_u_004_a_not_found_refusal_names_the_nearest_line() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    fs::write(fixture.root().join("imports.ts"), IMPORTS)?;
    let tool = EditFile::new();
    let error = tool
        .call(
            request("imports.ts", "import { validate } from './validate';", "x"),
            &cx,
        )
        .await
        .err()
        .ok_or("expected error")?;
    assert_eq!(
        detail(error),
        "not found; the nearest line is line 2: import { validate } from \"./validate\";"
    );
    // Lines equally near are all named, in order.
    fs::write(
        fixture.root().join("pair"),
        "let a = 1;\nlet b = 1;\nreturn;\n",
    )?;
    let error = tool
        .call(request("pair", "let c = 1;", "x"), &cx)
        .await
        .err()
        .ok_or("expected error")?;
    assert_eq!(
        detail(error),
        "not found; the nearest lines are:\nline 1: let a = 1;\nline 2: let b = 1;"
    );
    // Only the first line of a multi-line old_text is looked for.
    let error = tool
        .call(
            request(
                "imports.ts",
                "import { validate } from './validate';\nmore\n",
                "x",
            ),
            &cx,
        )
        .await
        .err()
        .ok_or("expected error")?;
    assert!(detail(error).starts_with("not found; the nearest line is line 2: "));
    assert_eq!(
        fs::read_to_string(fixture.root().join("imports.ts"))?,
        IMPORTS
    );
    Ok(())
}

// B: nothing within 40 % of the line's length is no nearest line; a line over 400 bytes is never named.
#[tokio::test]
async fn tool_u_004_a_far_anchor_is_just_not_found() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    fs::write(fixture.root().join("imports.ts"), IMPORTS)?;
    let tool = EditFile::new();
    let error = tool
        .call(
            request("imports.ts", "completely unrelated words", "x"),
            &cx,
        )
        .await
        .err()
        .ok_or("expected error")?;
    assert_eq!(detail(error), "not found");
    fs::write(
        fixture.root().join("long"),
        format!("{}\n", "x".repeat(401)),
    )?;
    let error = tool
        .call(request("long", &format!("{}q", "x".repeat(390)), "x"), &cx)
        .await
        .err()
        .ok_or("expected error")?;
    assert_eq!(detail(error), "not found");
    Ok(())
}

// E: an edit whose old_text and new_text are the same is no change, said plainly; the file isn't touched.
#[tokio::test]
async fn tool_u_004_a_no_op_edit_is_no_change() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    let path = fixture.root().join("readme");
    fs::write(&path, "keep\nother\n")?;
    fs::File::options()
        .write(true)
        .open(&path)?
        .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1000))?;
    let before = fs::metadata(&path)?.modified()?;
    let tool = EditFile::new();
    let output = tool.call(request("readme", "keep", "keep"), &cx).await?;
    assert_eq!(
        output.text,
        "no change: old_text and new_text are the same, so the file was not changed."
    );
    assert_eq!(fs::read_to_string(&path)?, "keep\nother\n");
    assert_eq!(fs::metadata(&path)?.modified()?, before);
    // Beside a real edit, the same-text edit is left out and said so; the real one is made.
    let output = tool
        .call(
            json!({"path":"readme","edits":[{"old_text":"keep","new_text":"keep"},{"old_text":"other","new_text":"changed"}]}),
            &cx,
        )
        .await?;
    assert_eq!(fs::read_to_string(&path)?, "keep\nchanged\n");
    assert!(
        output
            .text
            .starts_with("edited readme: 1 edits, +1 -1 lines\n"),
        "{}",
        output.text
    );
    assert!(
        output
            .text
            .ends_with("1 edit changed nothing: its old_text and new_text are the same.\n"),
        "{}",
        output.text
    );
    // The sentence on a line of its own: one line break before it, never a blank line.
    assert!(
        output.text.contains("\n1 edit changed nothing")
            && !output.text.contains("\n\n1 edit changed nothing"),
        "{}",
        output.text
    );
    // An empty old_text is still refused, and a missing file is still missing.
    let error = tool
        .call(request("readme", "", "x"), &cx)
        .await
        .err()
        .ok_or("expected error")?;
    assert!(detail(error).contains("old text must be nonempty"));
    assert!(
        tool.call(request("absent", "same", "same"), &cx)
            .await
            .is_err()
    );
    Ok(())
}

// D: what the second identical refusal adds, from the tool: the file's region around the lines the refusal named.
#[tokio::test]
async fn tool_u_004_a_repeated_refusal_carries_the_region() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    let mut source = twenty_lines()?;
    for number in 21..=60 {
        writeln!(source, "let line_{number} = {number};")?;
    }
    fs::write(fixture.root().join("loop.rs"), &source)?;
    let lines: Vec<&str> = source.lines().collect();
    let region = |from: usize, to: usize| -> String {
        (from..=to).fold(String::new(), |mut out, number| {
            // Writing to a String cannot fail.
            let _ = writeln!(out, "{number:>6}\t{}", lines[number - 1]);
            out
        })
    };
    let tool = EditFile::new();
    // Ambiguous at 10 and 15: from three lines before the first, at most 40 lines.
    let note = tool
        .repeated_refusal(&request("loop.rs", "if done", "if finished"), &cx)
        .await
        .ok_or("expected a note")?;
    assert_eq!(
        note,
        format!(
            "You sent the same arguments as the call that just failed. Here are lines 7-46 of loop.rs:\n{}\
             Read them and send a different old_text.",
            region(7, 46)
        )
    );
    // Not found, nearest line 30: the region around it.
    let note = tool
        .repeated_refusal(&request("loop.rs", "let line_30 = 31;", "x"), &cx)
        .await
        .ok_or("expected a note")?;
    assert!(
        note.contains("Here are lines 27-60 of loop.rs:\n"),
        "{note}"
    );
    assert!(note.contains(&region(27, 60)));
    // An anchor near no line still gets the nearest region, whatever its distance.
    let note = tool
        .repeated_refusal(&request("loop.rs", "zzzz zzzz zzzz zzzz", "x"), &cx)
        .await
        .ok_or("expected a note")?;
    assert!(
        note.starts_with(
            "You sent the same arguments as the call that just failed. Here are lines "
        )
    );
    // A later edit's failure is found after the earlier edits are made in memory; the file stays as it was.
    let note = tool
        .repeated_refusal(
            &json!({"path":"loop.rs","edits":[{"old_text":"let line_1 = 1;","new_text":"let first = 1;"},{"old_text":"if done","new_text":"x"}]}),
            &cx,
        )
        .await
        .ok_or("expected a note")?;
    assert!(note.contains("Here are lines 7-46 of loop.rs:\n"), "{note}");
    assert_eq!(fs::read_to_string(fixture.root().join("loop.rs"))?, source);
    // Nothing to add: a call that would succeed, a missing file, arguments that aren't an edit.
    for args in [
        request("loop.rs", "let line_1 = 1;", "let first = 1;"),
        request("absent", "x", "y"),
        json!({"path": 3}),
    ] {
        assert_eq!(tool.repeated_refusal(&args, &cx).await, None, "{args}");
    }
    Ok(())
}

// The registry asks the tool for the region with the call's arguments as journaled; an unknown tool or arguments its
// schema refuses add nothing.
#[tokio::test]
async fn tool_u_004_the_registry_asks_the_tool_for_its_region() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    fs::write(fixture.root().join("loop.rs"), twenty_lines()?)?;
    let mut registry = tqa_tools::ToolRegistry::new();
    registry.register(std::sync::Arc::new(EditFile::new()))?;
    let args = request("loop.rs", "if done", "if finished");
    let note = registry
        .repeated_refusal("edit_file", &args, &cx)
        .await
        .ok_or("expected a note")?;
    assert!(
        note.starts_with(
            "You sent the same arguments as the call that just failed. Here are lines 7-20 of loop.rs:\n"
        ),
        "{note}"
    );
    assert_eq!(registry.repeated_refusal("deploy", &args, &cx).await, None);
    assert_eq!(
        registry
            .repeated_refusal("edit_file", &json!({"path": 7}), &cx)
            .await,
        None
    );
    Ok(())
}

// Past a thousand matches the count stops, and the refusal says so ("or more").
#[tokio::test]
async fn tool_u_004_an_ambiguous_refusal_stops_counting_at_a_thousand() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    fs::write(fixture.root().join("many"), "same\n".repeat(1500))?;
    let detail = detail(
        EditFile::new()
            .call(request("many", "same", "other"), &cx)
            .await
            .err()
            .ok_or("expected error")?,
    );
    assert!(
        detail.starts_with(
            "ambiguous; matches at lines 1, 2, 3, 4, 5, 6, 7, 8, 9, 10 and 990 or more\n"
        ),
        "{detail}"
    );
    Ok(())
}

/// A made-up key body, generated here so the source holds nothing key-shaped for the secrets scan (G-10).
fn made_up_key() -> String {
    const BASE62: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut state: u64 = 7;
    let body: String = (0..40)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            char::from(BASE62[usize::try_from(state % 62).unwrap_or(0)])
        })
        .collect();
    format!("sk-{body}")
}

// TEST: TOOL-U-151
#[tokio::test]
async fn tool_u_151_no_file_tool_saves_a_key_into_a_memory_note() -> TestResult {
    use tqa_tools::builtin::WriteFile;
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    let key = made_up_key();
    // write_file: refused with the reason, and nothing is left behind (no note, no folder's staging file).
    let refused = WriteFile::new()
        .call(
            json!({"path": ".memory/deploy.md", "create_parents": true,
                   "content": format!("about: the deploy key\n\nThe key is {key}\n")}),
            &cx,
        )
        .await;
    let said = refused.map_or_else(detail, |_| "written".to_owned());
    assert!(said.contains("can't hold a key"), "{said}");
    assert!(!fixture.root().join(".memory/deploy.md").exists());
    let left: Vec<_> = fs::read_dir(fixture.root().join(".memory"))
        .map(|entries| entries.filter_map(Result::ok).collect())
        .unwrap_or_default();
    assert!(left.is_empty(), "{left:?}");
    // An ordinary note is saved; an edit that would add the key is refused and the note stays as it was.
    let note = "about: Deploys go through staging\nkind: convention\n\nRun make stage first.\n";
    WriteFile::new()
        .call(
            json!({"path": ".memory/deploy.md", "create_parents": true, "content": note}),
            &cx,
        )
        .await?;
    let edited = EditFile::new()
        .call(
            request(
                ".memory/deploy.md",
                "Run make stage first.",
                &format!("Use {key}."),
            ),
            &cx,
        )
        .await;
    assert!(
        edited
            .map_or_else(detail, |_| "edited".to_owned())
            .contains("can't hold a key")
    );
    assert_eq!(
        fs::read_to_string(fixture.root().join(".memory/deploy.md"))?,
        note
    );
    // The same content outside the memory folder is the workspace's own business, as before.
    WriteFile::new()
        .call(
            json!({"path": "config.env", "content": format!("TOKEN={key}\n")}),
            &cx,
        )
        .await?;
    assert!(fixture.root().join("config.env").is_file());
    Ok(())
}
