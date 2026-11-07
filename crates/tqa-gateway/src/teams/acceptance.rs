//! The administrator's acceptance check for `done` (ADR-0034 point 10, GW-I-198): an optional command the lead's
//! runtime runs in the lead's workspace before the team stops, bounded in time, reporting its exit and the end of its
//! output with `done`. The gateway holds `done` to that report, against the check set when `done` is decided; a check
//! that didn't run, ran another command or failed refuses `done` with `acceptance_failed`, and the team goes on.
use crate::GatewayError;
use serde_json::json;
use sqlx::PgConnection;
use tqa_types::{CheckReport, MAX_CHECK_OUTPUT_BYTES, last_bytes};

/// No report while a check is set: the lead's runtime predates the check, which happens only mid-upgrade.
const NOT_RUN: &str = "the acceptance check didn't run: this task's environment is older than the server. Stop the \
                       task, then send it a message so it starts again on the new version, and ask it to finish";
/// The report is for another command: the administrator changed the check while it ran.
const CHANGED: &str =
    "the acceptance check changed while it ran; call done again to run the new one";
/// The check ran and failed: its exit and output are the details.
const FAILED: &str = "the acceptance check failed; fix what it found, then call done again";

/// Holds the lead's `done` to the administrator's acceptance check, read in `done`'s transaction: unset, `done` goes
/// on whatever `report` says; set, only a report of that command exiting 0 within its time lets it go on.
///
/// # Errors
/// `409 acceptance_failed`, with `details: {exit_code, timed_out, output}` (the output's last 4 KiB) when the check
/// ran and failed; storage failure.
pub(crate) async fn check(
    connection: &mut PgConnection,
    report: Option<&CheckReport>,
) -> Result<(), GatewayError> {
    let Some(command) = crate::admin::agent_teams::acceptance_check(connection).await? else {
        return Ok(());
    };
    let Some(report) = report else {
        return Err(GatewayError::AcceptanceFailed(NOT_RUN, None));
    };
    if report.command != command {
        return Err(GatewayError::AcceptanceFailed(CHANGED, None));
    }
    if report.exit_code == Some(0) && !report.timed_out {
        return Ok(());
    }
    let details = json!({
        "exit_code": report.exit_code,
        "timed_out": report.timed_out,
        "output": last_bytes(&report.output, MAX_CHECK_OUTPUT_BYTES),
    });
    Err(GatewayError::AcceptanceFailed(
        FAILED,
        Some(Box::new(details)),
    ))
}
