use super::*;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const VALID: &str = r#"
id = "py-fix"
title = "Fix the off-by-one"
tags = ["python"]
budgets = { max_steps = 30, wall_clock_secs = 600, max_tokens = 200000 }
protected = ["tests/test_core.py"]
check = { image = "runtime", timeout_secs = 120 }
"#;

/// A complete task directory named `py-fix` whose manifest is `manifest`.
fn task(manifest: &str) -> Result<(tempfile::TempDir, std::path::PathBuf), std::io::Error> {
    let root = tempfile::tempdir()?;
    let directory = root.path().join("py-fix");
    std::fs::create_dir_all(directory.join("seed"))?;
    std::fs::create_dir_all(directory.join("check"))?;
    std::fs::write(directory.join("task.toml"), manifest)?;
    std::fs::write(directory.join("task.md"), "Fix it.\n")?;
    std::fs::write(directory.join("check/run.sh"), "#!/bin/sh\n")?;
    Ok((root, directory))
}

#[test]
fn tk_u_002_valid_task_loads() -> TestResult {
    let (_root, directory) = task(VALID)?;
    let manifest = Manifest::load(&directory)?;
    assert_eq!(manifest.id, "py-fix");
    assert_eq!(manifest.check.image, CheckImage::Runtime);
    assert_eq!(manifest.budgets.max_tokens, 200_000);
    assert!(!manifest.expects_compaction, "absent means no expectation");
    let (_root, directory) = task(&format!("{VALID}expects_compaction = true\n"))?;
    assert!(Manifest::load(&directory)?.expects_compaction);
    let (_root, directory) = task(&format!("{VALID}template = \"workstation\"\n"))?;
    assert_eq!(
        Manifest::load(&directory)?.template.as_deref(),
        Some("workstation")
    );
    for bad in ["Work Station", ""] {
        let (_root, directory) = task(&format!("{VALID}template = \"{bad}\"\n"))?;
        assert_eq!(
            Manifest::load(&directory),
            Err(ManifestError::Invalid("template")),
            "{bad:?}"
        );
    }
    Ok(())
}

#[test]
fn tk_u_002_out_of_bound_fields_are_named() -> TestResult {
    for (from, to, field) in [
        ("id = \"py-fix\"", "id = \"other\"", "id"),
        ("id = \"py-fix\"", "id = \"Py-Fix\"", "id"),
        ("Fix the off-by-one", "", "title"),
        ("max_steps = 30", "max_steps = 501", "budgets.max_steps"),
        ("max_steps = 30", "max_steps = 0", "budgets.max_steps"),
        (
            "wall_clock_secs = 600",
            "wall_clock_secs = 86401",
            "budgets.wall_clock_secs",
        ),
        (
            "max_tokens = 200000",
            "max_tokens = 0",
            "budgets.max_tokens",
        ),
        (
            "timeout_secs = 120",
            "timeout_secs = 3601",
            "check.timeout_secs",
        ),
        ("tests/test_core.py", "../outside", "protected"),
        ("tests/test_core.py", "/etc/passwd", "protected"),
    ] {
        let (_root, directory) = task(&VALID.replace(from, to))?;
        assert_eq!(
            Manifest::load(&directory),
            Err(ManifestError::Invalid(field)),
            "{to}"
        );
    }
    Ok(())
}

#[test]
fn tk_u_002_schema_errors_and_missing_parts_are_refused() -> TestResult {
    for manifest in [
        format!("{VALID}\nextra = 1\n"),
        VALID.replace("image = \"runtime\"", "image = \"host\""),
    ] {
        let (_root, directory) = task(&manifest)?;
        assert!(matches!(
            Manifest::load(&directory),
            Err(ManifestError::Schema(_))
        ));
    }
    for (remove, missing) in [
        ("task.md", "task.md"),
        ("seed", "seed/"),
        ("check/run.sh", "check/run.sh"),
    ] {
        let (_root, directory) = task(VALID)?;
        let path = directory.join(remove);
        if path.is_dir() {
            std::fs::remove_dir(&path)?;
        } else {
            std::fs::remove_file(&path)?;
        }
        assert_eq!(
            Manifest::load(&directory),
            Err(ManifestError::Missing(missing))
        );
    }
    let (_root, directory) = task(&format!("{VALID}#{}\n", "x".repeat(70_000)))?;
    assert_eq!(Manifest::load(&directory), Err(ManifestError::Unreadable));
    Ok(())
}

#[test]
fn tk_u_002_follow_ups_load_in_order_and_are_bounded() -> TestResult {
    let (_root, directory) = task(VALID)?;
    assert!(
        load_followups(&directory)?.is_empty(),
        "no follow-ups without the directory"
    );
    std::fs::create_dir(directory.join("followups"))?;
    std::fs::write(directory.join("followups/10.md"), "tenth\n")?;
    std::fs::write(directory.join("followups/02.md"), "second\n")?;
    assert_eq!(load_followups(&directory)?, ["second\n", "tenth\n"]);
    for (name, content) in [
        ("03.txt", "not markdown"),
        ("04.md", "  \n"),
        ("05.md", &"x".repeat(17 * 1024)[..]),
    ] {
        std::fs::write(directory.join("followups").join(name), content)?;
        assert_eq!(
            load_followups(&directory),
            Err(ManifestError::Invalid("followups")),
            "{name}"
        );
        std::fs::remove_file(directory.join("followups").join(name))?;
    }
    Ok(())
}
