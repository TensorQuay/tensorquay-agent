use super::*;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn flags(extra: &[&str]) -> Vec<String> {
    let base = [
        "--base-url",
        "http://127.0.0.1:8090",
        "--token-file",
        "/run/e2e/token",
        "--route",
        "local",
        "--tasks",
        "tasks",
        "--out",
        "out",
        "--storage-root",
        "/srv/storage",
        "--runtime-image",
        "tqa-runtime:base",
        "--gateway-image",
        "tqa-gateway:local",
    ];
    base.iter()
        .chain(extra)
        .map(|flag| (*flag).to_owned())
        .collect()
}
fn repository_tasks() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/e2e/tasks")
}

#[test]
fn tk_u_009_flags_parse_with_defaults_and_refuse_anything_unclear() -> TestResult {
    let options = Options::parse(flags(&[
        "--dry-run",
        "--only",
        "a",
        "--only",
        "b",
        "--native-tools",
        "false",
    ]))?;
    assert!(options.guards.dry_run && !options.route_facts.native_tools && !options.seed_check);
    assert_eq!(
        (options.repeat, options.margin_secs, options.only.len()),
        (1, 60, 2)
    );
    assert_eq!(options.repository, PathBuf::from("."));
    let refused = |extra: &[&str], needle: &str| {
        let error = Options::parse(flags(extra))
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(error.contains(needle), "{extra:?}: {error}");
    };
    refused(&["--color"], "unknown flag --color");
    refused(&["--repeat"], "--repeat needs a value");
    refused(&["--repeat", "0"], "--repeat must be at least 1");
    refused(&["--repeat", "x"], "--repeat needs a whole number");
    refused(
        &["--native-tools", "yes"],
        "--native-tools takes true or false",
    );
    let missing = Options::parse(["--tasks", "t", "--runtime-image", "r"].map(String::from)).err();
    assert!(
        missing
            .map(|error| error.to_string())
            .unwrap_or_default()
            .contains("--base-url is required")
    );
    let seeds =
        Options::parse(["--seed-check", "--tasks", "t", "--runtime-image", "r"].map(String::from))?;
    assert!(
        seeds.seed_check,
        "the seed check needs neither a stack nor a token"
    );
    Ok(())
}

#[test]
fn tk_u_009_paid_routes_need_permission_a_cap_and_confirmation() -> TestResult {
    let tasks = load_tasks(&repository_tasks(), &[])?;
    let estimate: u64 = tasks
        .iter()
        .map(|(_, manifest)| manifest.budgets.max_tokens)
        .sum();
    assert_eq!(
        Options::parse(flags(&["--dry-run"]))?.guard(&tasks)?,
        estimate
    );
    let paid = |extra: &[&str]| {
        Options::parse(flags(extra))
            .map_err(|error| error.to_string())
            .and_then(|options| options.guard(&tasks).map_err(|error| error.to_string()))
    };
    assert!(
        paid(&[])
            .err()
            .unwrap_or_default()
            .contains("--allow-paid-route and --max-total-tokens")
    );
    assert!(
        paid(&["--allow-paid-route"]).is_err(),
        "a paid route without a cap is refused"
    );
    let unconfirmed = paid(&["--allow-paid-route", "--max-total-tokens", "3000000"])
        .err()
        .unwrap_or_default();
    assert!(
        unconfirmed.contains(&format!("up to {} tokens", estimate.min(3_000_000)))
            && unconfirmed.contains("--yes"),
        "{unconfirmed}"
    );
    assert_eq!(
        paid(&["--allow-paid-route", "--max-total-tokens", "1000", "--yes"]),
        Ok(1000)
    );
    Ok(())
}

#[test]
fn tk_u_009_the_repository_task_set_loads_sorted_and_filters_by_id() -> TestResult {
    let tasks = load_tasks(&repository_tasks(), &[])?;
    let ids: Vec<&str> = tasks
        .iter()
        .map(|(_, manifest)| manifest.id.as_str())
        .collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(ids, sorted, "sorted by id, each id once");
    assert!(ids.len() >= 5 && ids.contains(&"py-fix-off-by-one"));
    assert!(
        tasks
            .iter()
            .all(|(directory, manifest)| directory.ends_with(&manifest.id))
    );
    for (_, manifest) in &tasks {
        let tagged = manifest.tags.iter().any(|tag| tag == "compaction");
        assert_eq!(
            manifest.expects_compaction, tagged,
            "{}: the compaction tag and flag agree",
            manifest.id
        );
    }
    // Tests run from the package root; a relative task root, as typed on the command line, must come back
    // absolute because Docker bind mounts refuse relative sources.
    let relative = load_tasks(Path::new("../../tests/e2e/tasks"), &[])?;
    assert!(
        relative
            .iter()
            .all(|(directory, _)| directory.is_absolute()),
        "bind mounts need absolute sources"
    );
    assert_eq!(
        load_tasks(&repository_tasks(), &["sh-build-fix".to_owned()])?.len(),
        1
    );
    let unknown = load_tasks(&repository_tasks(), &["missing".to_owned()])
        .err()
        .map(|error| error.to_string());
    assert_eq!(unknown.as_deref(), Some("no task named missing"));
    Ok(())
}

