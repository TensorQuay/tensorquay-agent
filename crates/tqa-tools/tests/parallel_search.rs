//! TOOL-U-005/PERF-050: ordered workers preserve pages, cursors and budget prefixes.
#[path = "support/context.rs"]
mod support;
use proptest::prelude::*;
use serde_json::{Value, json};
use std::{error::Error, fs, sync::Arc, time::Duration};
use tqa_testkit::{FakeClock, FakeSleeper, WorkspaceFixture, workspace_fixture};
use tqa_tools::{
    SearchStopReason, Tool, ToolContext, ToolError, ToolLimits, ToolOutput, builtin::Grep,
};

type TestResult = Result<(), Box<dyn Error>>;

fn tree(files: &[u8]) -> Result<WorkspaceFixture, ToolError> {
    let fixture = workspace_fixture()?;
    fs::create_dir(fixture.root().join("tree"))?;
    for (index, kind) in files.iter().enumerate() {
        let directory = fixture.root().join(format!("tree/d{:02}", index % 13));
        fs::create_dir_all(&directory)?;
        let bytes = match kind % 6 {
            0 => String::new(),
            1 => "miss\nordinary\n".into(),
            2 => format!("hit {index}\nmiss\nhit\n"),
            3 => format!("{}\0", "hit\n".repeat(30_000)),
            4 => "miss\nhit".into(),
            _ => format!("hit{}\n", "long".repeat(3000)),
        };
        fs::write(directory.join(format!("f{index:03}.txt")), bytes)?;
    }
    Ok(fixture)
}

fn context(fixture: &WorkspaceFixture) -> ToolContext {
    let mut cx = support::context(fixture);
    let clock = FakeClock::new();
    cx.clock = Arc::new(clock.clone());
    cx.sleeper = Arc::new(FakeSleeper::new(clock));
    cx
}

fn rows(output: &ToolOutput) -> Vec<&str> {
    output
        .text
        .lines()
        .filter(|row| !row.starts_with('['))
        .collect()
}

async fn compare_pages(args: Value, mut cx: ToolContext, depth: usize) -> TestResult {
    let mut request = args;
    for _ in 0..=depth {
        cx.limits.search_threads = 1;
        let baseline = Grep::new().call(request.clone(), &cx).await?;
        assert!(baseline.text.len() <= cx.limits.search_page_bytes);
        for threads in [2, 8] {
            cx.limits.search_threads = threads;
            let parallel = Grep::new().call(request.clone(), &cx).await?;
            assert_eq!(parallel, baseline, "worker count {threads}");
        }
        let status = baseline.search.ok_or("missing search status")?;
        assert_eq!(status.reason, None);
        let Some(cursor) = status.next_cursor else {
            break;
        };
        request["cursor"] = json!(cursor);
    }
    Ok(())
}

#[test]
fn perf_050_recommended_scan_limits_and_worker_bound_are_explicit() -> TestResult {
    let mut limits = ToolLimits::RECOMMENDED;
    assert_eq!(limits.search_threads, 0);
    assert_eq!(limits.search_max_files, 200_000);
    assert_eq!(limits.glob_entries, 250_000);
    assert_eq!(limits.search_max_bytes, 512 * 1024 * 1024);
    assert_eq!(limits.search_max_elapsed, Duration::from_secs(5));
    assert_eq!(limits.glob_max_elapsed, Duration::from_secs(2));
    for threads in [0, 1, 2, 8, 64] {
        limits.search_threads = threads;
        limits.validate()?;
    }
    for threads in [65, u16::MAX] {
        limits.search_threads = threads;
        assert!(limits.validate().is_err());
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig {cases:48,rng_seed:proptest::test_runner::RngSeed::Fixed(18),failure_persistence:None,..ProptestConfig::default()})]
    #[test]
    fn tool_u_005_parallel_pages_and_cursors_match_one_worker(
        files in prop::collection::vec(any::<u8>(), 0..=300),
        pattern in prop::sample::select(vec!["hit", "miss", "^hit", "hit [0-9]+", "ordinary|hit", "absent"]),
        mode in prop::sample::select(vec!["files_with_matches", "count", "content"]),
        bytes in 512usize..=8192,
        limit in 1u32..=40,
        depth in 0usize..=3,
    ) {
        let fixture = tree(&files)?;
        let mut cx = context(&fixture);
        cx.limits.search_page_bytes = bytes;
        let args = json!({"pattern":pattern,"path":"tree","output_mode":mode,"max_results":limit});
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        runtime.block_on(compare_pages(args,cx,depth)).map_err(|error| TestCaseError::fail(error.to_string()))?;
    }

    #[test]
    fn tool_u_005_parallel_byte_budget_keeps_an_ordered_result_prefix(
        files in prop::collection::vec(any::<u8>(), 1..=100),
        bytes in 1u64..=200_000,
        mode in prop::sample::select(vec!["files_with_matches", "count", "content"]),
    ) {
        let fixture=tree(&files)?;
        let runtime=tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        runtime.block_on(async {
            let mut cx=context(&fixture);
            let args=json!({"pattern":"hit","path":"tree","output_mode":mode});
            cx.limits.search_threads=1;
            let complete=Grep::new().call(args.clone(),&cx).await?;
            for threads in [1,2,8] {
                cx.limits.search_threads=threads;
                cx.limits.search_max_bytes=bytes;
                let bounded=Grep::new().call(args.clone(),&cx).await?;
                let status=bounded.search.as_ref().ok_or("missing status")?;
                assert!(status.scanned_bytes<=bytes);
                assert!(rows(&complete).starts_with(&rows(&bounded)));
                if status.reason.is_some() {assert_eq!(status.reason,Some(SearchStopReason::Bytes));}
            }
            Ok::<_,Box<dyn Error>>(())
        }).map_err(|error| TestCaseError::fail(error.to_string()))?;
    }
}

