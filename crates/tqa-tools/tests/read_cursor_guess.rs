//! TOOL-U-051: a model's guessed cursor next to a line range reads the lines, and every paging tool's cursor says it's
//! opaque. A model sent `read_file` `cursor: "1"` with `start_line` 1 and `end_line` 200 three times; each was refused
//! and the run stopped on the repeated failure (the c14b-rc demo, 7 Oct 2026).
#[path = "support/context.rs"]
mod support;
use serde_json::json;
use std::{error::Error, fmt::Write as _, fs, sync::Arc};
use tqa_testkit::workspace_fixture;
use tqa_tools::{
    Tool, ToolLimits, ToolRegistry,
    builtin::{Glob, Grep, ReadFile},
};
type TestResult = Result<(), Box<dyn Error>>;

const IGNORED: &str =
    "[cursor ignored: it wasn't one an earlier read_file gave; these are the lines asked for]";

// TEST: TOOL-U-051
#[tokio::test]
async fn tool_u_051_a_guessed_cursor_with_a_line_range_reads_the_lines() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    let mut source = String::new();
    for line in 1..=300 {
        writeln!(source, "entry {line:03} amount {}", line * 7)?;
    }
    fs::write(fixture.root().join("ledger.txt"), &source)?;
    let tool = ReadFile::new();
    for (guess, range) in [
        ("1", json!({"start_line": 1, "end_line": 200})),
        ("200", json!({"start_line": 201})),
        ("page-2", json!({"end_line": 10})),
    ] {
        let mut args = range.clone();
        args["path"] = json!("ledger.txt");
        let without = tool.call(args.clone(), &cx).await?.text;
        args["cursor"] = json!(guess);
        let with = tool.call(args, &cx).await?.text;
        // A whole page has no notice: the note is its last line.
        assert_eq!(with, format!("{without}{IGNORED}"), "{guess} with {range}");
    }
    Ok(())
}

// TEST: TOOL-U-051
/// Alone, a guessed cursor is still refused, word for word as before (the harness classifies runs by this text).
#[tokio::test]
async fn tool_u_051_a_guessed_cursor_alone_is_still_refused() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    let error = ReadFile::new()
        .call(json!({"path": "hello.txt", "cursor": "1"}), &cx)
        .await
        .err()
        .ok_or("a guessed cursor alone was accepted")?;
    assert_eq!(
        error.model_message(),
        "invalid_args: /cursor: not a cursor from an earlier read_file; to read by lines, leave cursor out"
    );
    Ok(())
}

// TEST: TOOL-U-051
/// A cursor the tool issued still continues, with no notice; with a line range it's still refused (TOOL-U-001).
#[tokio::test]
async fn tool_u_051_an_issued_cursor_still_continues() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    fs::write(fixture.root().join("long"), "a".repeat(10000))?;
    let tool = ReadFile::new();
    let page = tool.call(json!({"path": "long"}), &cx).await?.text;
    let cursor = page
        .lines()
        .last()
        .and_then(|notice| notice.split_once("continue with cursor="))
        .and_then(|(_, cursor)| cursor.strip_suffix(']'))
        .ok_or("no cursor")?
        .to_owned();
    let next = tool
        .call(json!({"path": "long", "cursor": cursor}), &cx)
        .await?
        .text;
    assert!(!next.contains("[cursor ignored"));
    assert!(next.contains('a'));
    let error = tool
        .call(
            json!({"path": "long", "cursor": cursor, "start_line": 1}),
            &cx,
        )
        .await
        .err()
        .ok_or("an issued cursor with a line accepted")?;
    assert!(
        error
            .model_message()
            .contains("cursor excludes start_line and end_line")
    );
    Ok(())
}

// TEST: TOOL-U-051
/// `read_file`, `grep` and `glob` each describe their cursor to the model, and the schemas still register and take a null
/// cursor (strict schemas: every optional property is required and nullable).
#[test]
fn tool_u_051_every_paging_cursor_is_described() -> TestResult {
    let tools: [Arc<dyn Tool>; 3] = [
        Arc::new(ReadFile::new()),
        Arc::new(Grep::new()),
        Arc::new(Glob::new()),
    ];
    let mut registry = ToolRegistry::new();
    for tool in tools {
        let cursor = &tool.spec().parameters["properties"]["cursor"];
        assert_eq!(cursor["type"], json!(["string", "null"]));
        let description = cursor["description"].as_str().ok_or("no description")?;
        assert!(description.contains("Never a line number"), "{description}");
        registry.register(tool)?;
    }
    assert!(
        registry
            .prepare(
                "read_file",
                &json!({"path": "a", "start_line": null, "end_line": null, "cursor": null})
            )
            .is_ok()
    );
    Ok(())
}

// TEST: TOOL-U-051
/// On a partial page the note goes before the notice, so the notice stays the last line a model follows, and the page
/// keeps its byte bound, note included.
#[tokio::test]
async fn tool_u_051_the_note_precedes_the_notice_within_the_page_bound() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    let mut source = String::new();
    for line in 1..=400 {
        writeln!(source, "entry {line:03} {}", "x".repeat(90))?;
    }
    fs::write(fixture.root().join("wide.txt"), &source)?;
    let tool = ReadFile::new();
    let without = tool
        .call(json!({"path": "wide.txt", "start_line": 1}), &cx)
        .await?
        .text;
    let with = tool
        .call(
            json!({"path": "wide.txt", "start_line": 1, "cursor": "1"}),
            &cx,
        )
        .await?
        .text;
    let lines: Vec<&str> = with.lines().collect();
    let notice = lines.last().ok_or("empty")?;
    assert!(notice.starts_with("[partial: lines 1-"), "{notice}");
    assert_eq!(lines[lines.len() - 2], IGNORED);
    assert!(
        with.len() <= ToolLimits::RECOMMENDED.read_page_bytes,
        "{} bytes",
        with.len()
    );
    // The same notice as without the guess, or one line earlier when the note took a line's room.
    let continued = |text: &str| {
        text.lines()
            .last()
            .and_then(|notice| notice.rsplit_once("start_line="))
            .and_then(|(_, next)| next.trim_end_matches(']').parse::<u64>().ok())
    };
    let (alone, guessed) = (
        continued(&without).ok_or("no notice")?,
        continued(&with).ok_or("no notice")?,
    );
    assert!(
        guessed == alone || guessed + 1 == alone,
        "{guessed} vs {alone}"
    );
    Ok(())
}

// TEST: TOOL-U-051
/// A binary file gets its summary and that the cursor was ignored, never "the lines asked for".
#[tokio::test]
async fn tool_u_051_a_binary_file_never_reads_as_lines() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    fs::write(fixture.root().join("blob.bin"), [0_u8, 1, 2, 0, 255, 0])?;
    let text = ReadFile::new()
        .call(
            json!({"path": "blob.bin", "start_line": 1, "cursor": "1"}),
            &cx,
        )
        .await?
        .text;
    assert!(text.starts_with("[binary file, 6 bytes]"), "{text}");
    assert!(text.contains("cursor ignored"), "{text}");
    assert!(!text.contains("lines asked for"), "{text}");
    Ok(())
}
