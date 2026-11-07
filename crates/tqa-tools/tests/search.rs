//! TOOL-U-005: ripgrep-backed search modes, filters, pages and scan budgets.
#[path = "support/context.rs"]
mod support;
use proptest::prelude::*;
use serde_json::{Value, json};
use std::{
    error::Error,
    fs,
    sync::Arc,
    time::{Duration, Instant},
};
use tqa_testkit::{WorkspaceFixture, workspace_fixture};
use tqa_tools::{
    SearchStopReason, Tool, ToolError,
    builtin::{Glob, Grep},
};
type TestResult = Result<(), Box<dyn Error>>;

fn fixture() -> Result<WorkspaceFixture, Box<dyn Error>> {
    let fixture = workspace_fixture()?;
    fs::create_dir_all(fixture.root().join("src"))?;
    fs::write(
        fixture.root().join("src/alpha.rs"),
        "first\nneedle one\nbetween\nneedle needle\nlast\nfar\nzero\nend\nneedle three\n",
    )?;
    fs::write(fixture.root().join("src/beta.ts"), "NEEDLE\nonly\n")?;
    fs::write(fixture.root().join("gamma.txt"), "needle\n")?;
    Ok(fixture)
}
fn data(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| !line.starts_with('['))
        .map(str::to_owned)
        .collect()
}
async fn pages(
    tool: &dyn Tool,
    args: Value,
    cx: &tqa_tools::ToolContext,
) -> Result<Vec<String>, Box<dyn Error>> {
    let mut request = args;
    let mut rows = Vec::new();
    loop {
        let output = tool.call(request.clone(), cx).await?;
        assert!(output.text.len() <= 8192);
        rows.extend(data(&output.text));
        let status = output.search.ok_or("missing search metadata")?;
        assert_eq!(status.reason, None);
        if let Some(cursor) = status.next_cursor {
            assert!(!status.complete);
            assert!(
                output
                    .text
                    .contains(&format!("[partial: continue with cursor={cursor}]"))
            );
            request["cursor"] = json!(cursor);
        } else {
            assert!(status.complete);
            break;
        }
        assert!(rows.len() < 10000, "cursor did not advance");
    }
    Ok(rows)
}

#[tokio::test]
async fn tool_u_005_grep_modes_context_and_pattern_options() -> TestResult {
    let fixture = fixture()?;
    let cx = support::context(&fixture);
    let tool = Grep::new();
    let files = tool.call(json!({"pattern":"needle"}), &cx).await?;
    assert_eq!(data(&files.text), ["gamma.txt", "src/alpha.rs"]);
    assert!(files.text.contains("[binary files skipped: 1]"));
    let count = tool
        .call(json!({"pattern":"needle","output_mode":"count"}), &cx)
        .await?;
    assert_eq!(data(&count.text), ["gamma.txt:1", "src/alpha.rs:3"]);
    let content = tool
        .call(
            json!({"pattern":"needle","path":"src/alpha.rs","output_mode":"content"}),
            &cx,
        )
        .await?;
    assert_eq!(
        data(&content.text),
        [
            "src/alpha.rs:2:needle one",
            "src/alpha.rs:4:needle needle",
            "src/alpha.rs:9:needle three"
        ]
    );
    assert!(files.text.len() < count.text.len() && files.text.len() < content.text.len());
    Ok(())
}