#[test]
fn tk_u_009_mount_sources_are_absolute_without_entering_the_storage() -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    let outer = tempfile::tempdir()?;
    let locked = outer.path().join("installation");
    std::fs::create_dir(&locked)?;
    // The runtime user owns the installation; the host user cannot enter it.
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))?;
    let storage = locked.join("storage");
    let resolved = mount_source(&storage);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700))?;
    assert_eq!(resolved?, storage);
    let relative = mount_source(Path::new("storage"))?;
    assert_eq!(relative, std::env::current_dir()?.join("storage"));
    Ok(())
}

#[test]
fn tk_u_009_each_task_runs_in_its_template_and_is_checked_in_its_image() -> TestResult {
    let tasks = load_tasks(&repository_tasks(), &["py-fix-off-by-one".to_owned()])?;
    let mut manifest = tasks.into_iter().next().ok_or("task")?.1;
    assert_eq!(template_for(&manifest, None), None, "the gateway default");
    assert_eq!(
        template_for(&manifest, Some("minimal")),
        Some("minimal"),
        "the run's template"
    );
    manifest.template = Some("workstation".into());
    assert_eq!(
        template_for(&manifest, Some("minimal")),
        Some("workstation"),
        "the task's own template wins"
    );
    let both = Images {
        runtime: "sha256:r".into(),
        workstation: Some("sha256:w".into()),
    };
    let runtime_only = Images {
        runtime: "sha256:r".into(),
        workstation: None,
    };
    assert_eq!(both.for_check(&manifest)?, "sha256:r");
    manifest.check.image = CheckImage::Workstation;
    assert_eq!(both.for_check(&manifest)?, "sha256:w");
    let refused = runtime_only
        .for_check(&manifest)
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(refused.contains("add --workstation-image"), "{refused}");
    let tasks = [(PathBuf::from("t"), manifest)];
    assert!(
        require_images(&tasks, &runtime_only).is_err() && require_images(&tasks, &both).is_ok()
    );
    let parsed = Options::parse(flags(&[
        "--dry-run",
        "--workstation-image",
        "tqa-runtime:workstation",
    ]))?;
    assert_eq!(
        parsed.workstation_image.as_deref(),
        Some("tqa-runtime:workstation")
    );
    Ok(())
}

#[test]
fn tk_u_009_the_task_set_keeps_its_gate_requirements() -> TestResult {
    // Design 085 §5 and the 085d brief: at least 5 fallback-relevant tasks (E2E-FALLBACK-004), exactly 10 in the
    // comparison subset (E2E-CMP-003) and at least 2 long tasks that expect compaction.
    let tasks = load_tasks(&repository_tasks(), &[])?;
    let tagged = |tag: &str| {
        tasks
            .iter()
            .filter(|(_, manifest)| manifest.tags.iter().any(|t| t == tag))
            .count()
    };
    assert!(
        tagged("fallback-relevant") >= 5,
        "fallback-relevant: {}",
        tagged("fallback-relevant")
    );
    assert_eq!(tagged("cmp"), 10);
    let long = tasks.iter().filter(|(_, manifest)| {
        manifest.expects_compaction && manifest.tags.iter().any(|t| t == "long")
    });
    assert!(long.count() >= 2);
    for language in ["python", "shell", "typescript"] {
        for kind in [
            &["bug-fix"][..],
            &["feature"],
            &["add-tests"],
            &["refactor"],
            &["docs", "config"],
        ] {
            let covered = tasks.iter().any(|(_, manifest)| {
                manifest.tags.iter().any(|t| t == language)
                    && manifest.tags.iter().any(|t| kind.contains(&t.as_str()))
            });
            assert!(covered, "{language} has no {kind:?} task");
        }
    }
    Ok(())
}

#[test]
fn tk_u_018_the_live_suite_runs_one_small_task() -> TestResult {
    // E2E-PROVIDER-014 spends real quota: its task set is the one smoke task, inside the suite's per-route cap.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/e2e/live/tasks");
    let tasks = load_tasks(&root, &[])?;
    let ids: Vec<&str> = tasks
        .iter()
        .map(|(_, manifest)| manifest.id.as_str())
        .collect();
    assert_eq!(ids, ["smoke-fix"]);
    let budgets = &tasks[0].1.budgets;
    assert!(
        budgets.max_tokens <= 30_000 && budgets.max_steps <= 8,
        "{budgets:?}"
    );
    for variant in ["solve", "wrong"] {
        assert!(
            tasks[0]
                .0
                .join("dry-run")
                .join(format!("{variant}.json"))
                .is_file()
        );
    }
    Ok(())
}

