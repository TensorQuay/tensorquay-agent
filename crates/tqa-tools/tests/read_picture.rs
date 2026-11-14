//! TOOL-U-143: `read_file` on a picture says it's a picture, by its bytes and not its name, so the runner can show it to
//! a model that reads pictures (CORE-U-090). The owner's case (13 Nov 2026): the agent converted a photo to JPEG in its
//! workspace and still told the person it couldn't see it.
#[path = "support/context.rs"]
mod support;
use serde_json::json;
use std::{error::Error, fs};
use tqa_testkit::workspace_fixture;
use tqa_tools::{PICTURE_SUMMARY, Tool, builtin::ReadFile};
type TestResult = Result<(), Box<dyn Error>>;

// TEST: TOOL-U-143
#[tokio::test]
async fn tool_u_143_a_picture_reads_as_a_picture_and_other_binaries_as_before() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    let png = [b"\x89PNG\r\n\x1a\n".as_slice(), &[0, 0, 0, 13], b"IHDR"].concat();
    let jpeg = [0xFF_u8, 0xD8, 0xFF, 0xE0, 0, 16, b'J', b'F', b'I', b'F', 0];
    fs::create_dir_all(fixture.root().join("uploads"))?;
    fs::write(fixture.root().join("uploads/IMG_2237.jpg"), jpeg)?;
    fs::write(fixture.root().join("chart.data"), &png)?;
    fs::write(fixture.root().join("blob.bin"), [0_u8, 1, 2, 0, 255, 0])?;
    let read = |path: &str| {
        let tool = ReadFile::new();
        let cx = &cx;
        let path = path.to_owned();
        async move {
            tool.call(json!({ "path": path }), cx)
                .await
                .map(|output| output.text)
        }
    };
    let photo = read("uploads/IMG_2237.jpg").await?;
    assert!(photo.starts_with(PICTURE_SUMMARY), "{photo}");
    assert!(photo.contains("image/jpeg, 11 bytes"), "{photo}");
    // By its bytes: a PNG under another name is still a picture.
    let chart = read("chart.data").await?;
    assert!(
        chart.starts_with(PICTURE_SUMMARY) && chart.contains("image/png"),
        "{chart}"
    );
    // Anything else binary reads as before.
    assert!(
        read("blob.bin")
            .await?
            .starts_with("[binary file, 6 bytes]")
    );
    Ok(())
}
