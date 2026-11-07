//! PERF-050: informational warm-cache search measurements on a generated checkout.
#[path = "performance/memory.rs"]
mod memory;
#[path = "support/context.rs"]
mod support;
use serde_json::{Value, json};
use std::{
    error::Error,
    fs,
    io::Write,
    process::Command,
    time::{Duration, Instant},
};
use tqa_testkit::{WorkspaceFixture, workspace_fixture};
use tqa_tools::{
    Tool, ToolContext,
    builtin::{Glob, Grep},
};

const SEARCHABLE_FILES: usize = 100_000;
const IGNORED_FILES: usize = 50_000;
const SAMPLES: usize = 7;
// The single-worker comparator needs a separate cap to measure its full scan.
const ONE_WORKER_BASELINE_TIME: Duration = Duration::from_secs(30);

fn checkout() -> Result<WorkspaceFixture, Box<dyn Error>> {
    let fixture = workspace_fixture()?;
    fs::write(fixture.root().join(".gitignore"), "build/\n")?;
    for directory in 0..100 {
        let path = fixture.root().join(format!("source-{directory:03}"));
        fs::create_dir(&path)?;
        for file in 0..SEARCHABLE_FILES / 100 {
            let target = directory == 99 && file == 999;
            let name = if target {
                "needle.rs".to_owned()
            } else {
                format!("file-{file:04}.rs")
            };
            fs::write(
                path.join(name),
                if target {
                    "needle\nzzz123\n"
                } else {
                    "ordinary source line\n"
                },
            )?;
        }
    }
    for directory in 0..50 {
        let path = fixture.root().join(format!("build/{directory:03}"));
        fs::create_dir_all(&path)?;
        for file in 0..IGNORED_FILES / 50 {
            fs::write(path.join(format!("{file:04}.rs")), "needle\nzzz123\n")?;
        }
    }
    Ok(fixture)
}

async fn measure(
    label: &str,
    tool: &dyn Tool,
    args: Value,
    cx: &ToolContext,
) -> Result<f64, Box<dyn Error>> {
    tool.call(args.clone(), cx).await?;
    let mut samples = Vec::with_capacity(SAMPLES);
    let mut last = None;
    for _ in 0..SAMPLES {
        let start = Instant::now();
        let output = tool.call(args.clone(), cx).await?;
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
        assert!(output.text.len() <= 8192);
        assert_eq!(
            output.text.lines().collect::<Vec<_>>(),
            ["source-099/needle.rs"]
        );
        let status = output.search.ok_or("missing search status")?;
        if cx.limits.search_threads == 1 {
            assert!(
                status.complete,
                "one-worker baseline must exhaust every measured search within 30 seconds: {status:?}"
            );
        } else {
            assert!(
                status.complete,
                "default limits must exhaust every measured search: {status:?}"
            );
        }
        last = Some(status);
    }
    samples.sort_by(f64::total_cmp);
    let status = last.ok_or("missing search status")?;
    writeln!(
        std::io::stdout().lock(),
        "PERF-050 {label}: p50_ms={:.3}, samples_ms={samples:?}, complete={}, reason={:?}, elapsed_limit_s={}",
        samples[SAMPLES / 2],
        status.complete,
        status.reason,
        cx.limits.search_max_elapsed.as_secs()
    )?;
    Ok(samples[SAMPLES / 2])
}

#[tokio::test]
#[ignore = "PERF-050 generates 150000 files; run explicitly for local performance reporting"]
async fn perf_050_warm_checkout_literal_regex_and_glob() -> Result<(), Box<dyn Error>> {
    let fixture = checkout()?;
    let cx = support::context(&fixture);
    let mut baseline = cx.clone();
    baseline.limits.search_threads = 1;
    baseline.limits.search_max_elapsed = ONE_WORKER_BASELINE_TIME;
    environment()?;
    for (label, pattern, literal) in [
        ("literal", "needle", true),
        ("regex", "^[a-z]{3}[0-9]{3}$", false),
    ] {
        let args = json!({"pattern":pattern,"fixed_string":literal,"glob":"**/*.rs"});
        let one = measure(
            &format!("one-worker baseline {label}"),
            &Grep::new(),
            args.clone(),
            &baseline,
        )
        .await?;
        let parallel = measure(
            &format!("default automatic {label}"),
            &Grep::new(),
            args,
            &cx,
        )
        .await?;
        if let Some(baseline) = ripgrep(&fixture, label, pattern, literal)? {
            writeln!(
                std::io::stdout().lock(),
                "PERF-050 {label}: speedup={:.3}, ratio_to_ripgrep={:.3}",
                one / parallel,
                parallel / baseline
            )?;
            assert!(
                parallel <= baseline * 1.5,
                "automatic grep exceeds 1.5 times ripgrep"
            );
        }
    }
    measure(
        "default glob",
        &Glob::new(),
        json!({"pattern":"**/needle.rs"}),
        &cx,
    )
    .await?;
    environment()?;
    Ok(())
}

