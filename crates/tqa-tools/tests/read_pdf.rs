//! TOOL-U-145 (PRD item 30): `read_file` on a PDF answers its text by page, under the tool's page bound, with a cursor
//! to the next page; the line arguments are page numbers; an empty page says so; what isn't a PDF reads as before.
#[path = "support/context.rs"]
mod support;
use serde_json::json;
use std::{error::Error, fs};
use tqa_testkit::{pdf::pdf, workspace_fixture};
use tqa_tools::{PAGE_MARK, PDF_SUMMARY, Tool, builtin::ReadFile};
type TestResult = Result<(), Box<dyn Error>>;

// TEST: TOOL-U-145
#[tokio::test]
async fn tool_u_145_a_pdf_reads_by_page_with_a_cursor_to_the_next() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    fs::write(
        fixture.root().join("report.pdf"),
        pdf(&["Page one words", "", "Page three words"]),
    )?;
    let tool = ReadFile::new();
    let whole = tool.call(json!({ "path": "report.pdf" }), &cx).await?.text;
    assert!(
        whole.starts_with(&format!("{PDF_SUMMARY}3 pages]\n")),
        "{whole}"
    );
    assert!(
        whole.contains(&format!("{PAGE_MARK}1 ---\nPage one words\n")),
        "{whole}"
    );
    assert!(
        whole.contains(&format!("{PAGE_MARK}2 ---\n[no text on this page]\n")),
        "{whole}"
    );
    assert!(
        whole.contains(&format!("{PAGE_MARK}3 ---\nPage three words\n")),
        "{whole}"
    );
    assert!(whole.ends_with("[end of document: 3 pages]"), "{whole}");
    // The line arguments are page numbers.
    let third = tool
        .call(
            json!({ "path": "report.pdf", "start_line": 3, "end_line": 3 }),
            &cx,
        )
        .await?
        .text;
    assert!(third.contains("page 3 ---\nPage three words"), "{third}");
    assert!(!third.contains("Page one words"), "{third}");
    let past = tool
        .call(json!({ "path": "report.pdf", "start_line": 9 }), &cx)
        .await?
        .text;
    assert!(
        past.contains("[end of document: page 9 is past the last page]"),
        "{past}"
    );
    Ok(())
}

#[tokio::test]
async fn tool_u_145_long_pages_stop_at_the_bound_and_the_cursor_continues() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    // Three pages of 3,500 bytes each: two fit an 8 KiB read with its summary and notice, the third waits for the
    // cursor.
    let page = "word ".repeat(700);
    fs::write(fixture.root().join("long.pdf"), pdf(&[&page, &page, &page]))?;
    let tool = ReadFile::new();
    let first = tool.call(json!({ "path": "long.pdf" }), &cx).await?.text;
    assert!(
        first.len() <= cx.limits.read_page_bytes,
        "{} bytes",
        first.len()
    );
    assert!(
        first.contains("page 1 ---") && !first.contains("page 3 ---"),
        "{first}"
    );
    let notice = first.lines().last().ok_or("no notice")?;
    assert!(notice.starts_with("[partial: pages 1-"), "{notice}");
    let token = notice
        .split("cursor ")
        .nth(1)
        .and_then(|rest| rest.split(' ').next())
        .ok_or("no cursor")?;
    let next = tool
        .call(json!({ "path": "long.pdf", "cursor": token }), &cx)
        .await?
        .text;
    assert!(next.contains("page 3 ---"), "{next}");
    assert!(next.ends_with("[end of document: 3 pages]"), "{next}");
    // A guessed cursor beside page numbers is ignored, as for text (TOOL-U-051).
    let guessed = tool
        .call(
            json!({ "path": "long.pdf", "start_line": 3, "cursor": "1" }),
            &cx,
        )
        .await?
        .text;
    assert!(guessed.contains("page 3 ---"), "{guessed}");
    Ok(())
}

#[tokio::test]
async fn tool_u_145_other_files_read_as_before() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    fs::write(fixture.root().join("notes.txt"), "plain words\n")?;
    fs::write(fixture.root().join("blob.bin"), [0_u8, 1, 2, 0, 255, 0])?;
    // A PDF header on a damaged file: named as a PDF nothing could be read from, never an error.
    fs::write(fixture.root().join("broken.pdf"), b"%PDF-1.7\n")?;
    let tool = ReadFile::new();
    let text = tool.call(json!({ "path": "notes.txt" }), &cx).await?.text;
    assert!(text.contains("plain words"), "{text}");
    let blob = tool.call(json!({ "path": "blob.bin" }), &cx).await?.text;
    assert!(blob.starts_with("[binary file, 6 bytes]"), "{blob}");
    let broken = tool.call(json!({ "path": "broken.pdf" }), &cx).await?.text;
    assert!(
        broken.starts_with(&format!("{PDF_SUMMARY}9 bytes: damaged")),
        "{broken}"
    );
    Ok(())
}
