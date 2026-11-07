//! TOOL-U-001/005: hostile cursors and exact byte-budget boundaries.
#[path = "support/context.rs"]
mod support;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};
use std::{error::Error, fs};
use tqa_testkit::workspace_fixture;
use tqa_tools::{
    SearchStopReason, Tool, ToolError,
    builtin::{Glob, Grep, ReadFile},
};
type TestResult = Result<(), Box<dyn Error>>;

fn cursor(page: &str) -> Result<String, Box<dyn Error>> {
    Ok(page
        .rsplit_once("cursor=")
        .and_then(|(_, token)| token.strip_suffix(']'))
        .ok_or("missing cursor")?
        .into())
}
#[tokio::test]
async fn tool_u_001_known_eof_notice_and_buffer_split_crlf() -> TestResult {
    let fixture = workspace_fixture()?;
    let mut cx = support::context(&fixture);
    cx.limits.read_page_bytes = 512;
    fs::write(
        fixture.root().join("two"),
        format!("{}\n{}\n", "a".repeat(300), "b".repeat(300)),
    )?;
    let page = ReadFile::new().call(json!({"path":"two"}), &cx).await?.text;
    assert!(page.ends_with("[partial: lines 1-1 of 2; continue with start_line=2]"));
    fs::write(
        fixture.root().join("split"),
        format!("{}\r\nlast\r\n", "a".repeat(8191)),
    )?;
    let mut args = json!({"path":"split"});
    let mut rebuilt = String::new();
    loop {
        let page = ReadFile::new().call(args, &cx).await?.text;
        assert!(page.len() <= 512);
        let line = page
            .lines()
            .next()
            .and_then(|line| line.split_once('\t'))
            .ok_or("line")?;
        rebuilt.push_str(line.1);
        if page.contains("cursor=") {
            args = json!({"path":"split","cursor":cursor(&page)?});
        } else {
            assert!(page.contains("start_line=2]"));
            break;
        }
    }
    assert_eq!(rebuilt, "a".repeat(8191));
    assert_eq!(
        ReadFile::new()
            .call(json!({"path":"split","start_line":2}), &cx)
            .await?
            .text,
        "     2\tlast\n"
    );
    Ok(())
}
#[tokio::test]
async fn tool_u_001_forged_cursor_fields_and_changed_oversize_file() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    let path = fixture.root().join("long");
    fs::write(&path, "a".repeat(10000))?;
    let page = ReadFile::new()
        .call(json!({"path":"long"}), &cx)
        .await?
        .text;
    let token = cursor(&page)?;
    let record: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(&token)?)?;
    for (field, value) in [
        ("version", json!(2)),
        ("path", json!("../outside")),
        ("line", json!(u64::MAX)),
        ("offset", json!(u64::MAX)),
    ] {
        let mut changed = record.clone();
        changed[field] = value;
        let token = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&changed)?);
        assert!(matches!(
            ReadFile::new()
                .call(json!({"path":"long","cursor":token}), &cx)
                .await,
            Err(ToolError::InvalidArgs { .. })
        ));
    }
    fs::File::options()
        .write(true)
        .open(&path)?
        .set_len(64 * 1024 * 1024 + 1)?;
    let error = ReadFile::new()
        .call(json!({"path":"long","cursor":token}), &cx)
        .await
        .err()
        .ok_or("stale accepted")?;
    assert!(
        error
            .model_message()
            .contains("file changed since the cursor was issued; read again from start_line")
    );
    Ok(())
}
#[tokio::test]
async fn tool_u_005_partial_line_never_becomes_a_false_regex_end_match() -> TestResult {
    let fixture = workspace_fixture()?;
    let mut cx = support::context(&fixture);
    cx.limits.search_max_bytes = 5;
    fs::write(fixture.root().join("prefix"), "abcde-more")?;
    let output = Grep::new()
        .call(
            json!({"pattern":"^abcde$","path":"prefix","output_mode":"content"}),
            &cx,
        )
        .await?;
    assert_eq!(
        output.search.ok_or("status")?.reason,
        Some(SearchStopReason::Bytes)
    );
    assert!(!output.text.contains("prefix:1:"));
    Ok(())
}
#[tokio::test]
async fn tool_u_005_literal_unclosed_ignore_class_and_byte_sorted_paths() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    fs::create_dir(fixture.root().join("prefix"))?;
    for path in ["prefix/a", "prefix.txt", "["] {
        fs::write(fixture.root().join(path), "hit\n")?;
    }
    fs::write(fixture.root().join(".tqaignore"), "[\n")?;
    let output = Glob::new().call(json!({"pattern":"prefix*"}), &cx).await?;
    assert_eq!(output.text, "prefix.txt\nprefix/a");
    assert!(
        !Grep::new()
            .call(json!({"pattern":"hit"}), &cx)
            .await?
            .text
            .lines()
            .any(|line| line == "[")
    );
    Ok(())
}
#[cfg(unix)]
#[tokio::test]
async fn tool_u_005_ignore_symlinks_cannot_read_outside_workspace() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    std::os::unix::fs::symlink(fixture.outside(), fixture.root().join(".git/info"))?;
    assert!(matches!(
        Glob::new().call(json!({"pattern":"*"}), &cx).await,
        Err(ToolError::PathInvalid(_))
    ));
    Ok(())
}