#[tokio::test]
async fn tool_u_005_grep_context_and_pattern_options() -> TestResult {
    let fixture = fixture()?;
    let cx = support::context(&fixture);
    let tool = Grep::new();
    let surrounding = tool.call(json!({"pattern":"needle","path":"src/alpha.rs","output_mode":"content","context_lines":1}), &cx).await?;
    assert_eq!(
        data(&surrounding.text),
        [
            "src/alpha.rs-1-first",
            "src/alpha.rs:2:needle one",
            "src/alpha.rs-3-between",
            "src/alpha.rs:4:needle needle",
            "src/alpha.rs-5-last",
            "--",
            "src/alpha.rs-8-end",
            "src/alpha.rs:9:needle three"
        ]
    );
    let insensitive = tool
        .call(
            json!({"pattern":"needle","case_insensitive":true,"file_type":"ts"}),
            &cx,
        )
        .await?;
    assert_eq!(data(&insensitive.text), ["src/beta.ts"]);
    assert!(
        tool.call(
            json!({"pattern":"needle.*","fixed_string":true,"path":"src"}),
            &cx
        )
        .await?
        .text
        .is_empty()
    );
    assert_eq!(
        data(
            &tool
                .call(json!({"pattern":"^[a-z]+ [a-z]+$","glob":"*.rs"}), &cx)
                .await?
                .text
        ),
        ["src/alpha.rs"]
    );
    Ok(())
}

