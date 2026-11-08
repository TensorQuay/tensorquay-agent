//! TOOL-U-001: bounded line pages and stateless long-line continuation.
#[path = "support/context.rs"]
mod support;
use proptest::prelude::*;
use serde_json::{Value, json};
use std::{
    error::Error,
    fmt::Write as _,
    fs,
    io::Write,
    time::{Duration, UNIX_EPOCH},
};
use tqa_testkit::workspace_fixture;
use tqa_tools::{Tool, ToolError, builtin::ReadFile};
type TestResult = Result<(), Box<dyn Error>>;

fn next(text: &str, path: &str) -> Option<Value> {
    let notice = text.lines().last()?;
    if let Some((_, cursor)) = notice.split_once("continue with cursor=") {
        return Some(json!({"path":path,"cursor":cursor.strip_suffix(']')?}));
    }
    let (_, line) = notice.split_once("continue with start_line=")?;
    Some(json!({"path":path,"start_line":line.strip_suffix(']')?.parse::<u64>().ok()?}))
}
fn lines(text: &str) -> Vec<(u64, &str)> {
    text.lines()
        .filter_map(|line| {
            let (number, content) = line.split_once('\t')?;
            Some((number.trim().parse().ok()?, content))
        })
        .collect()
}

#[tokio::test]
async fn tool_u_001_default_ranges_and_literal_continuation() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    let mut source = String::new();
    for line in 1..=5000 {
        writeln!(source, "source {line}")?;
    }
    fs::write(fixture.root().join("many"), &source)?;
    let tool = ReadFile::new();
    let first = tool.call(json!({"path":"many"}), &cx).await?.text;
    assert_eq!(lines(&first).len(), 200);
    assert!(first.starts_with("     1\tsource 1\n"));
    assert!(first.ends_with("[partial: lines 1-200 of unknown; continue with start_line=201]"));
    let mut request = Some(json!({"path":"many"}));
    let mut collected = Vec::new();
    while let Some(args) = request {
        let page = tool.call(args, &cx).await?.text;
        assert!(page.len() <= 8192);
        assert!(
            page.lines()
                .filter(|line| line.starts_with("[partial:"))
                .count()
                <= 1
        );
        collected.extend(
            lines(&page)
                .into_iter()
                .map(|(n, text)| (n, text.to_owned())),
        );
        request = next(&page, "many");
    }
    assert_eq!(
        collected,
        (1..=5000)
            .map(|n| (n, format!("source {n}")))
            .collect::<Vec<_>>()
    );
    Ok(())
}

#[tokio::test]
async fn tool_u_001_line_ranges_are_trimmed_and_beyond_eof_is_explicit() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    let tool = ReadFile::new();
    let mut source = String::new();
    for line in 1..=5000 {
        writeln!(source, "source {line}")?;
    }
    fs::write(fixture.root().join("many"), source)?;
    assert_eq!(
        tool.call(json!({"path":"many","start_line":4999}), &cx)
            .await?
            .text,
        "  4999\tsource 4999\n  5000\tsource 5000\n"
    );
    assert_eq!(
        tool.call(json!({"path":"many","start_line":2,"end_line":3}), &cx)
            .await?
            .text,
        "     2\tsource 2\n     3\tsource 3\n"
    );
    let range = tool
        .call(json!({"path":"many","start_line":2,"end_line":4000}), &cx)
        .await?
        .text;
    assert_eq!(lines(&range).len(), 200);
    assert!(range.contains("continue with start_line=202]"));
    assert_eq!(
        tool.call(json!({"path":"many","start_line":6000}), &cx)
            .await?
            .text,
        "[end of file: 5000 lines; start_line 6000 is past the end]"
    );
    Ok(())
}

#[tokio::test]
async fn tool_u_001_page_byte_budget_is_hard_and_edit_cap_is_independent() -> TestResult {
    let fixture = workspace_fixture()?;
    let mut cx = support::context(&fixture);
    cx.limits.file_read_bytes = 1;
    fs::write(
        fixture.root().join("wide"),
        format!("{}\n", "x".repeat(600)).repeat(500),
    )?;
    let page = ReadFile::new()
        .call(json!({"path":"wide"}), &cx)
        .await?
        .text;
    assert!(page.len() <= 8192, "page was {} bytes", page.len());
    assert!(lines(&page).len() > 1 && lines(&page).len() < 200);
    cx.limits.read_page_bytes = 512;
    cx.limits.read_page_lines = 2;
    let page = ReadFile::new()
        .call(json!({"path":"hello.txt"}), &cx)
        .await?
        .text;
    assert_eq!(page, "     1\talpha\n     2\tbeta\n");
    Ok(())
}

