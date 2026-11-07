//! TK-U-022: `tqa-e2e --check-only` runs one task's check on a workspace another tool left (E2E-CMP-003), the same
//! way an attempt's check runs: its flags, the seed's protected paths (a changed one is `tampered`, before any
//! container), and the record it writes. The workspace is another program's output, so the copy drops symbolic links
//! (as the files API's download never makes one), and the host-side helpers never follow one: a link at or above a
//! protected path is `tampered`, `share` leaves links alone and `restore` refuses one.
//! TK-I-005 (Docker, ignored by default): passed, failed and broken checks, and the broken check's harness line.
use std::{error::Error, fs, path::Path};
use tqa_testkit::e2e::{
    check_only::{self, CheckOnly},
    classify::CheckOutcome,
};

type TestResult = Result<(), Box<dyn Error>>;

fn tasks() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/e2e/tasks")
}

fn flags(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &to.join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), to.join(entry.file_name()))?;
        }
    }
    Ok(())
}

#[test]
fn tk_u_022_flags_are_exact() -> TestResult {
    let parsed = CheckOnly::parse(flags(&[
        "--tasks",
        "t",
        "--task",
        "py-fix-off-by-one",
        "--workspace",
        "w",
        "--out",
        "o",
        "--runtime-image",
        "r",
    ]))?;
    assert_eq!(
        (parsed.task.as_str(), parsed.workstation_image.as_deref()),
        ("py-fix-off-by-one", None)
    );
    for bad in [
        flags(&[
            "--tasks",
            "t",
            "--task",
            "x",
            "--workspace",
            "w",
            "--out",
            "o",
        ]),
        flags(&[
            "--tasks",
            "t",
            "--task",
            "x",
            "--workspace",
            "w",
            "--out",
            "o",
            "--runtime-image",
            "r",
            "--route",
            "l",
        ]),
        flags(&["--tasks", "t", "--task", "x", "--workspace", "w", "--out"]),
    ] {
        assert!(CheckOnly::parse(bad.clone()).is_err(), "{bad:?}");
    }
    Ok(())
}

#[test]
fn tk_u_022_the_copy_drops_links_and_counts_them() -> TestResult {
    let scratch = tempfile::tempdir()?;
    let outside = scratch.path().join("outside.txt");
    fs::write(&outside, "host content")?;
    let workspace = scratch.path().join("workspace");
    fs::create_dir_all(workspace.join("src"))?;
    fs::write(workspace.join("src/app.py"), "print(1)\n")?;
    std::os::unix::fs::symlink(&outside, workspace.join("leak.txt"))?;
    std::os::unix::fs::symlink(scratch.path(), workspace.join("src/up"))?;
    let copy = scratch.path().join("copy");
    let dropped = check_only::copy_workspace(&workspace, &copy)?;
    assert_eq!(dropped, 2);
    assert_eq!(fs::read_to_string(copy.join("src/app.py"))?, "print(1)\n");
    for name in ["leak.txt", "src/up"] {
        assert!(
            fs::symlink_metadata(copy.join(name)).is_err(),
            "{name} was copied"
        );
    }
    Ok(())
}

/// A file outside the copy, owner-only, as a host secret would be.
fn secret(directory: &Path, content: &str) -> std::io::Result<std::path::PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let path = directory.join("secret");
    fs::write(&path, content)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    Ok(path)
}

fn mode(path: &Path) -> std::io::Result<u32> {
    use std::os::unix::fs::PermissionsExt;
    Ok(fs::symlink_metadata(path)?.permissions().mode() & 0o7777)
}

#[test]
fn tk_u_022_the_host_helpers_never_follow_a_link() -> TestResult {
    use tqa_testkit::e2e::workspace;
    let scratch = tempfile::tempdir()?;
    let seed = tasks().join("py-fix-off-by-one/seed");
    let protected = vec!["tests/test_pagination.py".to_owned()];
    let pristine = fs::read_to_string(seed.join("tests/test_pagination.py"))?;
    let host = scratch.path().join("host");
    fs::create_dir_all(&host)?;
    // A host secret with the same bytes as the protected file, so only the link gives the swap away.
    let secret = secret(&host, &pristine)?;
    let host_mode = mode(&host)?;

    // A copy whose protected file is a link to the secret, beside links to the secret and to its directory.
    let copy = scratch.path().join("copy");
    copy_dir(&seed, &copy)?;
    fs::remove_file(copy.join("tests/test_pagination.py"))?;
    std::os::unix::fs::symlink(&secret, copy.join("tests/test_pagination.py"))?;
    std::os::unix::fs::symlink(&secret, copy.join("leak"))?;
    std::os::unix::fs::symlink(&host, copy.join("leak-dir"))?;
    workspace::share(&copy)?;
    assert_eq!(
        (mode(&secret)?, mode(&host)?),
        (0o600, host_mode),
        "share followed a link"
    );
    assert!(
        workspace::tampered(&seed, &copy, &protected)?,
        "a linked protected file"
    );
    assert!(
        workspace::restore(&seed, &copy, &protected).is_err(),
        "restore wrote through a link"
    );
    assert_eq!(fs::read_to_string(&secret)?, pristine);

    // A copy whose protected file sits under a linked directory.
    let linked_parent = scratch.path().join("linked-parent");
    copy_dir(&seed, &linked_parent)?;
    fs::remove_dir_all(linked_parent.join("tests"))?;
    let elsewhere = scratch.path().join("elsewhere");
    copy_dir(&seed.join("tests"), &elsewhere)?;
    std::os::unix::fs::symlink(&elsewhere, linked_parent.join("tests"))?;
    assert!(
        workspace::tampered(&seed, &linked_parent, &protected)?,
        "a linked parent"
    );
    assert!(workspace::restore(&seed, &linked_parent, &protected).is_err());
    Ok(())
}

