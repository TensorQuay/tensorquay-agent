//! PERF-050: run each case in its own release process to compare peak resident memory.
use super::*;

const LARGE_FILES: u64 = 8;
const LINE_BYTES: u64 = 48 * 1024 * 1024;

async fn large_lines(threads: u16) -> Result<(), Box<dyn Error>> {
    let fixture = workspace_fixture()?;
    let chunk = vec![b'x'; 1024 * 1024];
    for index in 0..LARGE_FILES {
        let mut file = fs::File::create(fixture.root().join(format!("large-{index}")))?;
        for _ in 0..LINE_BYTES / chunk.len() as u64 {
            file.write_all(&chunk)?;
        }
    }
    drop(chunk);
    let mut cx = support::context(&fixture);
    cx.limits.search_threads = threads;
    let output = Grep::new().call(json!({"pattern":"zzz"}), &cx).await?;
    assert_eq!(output.text.trim(), "[binary files skipped: 1]");
    let status = output.search.ok_or("missing search status")?;
    assert!(
        status.complete,
        "large-line memory probe must complete: {status:?}"
    );
    assert!(status.scanned_bytes >= LARGE_FILES * LINE_BYTES);
    writeln!(
        std::io::stdout().lock(),
        "PERF-050 large lines: configured_threads={threads}, status={status:?}"
    )?;
    Ok(())
}

#[tokio::test]
#[ignore = "PERF-050 writes 384 MiB; run separately in release mode with a peak RSS recorder"]
async fn perf_050_large_line_memory_one_worker() -> Result<(), Box<dyn Error>> {
    large_lines(1).await
}

#[tokio::test]
#[ignore = "PERF-050 writes 384 MiB; run separately in release mode with a peak RSS recorder"]
async fn perf_050_large_line_memory_eight_workers() -> Result<(), Box<dyn Error>> {
    large_lines(8).await
}
