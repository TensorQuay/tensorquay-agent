//! `summary.md` for one run. Completion counts each task's final
//! attempt; harness failures count every attempt, including a provider failure a rerun superseded, so a
//! rerun can never hide one. A real-model attempt of a task that expects compaction but recorded none is
//! listed as `harness.expectation`: a finding about the harness, not counted against the model.
use super::{
    classify::Class,
    result::{Arm, Attempt},
};
use std::fmt::Write as _;

/// Standard normal quantile for a two-sided 95 % interval.
const Z95: f64 = 1.96;
/// PRD metric M1.
const COMPLETION_TARGET: f64 = 0.9;

/// One attempt and the task fact the summary needs.
#[derive(Debug)]
pub struct Row<'a> {
    /// The attempt as written to `attempts.jsonl`.
    pub attempt: &'a Attempt,
    /// Whether the task expects a compaction.
    pub expects_compaction: bool,
}

/// What the summary covers.
#[derive(Debug)]
pub struct Run<'a> {
    /// The route under test.
    pub route: &'a str,
    /// The test row the verdict names (E2E-WAVE-001, E2E-FALLBACK-004, the phase demo...).
    pub row: &'a str,
    /// A dry run against the fake model is never a gate result.
    pub dry_run: bool,
    /// Every attempt, in order.
    pub rows: &'a [Row<'a>],
    /// Tasks not started because the run's token budget was spent.
    pub skipped: &'a [String],
    /// What the run left on the server.
    pub cleanup: Cleanup,
}

/// What the run did with its tasks and workspaces on the server (TK-U-027).
#[derive(Debug, PartialEq, Eq)]
pub enum Cleanup {
    /// `--keep-workspaces`: all kept.
    Kept,
    /// Deleted, except the workspaces named, still busy when the sweep's bound passed.
    Done {
        /// The workspaces left on the server.
        left: Vec<String>,
    },
}

/// Renders the summary.
#[must_use]
pub fn render(run: &Run<'_>) -> String {
    let mut text = verdict(run);
    text.push_str(&arms(run.rows));
    text.push_str(&attempts(run.rows));
    text.push_str(&findings(run));
    text.push_str(&cleanup(run));
    text
}

/// The cleanup line, and a section naming any workspace left on the server.
fn cleanup(run: &Run<'_>) -> String {
    match &run.cleanup {
        Cleanup::Kept => format!(
            "\nCleanup: kept on the server (--keep-workspaces): {} attempts' tasks and workspaces.\n",
            run.rows.len()
        ),
        Cleanup::Done { left } if left.is_empty() => {
            "\nCleanup: every attempt's task and workspace were deleted from the server.\n"
                .to_owned()
        }
        Cleanup::Done { left } => {
            let mut text = "\n## Left on the server\n\n".to_owned();
            for workspace in left {
                let _ = writeln!(
                    text,
                    "- workspace `{workspace}` could not be deleted in time; delete it by hand."
                );
            }
            text
        }
    }
}

/// The header and the verdict line, for the run's row.
fn verdict(run: &Run<'_>) -> String {
    let finals: Vec<&Row<'_>> = run
        .rows
        .iter()
        .filter(|row| !superseded(run.rows, row))
        .collect();
    let passes = finals
        .iter()
        .filter(|row| row.attempt.verdict.class == Class::Pass)
        .count();
    let harness = run
        .rows
        .iter()
        .filter(|row| row.attempt.verdict.class.is_harness())
        .count();
    let rate = ratio(passes, finals.len());
    let (low, high) = wilson(passes, finals.len());
    let outcome = if run.dry_run {
        "dry run against the fake model, not a gate result"
    } else if rate >= COMPLETION_TARGET && harness == 0 {
        "PASS"
    } else {
        "FAIL"
    };
    let mut text = format!(
        "# E2E run summary\n\nRoute: `{}`. Tasks: {}.",
        run.route,
        finals.len()
    );
    let _ = writeln!(
        text,
        " Skipped for the run's token budget: {}.\n",
        run.skipped.len()
    );
    let _ = writeln!(
        text,
        "**{}: {outcome}.** Completion {passes}/{} = {:.1} % (95 % Wilson interval {:.1} to {:.1} %); \
         harness failures over all attempts: {harness}. The target is at least 90 % and no harness failure.\n",
        run.row,
        finals.len(),
        rate * 100.0,
        low * 100.0,
        high * 100.0
    );
    text
}