#[tokio::test]
async fn tk_u_022_a_workspace_of_links_leaves_the_host_untouched() -> TestResult {
    let scratch = tempfile::tempdir()?;
    let seed = tasks().join("py-fix-off-by-one/seed");
    let pristine = fs::read_to_string(seed.join("tests/test_pagination.py"))?;
    let host = scratch.path().join("host");
    fs::create_dir_all(&host)?;
    let secret = secret(&host, &pristine)?;
    let workspace = scratch.path().join("workspace");
    copy_dir(&seed, &workspace)?;
    fs::remove_file(workspace.join("tests/test_pagination.py"))?;
    std::os::unix::fs::symlink(&secret, workspace.join("tests/test_pagination.py"))?;
    std::os::unix::fs::symlink(&host, workspace.join("leak-dir"))?;
    let out = scratch.path().join("record");
    let options = CheckOnly::parse(vec![
        "--tasks".into(),
        tasks().display().to_string(),
        "--task".into(),
        "py-fix-off-by-one".into(),
        "--workspace".into(),
        workspace.display().to_string(),
        "--out".into(),
        out.display().to_string(),
        "--runtime-image".into(),
        "unused-image".into(),
    ])?;
    // The linked protected file is dropped, so the copy lacks it: tampered, before any container.
    let verdict = check_only::run(&options).await?;
    assert_eq!(verdict.outcome, CheckOutcome::Tampered);
    let record: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(out.join("check.json"))?)?;
    assert_eq!(record["dropped"], 2);
    assert_eq!(
        (fs::read_to_string(&secret)?, mode(&secret)?),
        (pristine, 0o600)
    );
    Ok(())
}

#[tokio::test]
async fn tk_u_022_a_changed_protected_path_is_tampered_before_any_container() -> TestResult {
    let scratch = tempfile::tempdir()?;
    let workspace = scratch.path().join("workspace");
    copy_dir(&tasks().join("py-fix-off-by-one/seed"), &workspace)?;
    fs::write(
        workspace.join("tests/test_pagination.py"),
        "def test_nothing():\n    pass\n",
    )?;
    let out = scratch.path().join("record");
    let options = CheckOnly::parse(vec![
        "--tasks".into(),
        tasks().display().to_string(),
        "--task".into(),
        "py-fix-off-by-one".into(),
        "--workspace".into(),
        workspace.display().to_string(),
        "--out".into(),
        out.display().to_string(),
        // No container runs for a tampered copy, so the image is never used.
        "--runtime-image".into(),
        "unused-image".into(),
    ])?;
    let verdict = check_only::run(&options).await?;
    assert_eq!(verdict.outcome, CheckOutcome::Tampered);
    let record: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(out.join("check.json"))?)?;
    assert_eq!(record["task"], "py-fix-off-by-one");
    assert_eq!(record["outcome"], "tampered");
    // The workspace it was given is left as it was.
    assert_eq!(
        fs::read_to_string(workspace.join("tests/test_pagination.py"))?,
        "def test_nothing():\n    pass\n"
    );
    // A second run never overwrites a record.
    assert!(check_only::run(&options).await.is_err());
    Ok(())
}

/// A task whose check exits with `code`, in a temporary task set.
fn synthetic_task(root: &Path, id: &str, code: i32) -> std::io::Result<()> {
    let directory = root.join(id);
    fs::create_dir_all(directory.join("seed"))?;
    fs::create_dir_all(directory.join("check"))?;
    fs::write(directory.join("seed/README.md"), "seed\n")?;
    fs::write(directory.join("task.md"), "Do nothing.\n")?;
    fs::write(
        directory.join("check/run.sh"),
        format!("echo checked\nexit {code}\n"),
    )?;
    fs::write(
        directory.join("task.toml"),
        format!(
            "id = \"{id}\"\ntitle = \"Synthetic check {code}\"\ntags = [\"synthetic\"]\n\
             budgets = {{ max_steps = 5, wall_clock_secs = 60, max_tokens = 1000 }}\n\
             protected = []\ncheck = {{ image = \"runtime\", timeout_secs = 60 }}\n"
        ),
    )
}

