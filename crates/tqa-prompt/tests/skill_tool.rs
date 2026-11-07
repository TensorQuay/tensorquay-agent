//! TOOL-U-024: the `skill` tool loads a skill of the run's index on demand: its body and helper paths, bounded,
//! read-only, refusing unknown names and folders that changed after discovery.
use serde_json::json;
use std::{error::Error, fs, future::Future, path::Path, pin::Pin, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use tqa_prompt::{MAX_LISTED_SKILL_FILES, SkillTier, SkillTool, discover_skills};
use tqa_tools::{Tool, ToolContext, ToolError, ToolLimits, ToolRegistry, WorkspaceRoot};
use tqa_types::Sleeper;

type TestResult = Result<(), Box<dyn Error>>;

fn context(workspace: &Path) -> Result<ToolContext, Box<dyn Error>> {
    let clock = tqa_testkit::FakeClock::new();
    Ok(ToolContext {
        workspace: WorkspaceRoot::open(workspace)?,
        cancel: CancellationToken::new(),
        clock: Arc::new(clock.clone()),
        sleeper: Arc::new(tqa_testkit::FakeSleeper::new(clock)),
        env_allow: vec![],
        limits: ToolLimits::RECOMMENDED,
    })
}
fn skill(root: &Path, name: &str, body: &str) -> Result<(), std::io::Error> {
    fs::create_dir_all(root.join(name))?;
    fs::write(
        root.join(name).join("SKILL.md"),
        format!("---\nname: {name}\ndescription: The {name} skill.\n---\n{body}"),
    )
}
fn registry(root: &Path) -> Result<ToolRegistry, Box<dyn Error>> {
    let set = discover_skills(&[(SkillTier::Install, root)]);
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(SkillTool::new(&set)?))?;
    Ok(registry)
}

#[tokio::test]
async fn tool_u_024_a_skill_loads_its_body_and_helper_paths() -> TestResult {
    let (root, workspace) = (tempfile::tempdir()?, tempfile::tempdir()?);
    skill(root.path(), "pdf-report", "1. Run make_report.py.\n")?;
    fs::create_dir(root.path().join("pdf-report/templates"))?;
    fs::write(root.path().join("pdf-report/make_report.py"), "print()\n")?;
    fs::write(root.path().join("pdf-report/templates/page.md"), "# page\n")?;
    skill(root.path(), "bare", "No newline at the end")?;
    let (registry, cx) = (registry(root.path())?, context(workspace.path())?);
    let spec = registry.get("skill").ok_or("skill is registered")?.spec();
    assert!(spec.parallel_safe);
    let folder = root.path().join("pdf-report");
    let output = registry
        .call("skill", json!({"name": "pdf-report"}), &cx)
        .await?;
    assert_eq!(
        output.text,
        format!(
            "Skill pdf-report (install) in {}\n\n1. Run make_report.py.\n\nHelper files (2):\n{}\n{}\n",
            folder.display(),
            folder.join("make_report.py").display(),
            folder.join("templates/page.md").display()
        )
    );
    let output = registry.call("skill", json!({"name": "bare"}), &cx).await?;
    assert!(
        output
            .text
            .ends_with("No newline at the end\n\nNo helper files.\n"),
        "{}",
        output.text
    );
    Ok(())
}

#[tokio::test]
async fn tool_u_024_unknown_names_and_bad_arguments_are_refused() -> TestResult {
    let (root, workspace) = (tempfile::tempdir()?, tempfile::tempdir()?);
    for number in 0..40 {
        skill(root.path(), &format!("skill-number-{number:02}"), "Body\n")?;
    }
    let (registry, cx) = (registry(root.path())?, context(workspace.path())?);
    let Err(ToolError::InvalidArgs { path, detail }) = registry
        .call("skill", json!({"name": "missing"}), &cx)
        .await
    else {
        return Err("an unknown skill is refused".into());
    };
    assert_eq!(path, "/name");
    assert!(
        detail.starts_with("unknown skill; available: skill-number-00 skill-number-01"),
        "{detail}"
    );
    assert!(detail.ends_with(" …") && detail.len() <= 304, "{detail}");
    for args in [
        json!({}),
        json!({"name": "../etc"}),
        json!({"name": "a", "extra": 1}),
    ] {
        assert!(
            matches!(
                registry.call("skill", args, &cx).await,
                Err(ToolError::InvalidArgs { .. })
            ),
            "the schema refuses bad arguments"
        );
    }
    let empty = SkillTool::new(&discover_skills(&[]))?;
    let Err(ToolError::InvalidArgs { detail, .. }) = empty.call(json!({"name": "any"}), &cx).await
    else {
        return Err("no skills, nothing to load".into());
    };
    assert_eq!(detail, "unknown skill; no skills are available in this run");
    Ok(())
}

