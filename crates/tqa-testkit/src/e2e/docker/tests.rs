use super::*;

fn joined(args: &[OsString]) -> String {
    args.iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn tk_u_007_the_check_runs_isolated_as_the_workspace_owner() {
    let args = joined(&check_args(
        "c-1",
        "sha256:img",
        Path::new("/t/check"),
        Path::new("/tmp/copy"),
    ));
    for required in [
        "create --network none --user 1000:1000",
        "--name c-1",
        "--cpus 2 --memory 2g",
        "type=bind,source=/t/check,target=/check,readonly",
        "type=bind,source=/tmp/copy,target=/workspace ",
        "--entrypoint sh sha256:img /check/run.sh",
    ] {
        assert!(args.contains(required), "missing {required} in {args}");
    }
}

#[test]
fn tk_u_007_the_seed_is_copied_offline_committed_reproducibly_and_handed_to_the_owner() {
    let args = seed_args(
        "sha256:img",
        Path::new("/t/seed"),
        Path::new("/s/workspaces/w"),
    );
    let text = joined(&args);
    for required in [
        "run --rm --network none --user 0:0",
        "type=bind,source=/t/seed,target=/seed,readonly",
        "type=bind,source=/s/workspaces/w,target=/workspace ",
        "GIT_AUTHOR_DATE=2026-01-01T00:00:00Z",
        "GIT_COMMITTER_DATE=2026-01-01T00:00:00Z",
    ] {
        assert!(text.contains(required), "missing {required} in {text}");
    }
    let script = args
        .last()
        .map(|arg| arg.to_string_lossy().into_owned())
        .unwrap_or_default();
    assert!(
        script.starts_with("set -eu; cp -a /seed/. /workspace/; chmod 0700 /workspace;")
            && script.ends_with("chown -R 1000:1000 /workspace"),
        "{script}"
    );
}

#[test]
fn tk_u_007_a_checked_copy_is_handed_back_to_the_host_user_offline() {
    let text = joined(&reclaim_args("sha256:img", Path::new("/tmp/copy")));
    assert!(
        text.starts_with("run --rm --network none --user 0:0"),
        "{text}"
    );
    assert!(
        text.contains("type=bind,source=/tmp/copy,target=/workspace "),
        "{text}"
    );
    assert!(
        text.ends_with("--entrypoint sh sha256:img -c chown -R 0:0 /workspace"),
        "{text}"
    );
}

#[test]
fn tk_u_007_a_broken_check_names_its_cause() {
    use super::{CheckOutcome, CheckRun};
    use std::time::Duration;
    let bound = Duration::from_mins(1);
    let run = |outcome, exit, timed_out, millis| CheckRun {
        outcome,
        output: String::new(),
        exit,
        timed_out,
        elapsed: Duration::from_millis(millis),
        bound,
    };
    // A pass or a fail needs no note.
    assert_eq!(
        run(CheckOutcome::Passed, Some(0), false, 1200).broken_note(),
        None
    );
    assert_eq!(
        run(CheckOutcome::Failed, Some(1), false, 1200).broken_note(),
        None
    );
    for (case, expected) in [
        (
            run(CheckOutcome::Broken, Some(125), false, 3210),
            "check broken: docker exited 125 after 3.2 s (bound 60 s)",
        ),
        (
            run(CheckOutcome::Broken, None, true, 60_000),
            "check broken: timed out after 60.0 s (bound 60 s)",
        ),
        (
            run(CheckOutcome::Broken, None, false, 40),
            "check broken: no exit status (the container did not start or was killed) after 0.0 s (bound 60 s)",
        ),
    ] {
        assert_eq!(case.broken_note().as_deref(), Some(expected));
        let line: serde_json::Value =
            serde_json::from_str(&case.harness_line().unwrap_or_default()).unwrap_or_default();
        assert_eq!(line["harness"]["timed_out"], case.timed_out, "{line}");
        assert_eq!(
            line["harness"]["exit"],
            serde_json::json!(case.exit),
            "{line}"
        );
        assert_eq!(line["harness"]["bound_ms"], 60_000, "{line}");
    }
}

/// TK-I-006 counts the harness's own containers by this label, so every seed, check and hand-back container carries it.
#[test]
fn tk_u_024_every_harness_container_carries_the_harness_label() {
    let copy = Path::new("/tmp/copy");
    for args in [
        check_args("c-1", "sha256:img", Path::new("/t/check"), copy),
        seed_args("sha256:img", Path::new("/t/seed"), copy),
        reclaim_args("sha256:img", copy),
    ] {
        let args = joined(&args);
        assert!(args.contains("--label tqa.e2e.container=1"), "{args}");
    }
}

/// A Docker client standing in for a loaded engine: the check's container prints its verdict and stops at once, but
/// removing a container takes 3 s (as `run --rm` makes the client wait for).
fn slow_removal_docker(directory: &Path) -> std::io::Result<std::path::PathBuf> {
    let path = directory.join("docker");
    std::fs::write(
        &path,
        "#!/bin/sh\n\
         case \"$1\" in\n\
         run) echo '{\"passed\": true}'; sleep 3; exit 0 ;;\n\
         create) echo created; exit 0 ;;\n\
         start) echo '{\"passed\": true}'; exit 0 ;;\n\
         rm) sleep 3; exit 0 ;;\n\
         *) exit 2 ;;\n\
         esac\n",
    )?;
    std::os::unix::fs::PermissionsExt::set_mode(
        &mut std::fs::metadata(&path)?.permissions(),
        0o755,
    );
    std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755))?;
    Ok(path)
}

// TEST: TK-U-025
#[tokio::test]
async fn tk_u_025_a_check_that_stopped_is_judged_without_waiting_for_its_removal()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let docker = slow_removal_docker(directory.path())?;
    let run = check_with(
        docker.as_os_str(),
        "sha256:img",
        (Path::new("/t/check"), Path::new("/t/copy")),
        Duration::from_secs(2),
    )
    .await;
    assert!(
        !run.timed_out,
        "the verdict came in time; only the removal was slow: {run:?}"
    );
    assert_eq!(run.outcome, CheckOutcome::Passed);
    assert!(run.output.contains("\"passed\": true"));
    assert!(run.elapsed < Duration::from_secs(2), "{:?}", run.elapsed);
    Ok(())
}