fn ignored_fixture() -> Result<WorkspaceFixture, Box<dyn Error>> {
    let fixture = fixture()?;
    for directory in ["node_modules", "custom", "nested", ".hidden", ".git/info"] {
        fs::create_dir_all(fixture.root().join(directory))?;
    }
    for path in [
        "drop.rs",
        "ignore.rs",
        "custom/a.rs",
        "excluded.rs",
        "nested/drop.rs",
        "nested/keep.rs",
        ".hidden/secret.rs",
        ".secret.rs",
    ] {
        fs::write(fixture.root().join(path), "needle\n")?;
    }
    fs::write(
        fixture.root().join(".gitignore"),
        "node_modules/\ndrop.rs\n",
    )?;
    fs::write(fixture.root().join("nested/.gitignore"), "*.rs\n!keep.rs\n")?;
    fs::write(fixture.root().join(".ignore"), "ignore.rs\n")?;
    fs::write(fixture.root().join(".tqaignore"), "custom/\n")?;
    fs::write(fixture.root().join(".git/info/exclude"), "excluded.rs\n")?;
    for n in 0..10_000 {
        fs::write(
            fixture.root().join(format!("node_modules/{n}.rs")),
            "needle\n",
        )?;
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(fixture.root().join("src"), fixture.root().join("alias"))?;
    Ok(fixture)
}

#[tokio::test]
async fn tool_u_005_ignore_rules_nested_negation_hidden_and_no_symlinks() -> TestResult {
    let fixture = ignored_fixture()?;
    let cx = support::context(&fixture);
    let glob = Glob::new().call(json!({"pattern":"**/*.rs"}), &cx).await?;
    assert_eq!(data(&glob.text), ["nested/keep.rs", "src/alpha.rs"]);
    assert!(
        glob.search.ok_or("metadata")?.scanned_files < 20,
        "ignored directory was walked"
    );
    let grep = Grep::new()
        .call(json!({"pattern":"needle","glob":"**/*.rs"}), &cx)
        .await?;
    assert_eq!(data(&grep.text), ["nested/keep.rs", "src/alpha.rs"]);
    assert_eq!(
        data(
            &Glob::new()
                .call(json!({"pattern":"**/.hidden/*.rs"}), &cx)
                .await?
                .text
        ),
        [".hidden/secret.rs"]
    );
    assert_eq!(
        data(
            &Grep::new()
                .call(json!({"pattern":"needle","path":".hidden"}), &cx)
                .await?
                .text
        ),
        [".hidden/secret.rs"]
    );
    assert_eq!(
        data(
            &Grep::new()
                .call(json!({"pattern":"needle","glob":".secret.rs"}), &cx)
                .await?
                .text
        ),
        [".secret.rs"]
    );
    assert!(
        Grep::new()
            .call(json!({"pattern":"secret","path":"alias"}), &cx)
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn tool_u_005_pages_are_sorted_complete_and_bound_to_options() -> TestResult {
    let fixture = fixture()?;
    for n in 0..80 {
        fs::write(fixture.root().join(format!("page-{n:03}.txt")), "needle\n")?;
    }
    let mut cx = support::context(&fixture);
    cx.limits.search_page_bytes = 512;
    for tool in [
        Box::new(Grep::new()) as Box<dyn Tool>,
        Box::new(Glob::new()),
    ] {
        let args = if tool.spec().name.as_str() == "grep" {
            json!({"pattern":"needle","glob":"page-*.txt","max_results":3})
        } else {
            json!({"pattern":"page-*.txt","max_results":3})
        };
        assert_eq!(
            pages(tool.as_ref(), args.clone(), &cx).await?,
            (0..80)
                .map(|n| format!("page-{n:03}.txt"))
                .collect::<Vec<_>>()
        );
        let first = tool.call(args.clone(), &cx).await?;
        let cursor = first
            .search
            .and_then(|status| status.next_cursor)
            .ok_or("cursor")?;
        let mut changed = args.clone();
        changed["pattern"] = json!("different");
        changed["cursor"] = json!(cursor);
        let error = tool
            .call(changed, &cx)
            .await
            .err()
            .ok_or("changed cursor accepted")?;
        assert!(error.model_message().contains("search again"));
        let mut changed = args;
        changed["cursor"] = json!("garbage!");
        assert!(matches!(
            tool.call(changed, &cx).await,
            Err(ToolError::InvalidArgs { .. })
        ));
    }
    Ok(())
}

#[tokio::test]
async fn tool_u_005_content_truncation_and_cursor_preserve_all_rows() -> TestResult {
    let fixture = fixture()?;
    let mut cx = support::context(&fixture);
    let line = format!("needle{}\n", "界".repeat(1000));
    fs::write(fixture.root().join("wide"), line.repeat(80))?;
    let tool = Grep::new();
    let all = pages(
        &tool,
        json!({"pattern":"needle","path":"wide","output_mode":"content"}),
        &cx,
    )
    .await?;
    assert_eq!(all.len(), 80);
    for (n, row) in all.iter().enumerate() {
        assert!(row.len() <= 500);
        assert!(row.starts_with(&format!("wide:{}:needle", n + 1)));
        assert!(row.ends_with("[truncated]"));
    }
    cx.limits.search_page_bytes = 512;
    let small = tool
        .call(
            json!({"pattern":"needle","path":"wide","output_mode":"content"}),
            &cx,
        )
        .await?;
    assert!(small.text.len() <= 512);
    Ok(())
}

struct AdvancingClock(std::sync::atomic::AtomicU64, Instant);
impl tqa_types::Clock for AdvancingClock {
    fn now(&self) -> Instant {
        self.1 + Duration::from_millis(self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }
}
#[tokio::test]
async fn tool_u_005_scan_budget_notices_and_structured_reasons() -> TestResult {
    let fixture = fixture()?;
    for tool in [
        Box::new(Grep::new()) as Box<dyn Tool>,
        Box::new(Glob::new()),
    ] {
        let args = if tool.spec().name.as_str() == "grep" {
            json!({"pattern":"needle"})
        } else {
            json!({"pattern":"**/*"})
        };
        for reason in [
            SearchStopReason::Files,
            SearchStopReason::Bytes,
            SearchStopReason::Time,
        ] {
            let mut cx = support::context(&fixture);
            set_budget(&mut cx, reason)?;
            let output = tool.call(args.clone(), &cx).await?;
            let status = output.search.ok_or("metadata")?;
            assert!(!status.complete);
            assert_eq!(status.reason, Some(reason));
            assert!(
                output.text.contains("[incomplete: scanned ")
                    && output.text.contains("; narrow the path or glob]")
            );
            assert!(status.scanned_files <= cx.limits.search_max_files);
            assert!(status.scanned_bytes <= cx.limits.search_max_bytes);
            assert!(output.text.len() <= 8192);
        }
    }
    Ok(())
}
fn set_budget(cx: &mut tqa_tools::ToolContext, reason: SearchStopReason) -> TestResult {
    match reason {
        SearchStopReason::Files => cx.limits.search_max_files = 1,
        SearchStopReason::Bytes => cx.limits.search_max_bytes = 2,
        SearchStopReason::Time => {
            cx.limits.search_max_elapsed = Duration::from_millis(2);
            cx.limits.glob_max_elapsed = Duration::from_millis(2);
            cx.clock = Arc::new(AdvancingClock(
                std::sync::atomic::AtomicU64::new(0),
                Instant::now(),
            ));
        }
        _ => return Err("unexpected reason".into()),
    }
    Ok(())
}

#[tokio::test]
async fn tool_u_005_invalid_search_options_and_binary_files() -> TestResult {
    let fixture = fixture()?;
    let cx = support::context(&fixture);
    for args in [
        json!({"pattern":"["}),
        json!({"pattern":"needle","context_lines":1}),
        json!({"pattern":"needle","output_mode":"content","context_lines":11}),
        json!({"pattern":"needle","file_type":"not_a_type"}),
        json!({"pattern":"needle","max_results":0}),
        json!({"pattern":"needle","glob":"["}),
    ] {
        let error = Grep::new()
            .call(args, &cx)
            .await
            .err()
            .ok_or("invalid accepted")?;
        assert!(matches!(error, ToolError::InvalidArgs { .. }), "{error:?}");
    }
    fs::write(
        fixture.root().join("late-binary"),
        format!("{}\0", "needle\n".repeat(2000)),
    )?;
    let output = Grep::new()
        .call(
            json!({"pattern":"needle","path":"late-binary","output_mode":"content"}),
            &cx,
        )
        .await?;
    assert_eq!(output.text, "[binary files skipped: 1]");
    assert!(
        Grep::new()
            .call(json!({"pattern":"needle","path":"../escape"}), &cx)
            .await
            .is_err()
    );
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig {cases:50,rng_seed:proptest::test_runner::RngSeed::Fixed(15),failure_persistence:None,..ProptestConfig::default()})]
    #[test]
    fn tool_u_005_cursor_walk_equals_unpaged_search(contents in prop::collection::vec(any::<bool>(), 1..60), limit in 1u32..8, context in 0u8..3) {
        let fixture = workspace_fixture()?;
        let source: String = contents.iter().map(|matches| if *matches {"hit\n"} else {"miss\n"}).collect();
        fs::write(fixture.root().join("random"), source)?;
        let cx = support::context(&fixture);
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        let (paged, entire) = runtime.block_on(async {
            let args = json!({"pattern":"hit","path":"random","output_mode":"content","context_lines":context});
            let entire = Grep::new().call(args.clone(), &cx).await?;
            let mut paged = args; paged["max_results"] = json!(limit);
            Ok::<_, Box<dyn Error>>((pages(&Grep::new(), paged, &cx).await?, data(&entire.text)))
        }).map_err(|error| TestCaseError::fail(error.to_string()))?;
        prop_assert_eq!(paged, entire);
    }
}

#[tokio::test]
async fn tool_e_016_glob_has_its_own_shorter_time_budget() -> TestResult {
    let fixture = fixture()?;
    let mut cx = support::context(&fixture);
    // Each clock read advances one millisecond, so a 2 ms glob budget runs out while a 60 s search budget does not.
    cx.clock = Arc::new(AdvancingClock(
        std::sync::atomic::AtomicU64::new(0),
        Instant::now(),
    ));
    cx.limits.glob_max_elapsed = Duration::from_millis(2);
    cx.limits.search_max_elapsed = Duration::from_mins(1);
    let glob = Glob::new().call(json!({"pattern":"**/*"}), &cx).await?;
    let status = glob.search.ok_or("glob metadata")?;
    assert_eq!(
        (status.complete, status.reason),
        (false, Some(SearchStopReason::Time))
    );
    assert!(glob.text.contains("[incomplete: scanned "));
    let grep = Grep::new().call(json!({"pattern":"needle"}), &cx).await?;
    assert!(
        grep.search.ok_or("grep metadata")?.complete,
        "grep keeps the search budget"
    );
    cx.limits.glob_max_elapsed = Duration::ZERO;
    assert!(
        cx.limits.validate().is_err(),
        "a zero glob budget is refused"
    );
    Ok(())
}