fn environment() -> Result<(), Box<dyn Error>> {
    let uptime = Command::new("uptime").output()?;
    assert!(uptime.status.success());
    writeln!(
        std::io::stdout().lock(),
        "PERF-050 os={}, arch={}, available_workers={}, load={}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::thread::available_parallelism()?.get(),
        String::from_utf8_lossy(&uptime.stdout).trim()
    )?;
    Ok(())
}

fn ripgrep(
    fixture: &WorkspaceFixture,
    label: &str,
    pattern: &str,
    literal: bool,
) -> Result<Option<f64>, Box<dyn Error>> {
    let mut command = Command::new("rg");
    command.current_dir(fixture.root()).args([
        "--no-config",
        "--color",
        "never",
        "--files-with-matches",
        "--glob",
        "**/*.rs",
    ]);
    if literal {
        command.arg("--fixed-strings");
    }
    command.args(["--", pattern, "."]);
    match command.output() {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            writeln!(
                std::io::stdout().lock(),
                "PERF-050 ripgrep absent from PATH"
            )?;
            return Ok(None);
        }
        result => {
            assert!(result?.status.success(), "ripgrep warmup failed");
        }
    }
    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let start = Instant::now();
        let output = command.output()?;
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
        assert!(output.status.success(), "ripgrep failed");
        assert_eq!(
            String::from_utf8(output.stdout)?
                .lines()
                .collect::<Vec<_>>(),
            ["./source-099/needle.rs"]
        );
    }
    samples.sort_by(f64::total_cmp);
    let median = samples[SAMPLES / 2];
    writeln!(
        std::io::stdout().lock(),
        "PERF-050 ripgrep {label}: p50_ms={median:.3}, samples_ms={samples:?}"
    )?;
    Ok(Some(median))
}

#[tokio::test]
#[ignore = "TOOL-E-016 generates 200000 files; run explicitly (it writes to disk)"]
async fn tool_e_016_glob_over_two_hundred_thousand_files_is_bounded_in_time_and_size()
-> Result<(), Box<dyn Error>> {
    const DIRECTORIES: usize = 200;
    const FILES_PER_DIRECTORY: usize = 1000;
    let fixture = workspace_fixture()?;
    for directory in 0..DIRECTORIES {
        let path = fixture.root().join(format!("tree-{directory:03}"));
        fs::create_dir(&path)?;
        for file in 0..FILES_PER_DIRECTORY {
            fs::write(path.join(format!("{file:04}.txt")), "")?;
        }
    }
    let cx = support::context(&fixture);
    let budget = cx.limits.glob_max_elapsed;
    assert_eq!(
        budget,
        Duration::from_secs(2),
        "the recommended glob budget"
    );
    let started = Instant::now();
    let output = Glob::new().call(json!({"pattern":"**/*.txt"}), &cx).await?;
    let elapsed = started.elapsed();
    // Real time is measured because a real traversal is observed; the margin covers rendering one page.
    assert!(
        elapsed <= budget + Duration::from_millis(500),
        "glob took {elapsed:?}"
    );
    assert!(
        output.text.len() <= cx.limits.search_page_bytes,
        "one bounded page"
    );
    let marked = output.text.contains("[incomplete: scanned ")
        || output.text.contains("[partial: continue with cursor=");
    assert!(
        marked,
        "a truncation marker says the result is not the whole tree"
    );
    writeln!(
        std::io::stdout().lock(),
        "TOOL-E-016: 200000 files, glob returned in {elapsed:?}"
    )?;
    Ok(())
}