/// For a comparison run (E2E-DECISION-008), one line per arm over its final attempts: completed, the engine's calls
/// and its fallbacks. Nothing for a run without an engine arm. The paired analysis is the evaluation's own.
fn arms(rows: &[Row<'_>]) -> String {
    if !rows.iter().any(|row| row.attempt.arm == Arm::Engine) {
        return String::new();
    }
    let mut text =
        "## Arms (E2E-DECISION-008)\n\n| Arm | Attempts | Completed | Engine calls | Fallbacks |\n\
                    |---|---|---|---|---|\n"
            .to_owned();
    for (arm, name) in [(Arm::Model, "model"), (Arm::Engine, "engine")] {
        let finals: Vec<&Attempt> = rows
            .iter()
            .filter(|row| row.attempt.arm == arm && !superseded(rows, row))
            .map(|row| row.attempt)
            .collect();
        let completed = finals
            .iter()
            .filter(|attempt| attempt.verdict.class == Class::Pass)
            .count();
        let calls: u64 = finals
            .iter()
            .map(|attempt| attempt.decisions.engine_calls)
            .sum();
        let fallbacks: u32 = finals
            .iter()
            .flat_map(|attempt| attempt.decisions.fallbacks.values())
            .sum();
        let _ = writeln!(
            text,
            "| {name} | {} | {completed} | {calls} | {fallbacks} |",
            finals.len()
        );
    }
    text.push('\n');
    text
}

/// One table row per attempt, reruns included.
fn attempts(rows: &[Row<'_>]) -> String {
    let mut text =
        "## Attempts\n\n| Task | Attempt | Rerun of | Class | Check | Steps | Tokens | Wall s |\n"
            .to_owned();
    text.push_str("|---|---|---|---|---|---|---|---|\n");
    for row in rows {
        let attempt = row.attempt;
        let class = serde_json::to_value(attempt.verdict.class)
            .map(|value| value.as_str().unwrap_or_default().to_owned())
            .unwrap_or_default();
        let check = attempt.check.map_or("not run".to_owned(), |check| {
            format!("{check:?}").to_lowercase()
        });
        let rerun = attempt.rerun_of.map_or("–".to_owned(), |of| of.to_string());
        let tokens = attempt
            .tally
            .prompt_tokens
            .saturating_add(attempt.tally.completion_tokens);
        let seconds = f64::from(u32::try_from(attempt.wall_ms).unwrap_or(u32::MAX)) / 1000.0;
        let _ = writeln!(
            text,
            "| {} | {} | {rerun} | {class} | {check} | {} | {tokens} | {seconds:.1} |",
            attempt.task_id, attempt.attempt, attempt.tally.steps
        );
    }
    text
}

/// Harness expectations and tasks skipped for the run's token budget.
fn findings(run: &Run<'_>) -> String {
    let mut text = String::new();
    let missed = run
        .rows
        .iter()
        .filter(|row| !run.dry_run && row.expects_compaction && row.attempt.tally.compactions == 0);
    for (index, row) in missed.enumerate() {
        if index == 0 {
            text.push_str("\n## Harness expectations (not counted against the model)\n\n");
        }
        let _ = writeln!(
            text,
            "- `harness.expectation`: {} attempt {} recorded no compaction.",
            row.attempt.task_id, row.attempt.attempt
        );
    }
    let refused = run
        .rows
        .iter()
        .filter(|row| row.attempt.verdict.class == super::classify::Class::ModelToolArgs);
    for (index, row) in refused.enumerate() {
        if index == 0 {
            text.push_str(
                "\n## Refused tool arguments (each reviewed by hand: our strictness is reclassified as the harness's)\n\n",
            );
        }
        let _ = writeln!(
            text,
            "- {} attempt {}: {}",
            row.attempt.task_id,
            row.attempt.attempt,
            row.attempt
                .detail
                .as_deref()
                .unwrap_or("no refused call recorded")
        );
    }
    let explained = run
        .rows
        .iter()
        .filter(|row| row.attempt.verdict.class != super::classify::Class::ModelToolArgs)
        .filter_map(|row| Some((row.attempt, row.attempt.detail.as_deref()?)));
    for (index, (attempt, detail)) in explained.enumerate() {
        if index == 0 {
            text.push_str("\n## Driver failures\n\n");
        }
        let _ = writeln!(
            text,
            "- {} attempt {}: {detail}",
            attempt.task_id, attempt.attempt
        );
    }
    if !run.skipped.is_empty() {
        text.push_str("\n## Skipped for the run's token budget\n\n");
        for task in run.skipped {
            let _ = writeln!(text, "- {task}");
        }
    }
    text
}

/// An attempt that a later attempt reran.
fn superseded(rows: &[Row<'_>], row: &Row<'_>) -> bool {
    rows.iter().any(|other| {
        other.attempt.task_id == row.attempt.task_id
            && other.attempt.rerun_of == Some(row.attempt.attempt)
    })
}

fn ratio(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        return 0.0;
    }
    count(part) / count(whole)
}

fn count(value: usize) -> f64 {
    f64::from(u32::try_from(value).unwrap_or(u32::MAX))
}

/// The Wilson score interval for `passes` of `total` at 95 %; (0, 0) when there is nothing to measure.
fn wilson(passes: usize, total: usize) -> (f64, f64) {
    if total == 0 {
        return (0.0, 0.0);
    }
    let (n, p) = (count(total), ratio(passes, total));
    let denominator = 1.0 + Z95 * Z95 / n;
    let centre = (p + Z95 * Z95 / (2.0 * n)) / denominator;
    let half = Z95 * (p * (1.0 - p) / n + Z95 * Z95 / (4.0 * n * n)).sqrt() / denominator;
    ((centre - half).max(0.0), (centre + half).min(1.0))
}

#[cfg(test)]
mod tests;