#[tokio::test]
async fn tool_u_024_the_file_list_is_bounded() -> TestResult {
    let (root, workspace) = (tempfile::tempdir()?, tempfile::tempdir()?);
    skill(root.path(), "many", "Body\n")?;
    for number in 0..=MAX_LISTED_SKILL_FILES + 4 {
        fs::write(root.path().join(format!("many/file-{number:03}.txt")), "x")?;
    }
    let (registry, cx) = (registry(root.path())?, context(workspace.path())?);
    let output = registry.call("skill", json!({"name": "many"}), &cx).await?;
    assert!(
        output.text.contains("Helper files (105):\n"),
        "{}",
        output.text
    );
    assert!(
        output
            .text
            .contains("file-099.txt\n[5 more files not listed]\n")
    );
    assert!(!output.text.contains("file-100.txt"));
    Ok(())
}

#[tokio::test]
async fn tool_u_024_a_folder_changed_after_discovery_is_refused() -> TestResult {
    let (root, workspace) = (tempfile::tempdir()?, tempfile::tempdir()?);
    for name in ["linked", "renamed", "grown", "gone", "replaced"] {
        skill(root.path(), name, "Body\n")?;
    }
    let (registry, cx) = (registry(root.path())?, context(workspace.path())?);
    std::os::unix::fs::symlink("/etc/passwd", root.path().join("linked/passwd"))?;
    fs::write(
        root.path().join("renamed/SKILL.md"),
        "---\nname: other\ndescription: d\n---\n",
    )?;
    let grown = format!(
        "---\nname: grown\ndescription: d\n---\n{}",
        "x".repeat(16 * 1024)
    );
    fs::write(root.path().join("grown/SKILL.md"), grown)?;
    fs::remove_file(root.path().join("gone/SKILL.md"))?;
    fs::remove_dir_all(root.path().join("replaced"))?;
    fs::write(root.path().join("replaced"), "now a file")?;
    for (name, code) in [
        ("linked", "symlink"),
        ("renamed", "name_mismatch"),
        ("grown", "too_large"),
        ("gone", "missing_manifest"),
        ("replaced", "missing_manifest"),
    ] {
        let result = registry.call("skill", json!({ "name": name }), &cx).await;
        let Err(ToolError::PathInvalid(message)) = result else {
            return Err(format!("{name}: {result:?}").into());
        };
        assert!(
            message.ends_with(&format!("cannot be loaded ({code})")),
            "{message}"
        );
    }
    Ok(())
}

/// A sleeper whose every deadline has already passed.
struct Expired;
impl Sleeper for Expired {
    fn sleep(&self, _: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(std::future::ready(()))
    }
}

#[tokio::test]
async fn tool_u_024_cancellation_and_the_deadline_stop_a_load() -> TestResult {
    let (root, workspace) = (tempfile::tempdir()?, tempfile::tempdir()?);
    skill(root.path(), "slow", "Body\n")?;
    let tool = SkillTool::new(&discover_skills(&[(SkillTier::User, root.path())]))?;
    let mut cx = context(workspace.path())?;
    cx.sleeper = Arc::new(Expired);
    assert_eq!(
        tool.call(json!({"name": "slow"}), &cx).await,
        Err(ToolError::Timeout)
    );
    cx.cancel.cancel();
    assert_eq!(
        tool.call(json!({"name": "slow"}), &cx).await,
        Err(ToolError::Cancelled)
    );
    Ok(())
}

#[test]
fn tool_u_032_the_skill_schema_is_strict_valid() -> TestResult {
    let dir = tempfile::tempdir()?;
    let set = discover_skills(&[(SkillTier::Install, dir.path())]);
    let tool = SkillTool::new(&set)?;
    assert!(
        tqa_types::strict_compatible(&tool.spec().parameters),
        "{}",
        tool.spec().parameters
    );
    Ok(())
}
