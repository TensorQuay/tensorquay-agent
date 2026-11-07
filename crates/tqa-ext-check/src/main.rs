//! The `ext-check` entry point; `tools/ext-check` runs it.
use std::{io::Write, process::ExitCode};

use tqa_ext_check::{
    catalog,
    cli::{self, Invocation, USAGE},
    report::{EXIT_ENVIRONMENT, EXIT_FAILED, EXIT_USAGE},
    run::{Provenance, RunError, run_decision, run_stdio, run_template},
};

fn main() -> ExitCode {
    match cli::parse(std::env::args().skip(1).collect()) {
        Ok(Invocation::Help) => {
            let _ = writeln!(std::io::stdout(), "{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Invocation::McpStdio(args)) => finish(run_stdio(&args, &provenance())),
        Ok(Invocation::Template(args)) => finish(run_template(&args, &provenance())),
        Ok(Invocation::CatalogLint(dir)) => lint(&dir),
        Ok(Invocation::Decision(args)) => finish(run_decision(&args, &provenance())),
        Ok(_) => usage("unsupported invocation"),
        Err(refusal) => usage(&refusal.to_string()),
    }
}

/// Prints each problem, then a count; a catalogue that can't be read counts as one problem.
fn lint(dir: &std::path::Path) -> ExitCode {
    let problems = catalog::lint(dir).unwrap_or_else(|_| {
        vec![catalog::Problem {
            file: ".".into(),
            field: "directory".into(),
            message: "the catalogue can't be read".into(),
        }]
    });
    let mut out = std::io::stdout().lock();
    for problem in &problems {
        let _ = writeln!(out, "{problem}");
    }
    let plural = if problems.len() == 1 { "" } else { "s" };
    let _ = writeln!(out, "ext-check catalog: {} problem{plural}", problems.len());
    if problems.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(EXIT_FAILED)
    }
}

/// Prints the report and exits with its status, or reports why there is none.
fn finish(result: Result<tqa_ext_check::report::Report, RunError>) -> ExitCode {
    match result {
        Ok(report) => {
            let _ = write!(std::io::stdout(), "{}", report.text());
            ExitCode::from(report.exit_code())
        }
        Err(RunError::Usage(message)) => usage(&message),
        Err(RunError::Environment(message)) => {
            let _ = writeln!(std::io::stderr(), "ext-check: {message}");
            ExitCode::from(EXIT_ENVIRONMENT)
        }
    }
}

fn usage(message: &str) -> ExitCode {
    let _ = writeln!(std::io::stderr(), "ext-check: {message}\n{USAGE}");
    ExitCode::from(EXIT_USAGE)
}

/// The commit `tools/ext-check` passes (hex, optionally `-dirty`), and the time now.
fn provenance() -> Provenance {
    let commit = std::env::var("TQA_EXT_CHECK_COMMIT")
        .ok()
        .filter(|commit| {
            let hash = commit.strip_suffix("-dirty").unwrap_or(commit);
            (7..=40).contains(&hash.len()) && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
        .unwrap_or_else(|| "unknown".to_owned());
    let date = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();
    Provenance { commit, date }
}