#[tokio::test]
async fn tool_u_005_first_file_over_byte_budget_contributes_no_rows() -> TestResult {
    let fixture = workspace_fixture()?;
    fs::write(fixture.root().join("first"), "hit\n".repeat(30_000))?;
    let mut cx = context(&fixture);
    cx.limits.search_max_bytes = 16_384;
    for threads in [1, 2, 8] {
        cx.limits.search_threads = threads;
        for mode in ["files_with_matches", "count", "content"] {
            let output = Grep::new()
                .call(
                    json!({"pattern":"hit","path":"first","output_mode":mode}),
                    &cx,
                )
                .await?;
            assert!(
                rows(&output).is_empty(),
                "a cut file must contribute no rows"
            );
            assert!(output.text.contains("narrow the path or glob"));
            let status = output.search.ok_or("missing status")?;
            assert_eq!(status.reason, Some(SearchStopReason::Bytes));
            assert_eq!(status.scanned_bytes, cx.limits.search_max_bytes);
            assert_eq!(status.scanned_files, 1);
            assert_eq!(status.next_cursor, None);
        }
    }
    Ok(())
}

#[tokio::test]
async fn tool_u_005_binary_classification_accounts_actual_reads_not_metadata_size() -> TestResult {
    let fixture = workspace_fixture()?;
    fs::create_dir(fixture.root().join("tree"))?;
    fs::write(
        fixture.root().join("tree/first"),
        format!("\0{}", "x".repeat(1024 * 1024)),
    )?;
    fs::write(fixture.root().join("tree/second"), "hit\n")?;
    let mut cx = context(&fixture);
    let binary = Grep::new()
        .call(json!({"pattern":"hit","path":"tree/first"}), &cx)
        .await?;
    let read = binary.search.ok_or("status")?.scanned_bytes;
    assert!(read < 1024 * 1024);
    cx.limits.search_max_bytes = read + 4;
    for threads in [1, 2, 8] {
        cx.limits.search_threads = threads;
        let output = Grep::new()
            .call(json!({"pattern":"hit","path":"tree"}), &cx)
            .await?;
        assert_eq!(rows(&output), ["tree/second"]);
        assert!(output.text.contains("[binary files skipped: 1]"));
        let status = output.search.ok_or("status")?;
        assert!(status.complete);
        assert_eq!(status.scanned_bytes, read + 4);
    }
    Ok(())
}

#[tokio::test]
async fn tool_u_005_long_line_before_binary_preserves_worker_equivalence() -> TestResult {
    let fixture = workspace_fixture()?;
    fs::create_dir(fixture.root().join("tree"))?;
    fs::write(
        fixture.root().join("tree/first"),
        format!("hit{}", "x".repeat(8 * 1024 * 1024)),
    )?;
    fs::write(
        fixture.root().join("tree/second"),
        format!("{}\0{}", "hit\n".repeat(20_000), "x".repeat(1024 * 1024)),
    )?;
    let cx = context(&fixture);
    for mode in ["files_with_matches", "count", "content"] {
        compare_pages(
            json!({"pattern":"hit","path":"tree","output_mode":mode}),
            cx.clone(),
            0,
        )
        .await?;
    }
    Ok(())
}

#[tokio::test]
async fn tool_u_005_parallel_file_budget_is_exact_in_path_order() -> TestResult {
    let fixture = tree(&[2; 40])?;
    let mut cx = context(&fixture);
    let args = json!({"pattern":"hit","path":"tree"});
    cx.limits.search_threads = 1;
    let complete = Grep::new().call(args.clone(), &cx).await?;
    let all = rows(&complete);
    for limit in [1, 2, 7, 39, 40] {
        for threads in [1, 2, 8] {
            cx.limits.search_threads = threads;
            cx.limits.search_max_files = limit;
            let output = Grep::new().call(args.clone(), &cx).await?;
            let status = output.search.as_ref().ok_or("missing status")?;
            assert_eq!(status.scanned_files, limit);
            assert_eq!(rows(&output), all[..usize::try_from(limit)?]);
            assert_eq!(
                status.reason,
                (limit < 40).then_some(SearchStopReason::Files)
            );
        }
    }
    Ok(())
}