#[test]
fn tk_u_024_concurrency_defaults_to_one_and_is_bounded() -> TestResult {
    assert_eq!(
        Options::parse(flags(&[]))?.concurrency,
        1,
        "one at a time unless asked"
    );
    assert_eq!(
        Options::parse(flags(&["--concurrency", "4"]))?.concurrency,
        4
    );
    assert_eq!(
        Options::parse(flags(&["--concurrency", "16"]))?.concurrency,
        16
    );
    for bad in ["0", "17", "four", "-1"] {
        assert!(
            Options::parse(flags(&["--concurrency", bad])).is_err(),
            "--concurrency {bad} accepted"
        );
    }
    Ok(())
}

// TEST: TK-U-027
#[test]
fn tk_u_027_an_attempts_workspace_is_deleted_unless_kept() -> TestResult {
    assert!(
        !Options::parse(flags(&[]))?.keep_workspaces,
        "deleted by default"
    );
    assert!(Options::parse(flags(&["--keep-workspaces"]))?.keep_workspaces);
    Ok(())
}

// TEST: TK-U-027
#[test]
fn tk_u_027_the_cleanup_wait_is_bounded_and_defaults_past_the_idle_window() -> TestResult {
    assert_eq!(Options::parse(flags(&[]))?.cleanup_wait_secs, 900);
    let options = Options::parse(flags(&["--cleanup-wait-secs", "0"]))?;
    assert_eq!(options.cleanup_wait_secs, 0);
    assert!(Options::parse(flags(&["--cleanup-wait-secs", "3601"])).is_err());
    Ok(())
}

// TEST: TK-U-032
#[test]
fn tk_u_032_the_engine_arm_needs_its_profile_and_a_call_cap_and_a_wall_cap_is_bounded() -> TestResult
{
    let profile = "0195d8c0-0000-7000-8000-00000000000a";
    let options = Options::parse(flags(&[
        "--engine-profile",
        profile,
        "--max-engine-calls",
        "3000",
        "--max-wall-minutes",
        "35",
    ]))?;
    assert_eq!(options.engine_profile.as_deref(), Some(profile));
    assert_eq!(options.max_engine_calls, Some(3000));
    assert_eq!(options.max_wall_minutes, Some(35));
    // Without the engine flags the run is exactly today's.
    let plain = Options::parse(flags(&[]))?;
    assert_eq!(
        (
            plain.engine_profile,
            plain.max_engine_calls,
            plain.max_wall_minutes
        ),
        (None, None, None)
    );
    for (extra, expected) in [
        (
            &["--engine-profile", profile][..],
            "--engine-profile needs --max-engine-calls",
        ),
        (
            &["--max-engine-calls", "10"][..],
            "--max-engine-calls needs --engine-profile",
        ),
        (
            &["--engine-profile", "router", "--max-engine-calls", "10"][..],
            "--engine-profile takes a profile id",
        ),
        (
            &["--engine-profile", profile, "--max-engine-calls", "0"][..],
            "--max-engine-calls must be at least 1",
        ),
        (
            &["--max-wall-minutes", "0"][..],
            "--max-wall-minutes is 1 to 1440",
        ),
        (
            &["--max-wall-minutes", "1441"][..],
            "--max-wall-minutes is 1 to 1440",
        ),
    ] {
        let refused = Options::parse(flags(extra))
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(refused.contains(expected), "{extra:?}: {refused}");
    }
    Ok(())
}

// TEST: TK-U-009
/// `--row` names the test row the summary's verdict is for; E2E-WAVE-001 when not given, as before.
#[test]
fn tk_u_009_the_row_flag_names_the_verdict_and_refuses_anything_unclear() -> TestResult {
    assert_eq!(Options::parse(flags(&[]))?.row, "E2E-WAVE-001");
    for row in ["E2E-FALLBACK-004", "E2E-WAVE-002", "phase demo (local-32k)"] {
        assert_eq!(Options::parse(flags(&["--row", row]))?.row, row);
    }
    for row in [
        "",
        " lead",
        "-E2E",
        "E2E*",
        "a\nb",
        "$(id)",
        &"x".repeat(49),
    ] {
        assert!(
            Options::parse(flags(&["--row", row])).is_err(),
            "{row:?} must be refused"
        );
    }
    Ok(())
}