#[tokio::test]
async fn tool_u_001_three_mib_single_unicode_line_reassembles_exactly() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    let source = format!("{}xyz", "é界".repeat(3 * 1024 * 1024 / 5));
    fs::write(fixture.root().join("minified"), &source)?;
    let mut request = Some(json!({"path":"minified"}));
    let mut collected = String::new();
    let mut pages = 0;
    while let Some(args) = request {
        let page = ReadFile::new().call(args, &cx).await?.text;
        assert!(page.len() <= 8192);
        let data = lines(&page);
        assert_eq!(data.len(), 1);
        assert_eq!(data[0].0, 1);
        collected.push_str(data[0].1);
        request = next(&page, "minified");
        if request.is_some() {
            assert!(page.contains("[partial: line 1 bytes "));
        }
        pages += 1;
        assert!(pages < 1000, "cursor did not advance");
    }
    assert!(pages > 300);
    assert_eq!(collected, source);
    Ok(())
}

#[tokio::test]
async fn tool_u_001_stale_cursors_reject_size_and_mtime_changes() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    let path = fixture.root().join("long");
    for replacement in ["b".repeat(10000), "changed".to_owned()] {
        fs::write(&path, "a".repeat(10000))?;
        fs::File::options()
            .write(true)
            .open(&path)?
            .set_modified(UNIX_EPOCH + Duration::from_secs(1000))?;
        let page = ReadFile::new()
            .call(json!({"path":"long"}), &cx)
            .await?
            .text;
        let cursor = next(&page, "long").ok_or("missing cursor")?;
        fs::write(&path, &replacement)?;
        fs::File::options()
            .write(true)
            .open(&path)?
            .set_modified(UNIX_EPOCH + Duration::from_secs(2000))?;
        let error = ReadFile::new()
            .call(cursor, &cx)
            .await
            .err()
            .ok_or("stale cursor accepted")?;
        assert!(matches!(error, ToolError::InvalidArgs { .. }));
        assert!(
            error
                .model_message()
                .contains("file changed since the cursor was issued; read again from start_line")
        );
    }
    Ok(())
}

/// An empty cursor is no cursor: small models fill an optional string with `""`, and a Gate 5 demo before 17.0 ended on
/// three refusals of exactly this call (2 Oct 2026). Its line range is read as if the cursor were left out.
#[tokio::test]
async fn tool_u_001_an_empty_cursor_counts_as_absent() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    for cursor in [json!(""), json!("  ")] {
        let with = ReadFile::new()
            .call(
                json!({"cursor": cursor, "path": "hello.txt", "start_line": 1, "end_line": 1}),
                &cx,
            )
            .await?
            .text;
        let without = ReadFile::new()
            .call(
                json!({"path": "hello.txt", "start_line": 1, "end_line": 1}),
                &cx,
            )
            .await?
            .text;
        assert_eq!(with, without, "{cursor}");
    }
    Ok(())
}

/// A refused cursor says what to drop. A cursor of `"None"` with a line range (a real model's call, verbatim) was refused
/// here until 2 Oct 2026; the lead's decision that day makes a null-word in a cursor absent, so it now reads as the same
/// range without a cursor (TOOL-U-034), and only a cursor that isn't this tool's is refused.
#[tokio::test]
async fn tool_u_001_a_refused_cursor_says_what_to_drop() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    let not_ours = "invalid_args: /cursor: not a cursor from an earlier read_file; to read by lines, leave cursor out";
    for args in [
        json!({"path": "hello.txt", "cursor": "Nonesuch"}),
        json!({"path": "hello.txt", "cursor": "e30"}),
    ] {
        let error = ReadFile::new()
            .call(args.clone(), &cx)
            .await
            .err()
            .ok_or_else(|| format!("accepted: {args}"))?;
        assert_eq!(error.model_message(), not_ours, "{args}");
    }
    // A cursor this tool issued, with a line argument: the refusal names both ways out.
    fs::write(fixture.root().join("long"), "a".repeat(10000))?;
    let page = ReadFile::new()
        .call(json!({"path":"long"}), &cx)
        .await?
        .text;
    let mut both = next(&page, "long").ok_or("missing cursor")?;
    both["start_line"] = json!(1);
    let error = ReadFile::new()
        .call(both, &cx)
        .await
        .err()
        .ok_or("a cursor with a line accepted")?;
    assert_eq!(
        error.model_message(),
        "invalid_args: /cursor: cursor excludes start_line and end_line; to continue from the cursor, leave both \
         out; to read by lines, leave cursor out"
    );
    Ok(())
}

#[tokio::test]
async fn tool_u_001_sixty_mib_crlf_and_oversized_file() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    let path = fixture.root().join("large");
    let mut file = fs::File::create(&path)?;
    let block = "large\n".repeat(1024 * 1024 / 2);
    for _ in 0..20 {
        file.write_all(block.as_bytes())?;
    }
    assert_eq!(file.metadata()?.len(), 60 * 1024 * 1024);
    let page = ReadFile::new()
        .call(json!({"path":"large","start_line":10_485_759}), &cx)
        .await?
        .text;
    assert_eq!(page, "10485759\tlarge\n10485760\tlarge\n");
    file.set_len(64 * 1024 * 1024 + 1)?;
    let error = ReadFile::new()
        .call(json!({"path":"large"}), &cx)
        .await
        .err()
        .ok_or("oversize accepted")?;
    assert!(error.model_message().contains("67108865"));
    fs::write(fixture.root().join("crlf"), "first\r\n\r\n界\r\nlast")?;
    assert_eq!(
        ReadFile::new()
            .call(json!({"path":"crlf","start_line":2}), &cx)
            .await?
            .text,
        "     2\t\n     3\t界\n     4\tlast\n"
    );
    Ok(())
}

