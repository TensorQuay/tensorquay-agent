//! PROMPT-U-002/005/006: the skills directory: manifests, tiers, precedence, bounds and the prompt index.
use std::{error::Error, fs, path::Path};
use tqa_prompt::{
    MAX_SKILLS, SKILL_INDEX_BYTES, SKILL_MANIFEST_BYTES, SkillTier, SkipReason, discover_skills,
    parse_manifest, render_skill_index,
};

type TestResult = Result<(), Box<dyn Error>>;

fn manifest(name: &str, description: &str) -> String {
    format!("---\nname: {name}\ndescription: {description}\n---\nUse {name} like this.\n")
}
fn skill(root: &Path, folder: &str, text: &str) -> Result<(), std::io::Error> {
    fs::create_dir_all(root.join(folder))?;
    fs::write(root.join(folder).join("SKILL.md"), text)
}
fn names(set: &tqa_prompt::SkillSet) -> Vec<(String, SkillTier)> {
    set.skills
        .iter()
        .map(|skill| (skill.name.clone(), skill.tier))
        .collect()
}
fn skipped(set: &tqa_prompt::SkillSet) -> Vec<(String, SkillTier, SkipReason)> {
    set.skipped
        .iter()
        .map(|skip| (skip.folder.clone(), skip.tier, skip.reason))
        .collect()
}

#[test]
fn prompt_u_002_valid_skills_load_and_an_invalid_manifest_is_skipped_with_its_reason() -> TestResult
{
    let install = tempfile::tempdir()?;
    skill(
        install.path(),
        "pdf-report",
        &manifest("pdf-report", "Turn a CSV into a PDF report."),
    )?;
    skill(
        install.path(),
        "changelog",
        &manifest("changelog", "Write a changelog entry from commits."),
    )?;
    skill(install.path(), "broken", "no front matter here\n")?;
    let set = discover_skills(&[(SkillTier::Install, install.path())]);
    assert_eq!(
        names(&set),
        [
            ("changelog".into(), SkillTier::Install),
            ("pdf-report".into(), SkillTier::Install)
        ]
    );
    assert_eq!(
        skipped(&set),
        [(
            "broken".into(),
            SkillTier::Install,
            SkipReason::BadFrontMatter
        )]
    );
    let index = render_skill_index(&set);
    assert!(
        index.contains("- changelog (install): Write a changelog entry from commits.\n"),
        "{index}"
    );
    assert!(
        index.find("changelog").unwrap_or(usize::MAX) < index.find("pdf-report").unwrap_or(0),
        "sorted by name"
    );
    assert!(!index.contains("broken"));
    Ok(())
}

#[test]
fn prompt_u_005_the_manifest_has_exactly_two_keys_between_fences() {
    let ok = |text: &str| {
        parse_manifest("pdf-report", text)
            .map(|parsed| (parsed.name, parsed.description, parsed.body))
    };
    assert_eq!(
        ok("---\nname: pdf-report\ndescription: Make reports.\n---\nBody\n"),
        Ok(("pdf-report".into(), "Make reports.".into(), "Body\n".into()))
    );
    assert_eq!(
        ok("---\r\ndescription: Make reports.\r\nname: pdf-report\r\n---\r\n").map(|p| p.1),
        Ok("Make reports.".into())
    );
    assert_eq!(
        ok("---\nname: pdf-report\ndescription: Make reports.\n---").map(|p| p.2),
        Ok(String::new()),
        "an empty body"
    );
    let long = "x".repeat(281);
    let cases = [
        (
            "name: pdf-report\ndescription: d\n---\n",
            SkipReason::BadFrontMatter,
        ),
        (
            "---\nname: pdf-report\ndescription: d\n",
            SkipReason::BadFrontMatter,
        ),
        (
            "---\nname: pdf-report\ndescription: d\nlicence: mit\n---\n",
            SkipReason::BadFrontMatter,
        ),
        (
            "---\nname: pdf-report\nname: pdf-report\n---\n",
            SkipReason::BadFrontMatter,
        ),
        ("---\nname: pdf-report\n---\n", SkipReason::BadFrontMatter),
        (
            "---\nname: pdf-report\ndescription:\n---\n",
            SkipReason::BadFrontMatter,
        ),
        (
            "---\nname pdf-report\ndescription: d\n---\n",
            SkipReason::BadFrontMatter,
        ),
        (
            "---\nname: other\ndescription: d\n---\n",
            SkipReason::NameMismatch,
        ),
        (
            "---\nname: PDF_Report\ndescription: d\n---\n",
            SkipReason::NameMismatch,
        ),
        ("", SkipReason::BadFrontMatter),
    ];
    for (text, reason) in cases {
        assert_eq!(ok(text).err(), Some(reason), "{text:?}");
    }
    let too_long = format!("---\nname: pdf-report\ndescription: {long}\n---\n");
    assert_eq!(ok(&too_long).err(), Some(SkipReason::BadFrontMatter));
    let control = "---\nname: pdf-report\ndescription: bell\u{7}\n---\n";
    assert_eq!(ok(control).err(), Some(SkipReason::BadFrontMatter));
}

