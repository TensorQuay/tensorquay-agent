use super::*;
use crate::e2e::{
    classify::{CheckOutcome, Verdict},
    result::{Arm, DecisionTally, Tally},
};

fn attempt(task: &str, number: u32, rerun_of: Option<u32>, class: Class) -> Attempt {
    Attempt {
        task_id: task.into(),
        attempt: number,
        rerun_of,
        conversation_id: None,
        wall_ms: 1500,
        tally: Tally {
            steps: 3,
            prompt_tokens: 100,
            completion_tokens: 20,
            compactions: 0,
            provider_retries: 0,
        },
        check: (class == Class::Pass).then_some(CheckOutcome::Passed),
        detail: None,
        verdict: Verdict {
            class,
            evidence_seq: None,
        },
        workspace_id: Some(format!("w-{task}-{number}")),
        arm: Arm::Model,
        round: 1,
        pair_position: 1,
        decisions: DecisionTally::default(),
    }
}
fn rows(attempts: &[Attempt], expects: bool) -> Vec<Row<'_>> {
    attempts
        .iter()
        .map(|attempt| Row {
            attempt,
            expects_compaction: expects,
        })
        .collect()
}

#[test]
fn tk_u_008_ninety_percent_without_harness_failures_passes_with_its_interval() {
    let mut attempts: Vec<Attempt> = (0..9)
        .map(|n| attempt(&format!("t{n}"), 1, None, Class::Pass))
        .collect();
    attempts.push(attempt("t9", 1, None, Class::ModelWrongWork));
    let text = render(&Run {
        route: "local",
        row: "E2E-WAVE-001",
        dry_run: false,
        rows: &rows(&attempts, false),
        skipped: &[],
        cleanup: Cleanup::Kept,
    });
    assert!(
        text.contains(
            "**E2E-WAVE-001: PASS.** Completion 9/10 = 90.0 % (95 % Wilson interval 59.6 to 98.2 %)"
        ),
        "{text}"
    );
    assert!(
        text.contains("| t9 | 1 | – | model.wrong_work | not run | 3 | 120 | 1.5 |"),
        "{text}"
    );
    assert!(
        (wilson(20, 20).0 - 0.839).abs() < 0.001,
        "20 of 20 still has a lower bound near 84 %"
    );
    assert!(wilson(0, 0).0.abs() < f64::EPSILON && wilson(0, 0).1.abs() < f64::EPSILON);
}

#[test]
fn tk_u_008_a_rerun_completes_the_task_but_its_provider_failure_still_fails_the_gate() {
    let mut failed = attempt("t1", 3, None, Class::HarnessDriver);
    failed.detail = Some("send the task: the gateway answered 409".into());
    let explained = render(&Run {
        route: "local",
        row: "E2E-WAVE-001",
        dry_run: false,
        rows: &rows(std::slice::from_ref(&failed), false),
        skipped: &[],
        cleanup: Cleanup::Kept,
    });
    assert!(
        explained.contains(
            "## Driver failures\n\n- t1 attempt 3: send the task: the gateway answered 409"
        ),
        "{explained}"
    );
    let attempts = [
        attempt("t0", 1, None, Class::HarnessProvider),
        attempt("t0", 2, Some(1), Class::Pass),
    ];
    let text = render(&Run {
        route: "local",
        row: "E2E-WAVE-001",
        dry_run: false,
        rows: &rows(&attempts, false),
        skipped: &[],
        cleanup: Cleanup::Kept,
    });
    assert!(
        text.contains("**E2E-WAVE-001: FAIL.** Completion 1/1 = 100.0 %"),
        "{text}"
    );
    assert!(
        text.contains("harness failures over all attempts: 1."),
        "{text}"
    );
    assert!(text.contains("| t0 | 2 | 1 | pass | passed |"), "{text}");
}

