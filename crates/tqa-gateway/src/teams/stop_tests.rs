//! GW-U-132: the agent team's end note in the gateway's fixed sentences (ADR-0034 point 10).
use super::{Parts, bytes, reason, sentences};
use tqa_types::{EndFile, EndReason, EndReport};

/// The note's parts, for `sentences`.
fn parts<'a>(
    reason: EndReason,
    summary: Option<&'a str>,
    reports: &'a [EndReport],
    files: &'a [EndFile],
) -> Parts<'a> {
    Parts {
        reason,
        summary,
        reports,
        files,
    }
}

fn report(member: &str, seq: u64) -> EndReport {
    EndReport {
        member: member.into(),
        seq,
    }
}

fn file(member: &str, seq: u64, name: &str, size: u64) -> EndFile {
    EndFile {
        member: member.into(),
        seq,
        name: name.into(),
        size,
    }
}

// TEST: GW-U-132
#[test]
fn a_finished_team_says_why_who_reported_and_what_came_back_never_its_tokens() {
    let note = sentences(
        &parts(
            EndReason::Done,
            Some("The report is in report.md."),
            &[
                report("researcher", 12),
                report("coder", 15),
                report("researcher", 20),
            ],
            &[
                file("researcher", 12, "results.md", 1204),
                file("coder", 15, "fix.patch", 230),
            ],
        ),
        30,
    );
    assert_eq!(
        note,
        "The team stopped: the lead said the work is done.\n\
         The lead's summary: The report is in report.md.\n\
         Reports: researcher (messages 12, 20); coder (message 15).\n\
         Files handed back: results.md from researcher (1.2 KB), fix.patch from coder (230 bytes).\n\
         Every task of the team is an ordinary task again."
    );
    assert!(
        !note.to_lowercase().contains("token"),
        "the tokens stay in the parts"
    );
}

// TEST: GW-U-132
#[test]
fn every_stop_reason_has_its_own_plain_first_line() {
    let first = |why| {
        sentences(&parts(why, None, &[], &[]), 45)
            .lines()
            .next()
            .map(str::to_owned)
    };
    for (why, line) in [
        (
            EndReason::Done,
            "The team stopped: the lead said the work is done.",
        ),
        (EndReason::Stopped, "The team stopped: a person stopped it."),
        (
            EndReason::Budget,
            "The team stopped: it used its whole budget.",
        ),
        (
            EndReason::Deadline,
            "The team stopped: its time limit passed.",
        ),
        (
            EndReason::Idle,
            "The team stopped: nothing happened in it for 45 minutes.",
        ),
        (
            EndReason::LeadDeleted,
            "The team stopped: the lead task was deleted.",
        ),
        (
            EndReason::PersonDisabled,
            "The team stopped: its person's account was turned off.",
        ),
    ] {
        assert_eq!(first(why).as_deref(), Some(line), "{why:?}");
    }
}

// TEST: GW-U-132
#[test]
fn a_team_with_nothing_to_show_says_so_without_empty_lists() {
    let note = sentences(&parts(EndReason::Deadline, Some("  "), &[], &[]), 30);
    assert_eq!(
        note,
        "The team stopped: its time limit passed.\nNo helper sent a report.\n\
         No files were handed back.\nEvery task of the team is an ordinary task again."
    );
}

// TEST: GW-U-132
#[test]
fn a_long_file_list_names_twenty_and_counts_the_rest() {
    let many: Vec<EndFile> = (0..23)
        .map(|n| file("coder", 9, &format!("part-{n}.csv"), 2_097_152))
        .collect();
    let note = sentences(
        &parts(EndReason::Budget, None, &[report("coder", 9)], &many),
        30,
    );
    let line = note
        .lines()
        .find(|line| line.starts_with("Files handed back"))
        .unwrap_or_default();
    assert!(line.contains("part-19.csv from coder (2 MB)"), "{line}");
    assert!(!line.contains("part-20.csv"), "{line}");
    assert!(line.ends_with(", and 3 more."), "{line}");
}

// TEST: GW-U-132
#[test]
fn sizes_and_stored_reasons_read_plainly() {
    assert_eq!(
        [bytes(0), bytes(1023), bytes(1024), bytes(5_242_880)],
        ["0 bytes", "1023 bytes", "1 KB", "5 MB"]
    );
    assert_eq!(reason(Some("idle")), EndReason::Idle);
    assert_eq!(reason(Some("lead_deleted")), EndReason::LeadDeleted);
    assert_eq!(
        reason(None),
        EndReason::Stopped,
        "a stop without a recorded reason reads as a person's Stop"
    );
}