#[test]
fn prompt_u_006_the_user_tier_shadows_the_install_tier_and_the_loss_is_recorded() -> TestResult {
    let (install, user) = (tempfile::tempdir()?, tempfile::tempdir()?);
    skill(
        install.path(),
        "report",
        &manifest("report", "Install version."),
    )?;
    skill(install.path(), "lint", &manifest("lint", "Install only."))?;
    skill(user.path(), "report", &manifest("report", "My version."))?;
    let missing = install.path().join("does-not-exist");
    let set = discover_skills(&[
        (SkillTier::Install, install.path()),
        (SkillTier::User, user.path()),
        (SkillTier::User, &missing),
    ]);
    assert_eq!(
        names(&set),
        [
            ("lint".into(), SkillTier::Install),
            ("report".into(), SkillTier::User)
        ]
    );
    assert_eq!(set.skills[1].description, "My version.");
    assert_eq!(
        skipped(&set),
        [("report".into(), SkillTier::Install, SkipReason::Shadowed)]
    );
    assert_eq!(
        render_skill_index(&set),
        render_skill_index(&set),
        "the index is byte-stable"
    );
    Ok(())
}

#[test]
fn prompt_u_006_files_and_folders_outside_the_bounds_are_skipped() -> TestResult {
    let root = tempfile::tempdir()?;
    skill(
        root.path(),
        "big-manifest",
        &format!(
            "{}{}",
            manifest("big-manifest", "d"),
            "x".repeat(usize::try_from(SKILL_MANIFEST_BYTES)?)
        ),
    )?;
    skill(root.path(), "big-folder", &manifest("big-folder", "d"))?;
    fs::write(
        root.path().join("big-folder/data.bin"),
        vec![0_u8; 1024 * 1024 + 1],
    )?;
    fs::create_dir(root.path().join("no-manifest"))?;
    skill(root.path(), "latin1", "")?;
    fs::write(
        root.path().join("latin1/SKILL.md"),
        b"---\nname: latin1\ndescription: caf\xe9\n---\n",
    )?;
    skill(
        root.path(),
        "linked-helper",
        &manifest("linked-helper", "d"),
    )?;
    std::os::unix::fs::symlink("/etc/passwd", root.path().join("linked-helper/passwd"))?;
    let elsewhere = tempfile::tempdir()?;
    skill(elsewhere.path(), "linked", &manifest("linked", "d"))?;
    std::os::unix::fs::symlink(elsewhere.path().join("linked"), root.path().join("linked"))?;
    fs::write(
        root.path().join("README.md"),
        "a file at the tier root is not a skill",
    )?;
    let set = discover_skills(&[(SkillTier::Install, root.path())]);
    assert!(set.skills.is_empty(), "{:?}", names(&set));
    let expected = [
        ("big-folder", SkipReason::TooLarge),
        ("big-manifest", SkipReason::TooLarge),
        ("latin1", SkipReason::NotUtf8),
        ("linked", SkipReason::Symlink),
        ("linked-helper", SkipReason::Symlink),
        ("no-manifest", SkipReason::MissingManifest),
    ];
    let expected: Vec<_> = expected
        .iter()
        .map(|(folder, reason)| ((*folder).to_owned(), SkillTier::Install, *reason))
        .collect();
    assert_eq!(skipped(&set), expected);
    Ok(())
}

