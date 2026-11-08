//! Runs the E2E task set against a stack through its public API (TEST-PLAN §16).
//! Exit status: 0 when the run was recorded, 1 when it could not be, 2 for unusable flags or tasks.
//! `--check-only` runs one task's check on a workspace another tool left (E2E-CMP-003); it exits 0 once the verdict is
//! recorded, whatever the verdict, and 1 or 2 as above. `--seed-only` seeds an empty workspace as an attempt's is seeded.
use std::process::ExitCode;
use tqa_testkit::e2e::{
    check_only::{self, CheckOnly, SeedOnly},
    classify::CheckOutcome,
    driver::{self, DriverError, Options},
};

fn main() -> ExitCode {
    let flags: Vec<String> = std::env::args().skip(1).collect();
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => return fail(&DriverError::Io(error)),
    };
    match runtime.block_on(start(flags)) {
        Ok(code) => code,
        Err(error) => fail(&error),
    }
}

async fn start(flags: Vec<String>) -> Result<ExitCode, DriverError> {
    if flags.first().map(String::as_str) == Some("--seed-only") {
        let options = SeedOnly::parse(flags.into_iter().skip(1))?;
        check_only::seed_only(&options).await?;
        write("seeded", &options.workspace.display().to_string());
        return Ok(ExitCode::SUCCESS);
    }
    if flags.first().map(String::as_str) == Some("--check-only") {
        let options = CheckOnly::parse(flags.into_iter().skip(1))?;
        let verdict = check_only::run(&options).await?;
        write("check", &format!("{:?}", verdict.outcome).to_lowercase());
        if let Some(note) = verdict.note {
            write("note", &note);
        }
        write(
            "record",
            &options.out.join("check.json").display().to_string(),
        );
        return Ok(ExitCode::SUCCESS);
    }
    let options = Options::parse(flags.clone())?;
    let tasks = driver::load_tasks(&options.tasks, &options.only)?;
    let images = driver::Images {
        runtime: options.runtime_image.clone(),
        workstation: options.workstation_image.clone(),
    };
    driver::require_images(&tasks, &images)?;
    if options.seed_check {
        let scratch = tempfile::tempdir()?;
        let outcomes = driver::seed_check(&tasks, &images, scratch.path()).await?;
        let mut valid = true;
        for (task, outcome) in outcomes {
            let good = outcome == CheckOutcome::Failed;
            valid &= good;
            write(
                &format!(
                    "{} seed check: {outcome:?}",
                    if good { "ok " } else { "BAD" }
                ),
                &task,
            );
        }
        return Ok(if valid {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        });
    }
    let bound = options.guard(&tasks)?;
    write(
        "tasks",
        &format!("{}; spending bound {bound} tokens", tasks.len()),
    );
    let summary = driver::run(&options, &tasks, flags).await?;
    write(
        "summary",
        &options.out.join("summary.md").display().to_string(),
    );
    write(
        "verdict",
        summary
            .lines()
            .find(|line| line.starts_with("**E2E-WAVE-001"))
            .unwrap_or_default(),
    );
    Ok(ExitCode::SUCCESS)
}

#[allow(
    clippy::print_stdout,
    reason = "the driver reports its progress on standard output"
)]
fn write(label: &str, text: &str) {
    println!("{label}: {text}");
}

#[allow(
    clippy::print_stderr,
    reason = "the driver reports why it stopped on standard error"
)]
fn fail(error: &DriverError) -> ExitCode {
    eprintln!("tqa-e2e: {error}");
    ExitCode::from(if matches!(error, DriverError::Usage(_)) {
        2
    } else {
        1
    })
}