#[test]
fn tk_u_008_missed_compaction_is_a_harness_expectation_on_real_routes_only() {
    let attempts = [attempt("long", 1, None, Class::Pass)];
    let skipped = ["later".to_owned()];
    let real = render(&Run {
        route: "local",
        row: "E2E-WAVE-001",
        dry_run: false,
        rows: &rows(&attempts, true),
        skipped: &skipped,
        cleanup: Cleanup::Kept,
    });
    assert!(
        real.contains("- `harness.expectation`: long attempt 1 recorded no compaction."),
        "{real}"
    );
    assert!(
        real.contains("**E2E-WAVE-001: PASS.**"),
        "an expectation is not counted against the model: {real}"
    );
    assert!(
        real.contains("## Skipped for the run's token budget\n\n- later"),
        "{real}"
    );
    let dry = render(&Run {
        route: "fake",
        row: "E2E-WAVE-001",
        dry_run: true,
        rows: &rows(&attempts, true),
        skipped: &[],
        cleanup: Cleanup::Kept,
    });
    assert!(
        !dry.contains("harness.expectation") && dry.contains("not a gate result"),
        "{dry}"
    );
}

// TEST: TK-U-027
#[test]
fn tk_u_027_the_summary_says_what_the_run_left_on_the_server() {
    let attempts = vec![
        attempt("a", 1, None, Class::Pass),
        attempt("b", 2, None, Class::ModelWrongWork),
    ];
    let rows = rows(&attempts, false);
    let summary = |cleanup: Cleanup| {
        render(&Run {
            route: "local",
            row: "E2E-WAVE-001",
            dry_run: false,
            rows: &rows,
            skipped: &[],
            cleanup,
        })
    };
    let text = summary(Cleanup::Done { left: vec![] });
    assert!(
        text.contains("Cleanup: every attempt's task and workspace were deleted from the server."),
        "{text}"
    );
    assert!(!text.contains("## Left on the server"), "{text}");
    let text = summary(Cleanup::Done {
        left: vec!["w-b-2".into()],
    });
    assert!(
        text.contains("## Left on the server\n\n- workspace `w-b-2` could not be deleted in time; delete it by hand."),
        "{text}"
    );
    assert!(!text.contains("every attempt's task"), "{text}");
    let text = summary(Cleanup::Kept);
    assert!(
        text.contains(
            "Cleanup: kept on the server (--keep-workspaces): 2 attempts' tasks and workspaces."
        ),
        "{text}"
    );
}

// TEST: TK-U-034
#[test]
fn tk_u_034_a_comparison_run_sums_up_each_arm_and_a_plain_run_shows_no_arms() {
    let engine = |task: &str, number, class, calls, fallbacks| {
        let mut row = attempt(task, number, None, class);
        row.arm = Arm::Engine;
        row.decisions.engine_calls = calls;
        row.decisions.fallbacks.insert("timeout".into(), fallbacks);
        row
    };
    let attempts = vec![
        attempt("t1", 1, None, Class::Pass),
        engine("t1", 2, Class::Pass, 5, 1),
        attempt("t2", 3, None, Class::ModelWrongWork),
        engine("t2", 4, Class::Pass, 7, 0),
    ];
    let run = |attempts| Run {
        route: "local",
        row: "E2E-WAVE-001",
        dry_run: false,
        rows: attempts,
        skipped: &[],
        cleanup: Cleanup::Kept,
    };
    let compared = rows(&attempts, false);
    let text = render(&run(&compared));
    assert!(text.contains("## Arms (E2E-DECISION-008)"), "{text}");
    assert!(text.contains("| model | 2 | 1 | 0 | 0 |"), "{text}");
    assert!(text.contains("| engine | 2 | 2 | 12 | 1 |"), "{text}");
    let plain = [attempt("t1", 1, None, Class::Pass)];
    let plain = rows(&plain, false);
    assert!(!render(&run(&plain)).contains("## Arms"));
}

// TEST: TK-U-008
/// The verdict names the run's row, so a fallback run's summary says E2E-FALLBACK-004, not E2E-WAVE-001 (the c14b-rc
/// records, 7 Oct 2026).
#[test]
fn tk_u_008_the_verdict_names_the_runs_row() {
    let attempts = [
        attempt("t1", 1, None, Class::Pass),
        attempt("t2", 2, None, Class::ModelUnreadable),
    ];
    let text = render(&Run {
        route: "local-text",
        row: "E2E-FALLBACK-004",
        dry_run: false,
        rows: &rows(&attempts, false),
        skipped: &[],
        cleanup: Cleanup::Kept,
    });
    assert!(
        text.contains("**E2E-FALLBACK-004: FAIL.** Completion 1/2 = 50.0 %")
            && !text.contains("E2E-WAVE-001"),
        "{text}"
    );
}