#[tokio::test]
async fn tool_u_001_invalid_ranges_cursors_and_limits() -> TestResult {
    let fixture = workspace_fixture()?;
    let mut cx = support::context(&fixture);
    for args in [
        json!({"path":"hello.txt","start_line":0}),
        json!({"path":"hello.txt","start_line":2,"end_line":1}),
        json!({"path":"hello.txt","cursor":"invalid!"}),
        json!({"path":"hello.txt","offset":1}),
    ] {
        assert!(matches!(
            ReadFile::new().call(args, &cx).await,
            Err(ToolError::InvalidArgs { .. })
        ));
    }
    // A cursor the tool never issued, next to a line argument, is a guess: the lines are read (TOOL-U-051).
    for args in [
        json!({"path":"hello.txt","cursor":"e30","start_line":1}),
        json!({"path":"hello.txt","cursor":"e30","end_line":1}),
    ] {
        let text = ReadFile::new().call(args.clone(), &cx).await?.text;
        assert!(text.ends_with("these are the lines asked for]"), "{args}");
    }
    for invalid in [0, 511, 8193] {
        cx.limits.read_page_bytes = invalid;
        assert!(cx.limits.validate().is_err());
        cx.limits.read_page_bytes = 8192;
        cx.limits.search_page_bytes = invalid;
        assert!(cx.limits.validate().is_err());
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig {cases:100,rng_seed:proptest::test_runner::RngSeed::Fixed(13),failure_persistence:None,..ProptestConfig::default()})]
    #[test]
    fn tool_u_001_random_line_pages_are_complete_and_ordered(contents in prop::collection::vec("[a-zé界]{0,300}", 1..80), final_newline in any::<bool>(), crlf in any::<bool>()) {
        let fixture = workspace_fixture()?;
        let mut cx = support::context(&fixture);
        cx.limits.read_page_lines = 7;
        let terminator = if crlf { "\r\n" } else { "\n" };
        let mut source = contents.join(terminator);
        if final_newline { source.push_str(terminator); }
        fs::write(fixture.root().join("random"), &source)?;
        let expected: Vec<_> = source.lines().enumerate().map(|(i,line)| (i as u64 + 1, line.to_owned())).collect();
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        let actual = runtime.block_on(async {
            let mut request = Some(json!({"path":"random"}));
            let mut actual = Vec::new();
            while let Some(args) = request {
                let page = ReadFile::new().call(args, &cx).await?.text;
                assert!(page.len() <= 8192);
                actual.extend(lines(&page).into_iter().map(|(n,text)| (n,text.to_owned())));
                request = next(&page, "random");
                assert!(actual.len() <= contents.len());
            }
            Ok::<_, ToolError>(actual)
        })?;
        prop_assert_eq!(actual, expected);
    }
}

#[tokio::test]
async fn tool_u_001_eof_counts_terminated_and_unterminated_lines() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    for (source, total) in [
        ("", 0),
        ("one", 1),
        ("one\n", 1),
        ("one\r\ntwo", 2),
        ("\n\n", 2),
    ] {
        fs::write(fixture.root().join("end"), source)?;
        for start in [total + 1, total + 10] {
            let output = ReadFile::new()
                .call(json!({"path":"end","start_line":start}), &cx)
                .await?;
            let expected = eof_notice(total, start);
            assert_eq!(output.text, expected, "source {source:?}");
        }
    }
    Ok(())
}

fn eof_notice(total: u64, start: u64) -> String {
    if total == 0 && start == 1 {
        "[end of file: 0 lines]".into()
    } else {
        format!("[end of file: {total} lines; start_line {start} is past the end]")
    }
}

/// A null-word cursor is no cursor (the lead's decision, 2 Oct 2026): the call reads as without it.
#[tokio::test]
async fn tool_u_001_a_null_word_cursor_counts_as_absent() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    for word in ["None", "null", "UNDEFINED"] {
        let with = ReadFile::new()
            .call(
                json!({"cursor": word, "path": "hello.txt", "start_line": 1, "end_line": 1}),
                &cx,
            )
            .await?
            .text;
        let without = ReadFile::new()
            .call(
                json!({"path": "hello.txt", "start_line": 1, "end_line": 1}),
                &cx,
            )
            .await?
            .text;
        assert_eq!(with, without, "{word}");
    }
    Ok(())
}