#[test]
fn prompt_u_006_the_count_and_the_index_are_bounded() -> TestResult {
    let root = tempfile::tempdir()?;
    for number in 0..=MAX_SKILLS {
        let name = format!("skill-{number:03}");
        skill(root.path(), &name, &manifest(&name, &"d".repeat(280)))?;
    }
    let set = discover_skills(&[(SkillTier::Install, root.path())]);
    assert_eq!(set.skills.len(), MAX_SKILLS);
    let last = format!("skill-{MAX_SKILLS:03}");
    assert_eq!(
        skipped(&set),
        [(last, SkillTier::Install, SkipReason::OverLimit)]
    );
    let index = render_skill_index(&set);
    assert!(index.len() <= SKILL_INDEX_BYTES, "{} bytes", index.len());
    assert!(
        index.ends_with("[skills index truncated: byte limit]\n"),
        "{}",
        &index[index.len().saturating_sub(80)..]
    );
    assert!(
        render_skill_index(&discover_skills(&[])).is_empty(),
        "no skills, no index"
    );
    Ok(())
}

#[test]
fn prompt_u_006_nested_helpers_load_and_an_unreadable_folder_is_skipped() -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir()?;
    skill(root.path(), "nested", &manifest("nested", "Has helpers."))?;
    fs::create_dir_all(root.path().join("nested/scripts/lib"))?;
    fs::write(root.path().join("nested/scripts/lib/run.py"), "print()\n")?;
    skill(root.path(), "locked", &manifest("locked", "d"))?;
    let locked = root.path().join("locked/private");
    fs::create_dir(&locked)?;
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000))?;
    let set = discover_skills(&[(SkillTier::User, root.path())]);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o700))?;
    assert_eq!(names(&set), [("nested".into(), SkillTier::User)]);
    assert_eq!(
        skipped(&set),
        [("locked".into(), SkillTier::User, SkipReason::Unreadable)]
    );
    assert!(render_skill_index(&set).contains("- nested (user): Has helpers.\n"));
    Ok(())
}

#[test]
fn prompt_u_005_skip_reasons_have_stable_codes() {
    let codes = [
        (SkipReason::MissingManifest, "missing_manifest"),
        (SkipReason::BadFrontMatter, "bad_front_matter"),
        (SkipReason::NameMismatch, "name_mismatch"),
        (SkipReason::TooLarge, "too_large"),
        (SkipReason::Symlink, "symlink"),
        (SkipReason::NotUtf8, "not_utf8"),
        (SkipReason::Shadowed, "shadowed"),
        (SkipReason::OverLimit, "over_limit"),
        (SkipReason::Unreadable, "unreadable"),
    ];
    for (reason, code) in codes {
        assert_eq!(reason.code(), code);
    }
    assert_eq!(
        (SkillTier::Install.label(), SkillTier::User.label()),
        ("install", "user")
    );
}

#[test]
fn prompt_u_006_the_truncated_index_never_exceeds_its_cap_with_the_marker() {
    // Every line length lands the cut at a different offset, so the marker's reserved bytes matter for some of them.
    for length in 200..=280 {
        let skills = (0..MAX_SKILLS)
            .map(|number| tqa_prompt::Skill {
                name: format!("skill-{number:03}"),
                description: "d".repeat(length),
                tier: SkillTier::Install,
                path: std::path::PathBuf::new(),
            })
            .collect();
        let set = tqa_prompt::SkillSet {
            skills,
            skipped: vec![],
        };
        let index = render_skill_index(&set);
        assert!(
            index.len() <= SKILL_INDEX_BYTES,
            "{length}: {} bytes",
            index.len()
        );
        assert!(
            index.ends_with("[skills index truncated: byte limit]\n"),
            "{length}"
        );
    }
}