#[tokio::test]
#[ignore = "needs Docker and TQA_E2E_RUNTIME_IMAGE"]
async fn tk_i_005_passed_failed_and_broken_checks_are_recorded() -> TestResult {
    let Ok(image) = std::env::var("TQA_E2E_RUNTIME_IMAGE") else {
        return Err("set TQA_E2E_RUNTIME_IMAGE to the runtime image".into());
    };
    let scratch = tempfile::tempdir()?;
    let root = scratch.path().join("tasks");
    for (id, code) in [("check-pass", 0), ("check-fail", 1), ("check-broken", 3)] {
        synthetic_task(&root, id, code)?;
    }
    for (id, outcome) in [
        ("check-pass", CheckOutcome::Passed),
        ("check-fail", CheckOutcome::Failed),
        ("check-broken", CheckOutcome::Broken),
    ] {
        let out = scratch.path().join(format!("{id}-record"));
        let options = CheckOnly::parse(vec![
            "--tasks".into(),
            root.display().to_string(),
            "--task".into(),
            id.into(),
            "--workspace".into(),
            root.join(id).join("seed").display().to_string(),
            "--out".into(),
            out.display().to_string(),
            "--runtime-image".into(),
            image.clone(),
        ])?;
        let verdict = check_only::run(&options).await?;
        assert_eq!(verdict.outcome, outcome, "{id}");
        let saved = fs::read_to_string(out.join("check.txt"))?;
        assert!(saved.starts_with("checked\n"), "{id}: {saved}");
        let broken = outcome == CheckOutcome::Broken;
        assert_eq!(saved.contains("{\"harness\":"), broken, "{id}: {saved}");
        assert_eq!(verdict.note.is_some(), broken, "{id}");
        if broken {
            assert!(
                verdict
                    .note
                    .as_deref()
                    .is_some_and(|note| note.contains("docker exited 3"))
            );
        }
    }
    Ok(())
}

// TEST: TK-U-023
#[test]
fn tk_u_023_seed_only_flags_are_exact() -> TestResult {
    let parsed = check_only::SeedOnly::parse(flags(&[
        "--tasks",
        "t",
        "--task",
        "py-fix-off-by-one",
        "--workspace",
        "w",
        "--runtime-image",
        "r",
    ]))?;
    assert_eq!(parsed.task, "py-fix-off-by-one");
    for bad in [
        flags(&["--tasks", "t", "--task", "x", "--workspace", "w"]),
        flags(&[
            "--tasks",
            "t",
            "--task",
            "x",
            "--workspace",
            "w",
            "--runtime-image",
            "r",
            "--out",
            "o",
        ]),
    ] {
        assert!(check_only::SeedOnly::parse(bad.clone()).is_err(), "{bad:?}");
    }
    Ok(())
}

// TEST: TK-U-023
#[tokio::test]
async fn tk_u_023_seed_only_refuses_a_workspace_that_is_not_empty() -> TestResult {
    let scratch = tempfile::tempdir()?;
    let workspace = scratch.path().join("workspace");
    fs::create_dir_all(&workspace)?;
    fs::write(workspace.join("left.txt"), "left over")?;
    let options = check_only::SeedOnly::parse(vec![
        "--tasks".into(),
        tasks().display().to_string(),
        "--task".into(),
        "py-fix-off-by-one".into(),
        "--workspace".into(),
        workspace.display().to_string(),
        "--runtime-image".into(),
        "unused-image".into(),
    ])?;
    let refused = check_only::seed_only(&options).await;
    assert!(refused.is_err());
    assert_eq!(fs::read_to_string(workspace.join("left.txt"))?, "left over");
    Ok(())
}

// TEST: TK-I-005
#[tokio::test]
#[ignore = "needs Docker and TQA_E2E_RUNTIME_IMAGE"]
async fn tk_i_005_seed_only_makes_the_drivers_baseline() -> TestResult {
    let Ok(image) = std::env::var("TQA_E2E_RUNTIME_IMAGE") else {
        return Err("set TQA_E2E_RUNTIME_IMAGE to the runtime image".into());
    };
    let scratch = tempfile::tempdir()?;
    let mut heads = Vec::new();
    for name in ["first", "second"] {
        let workspace = scratch.path().join(name);
        let options = check_only::SeedOnly::parse(vec![
            "--tasks".into(),
            tasks().display().to_string(),
            "--task".into(),
            "py-fix-off-by-one".into(),
            "--workspace".into(),
            workspace.display().to_string(),
            "--runtime-image".into(),
            image.clone(),
        ])?;
        check_only::seed_only(&options).await?;
        heads.push(check_only::seed_commit(&workspace)?);
        assert!(workspace.join("pagination.py").is_file());
    }
    // The fixed author, date and tree give every seeding the same baseline commit.
    assert_eq!(heads[0], heads[1]);
    assert_eq!(heads[0].len(), 40);
    Ok(())
}
