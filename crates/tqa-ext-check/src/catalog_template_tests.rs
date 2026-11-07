//! TK-U-015 (templates): a template entry passes the lint only with a passing `ext-check/1` template report for its
//! image and base, whose static checks, posture and boot to ready all passed (a skip is not a pass), made at a clean,
//! known commit, and with a snippet holding exactly its own `[[templates]]` entry, which the gateway accepts.
use std::path::Path;

use serde_json::{Value, json};

use super::catalog::lint;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const IMAGE: &str = "sha256:35f1eae4999643bf859a35fa9c7e2b0adcd41c3f91b21990cf84f6ec945f0347";

fn entry() -> String {
    format!(
        "# workstation\n\nThe built-in Workstation environment.\n\n\
         | Field | Value |\n|---|---|\n\
         | Name | workstation |\n| Kind | template |\n| Source | `images/runtime/Dockerfile` in this repository |\n\
         | Version | this repository at 0123456 |\n| Base | workstation |\n| Image | {IMAGE} |\n\
         | Tested | ext-check 0123456789ab, 2026-09-27T07:00:00Z |\n| Report | workstation.ext-check.json |\n\n\
         ## Configuration\n\n```toml\n[[templates]]\nname = \"workstation\"\ndisplay_name = \"Workstation\"\n\
         image = \"{IMAGE}\"\ncapabilities = [\"shell\"]\n```\n\n## Notes\n\nNone.\n"
    )
}

fn report() -> Value {
    json!({
        "schema": "ext-check/1", "kind": "template",
        "target": {"image": IMAGE, "image_id": IMAGE, "base": "workstation"},
        "server": null, "tools": [], "tools_sha256": null,
        "checks": [
            {"code": "template_checks", "status": "pass", "message": "ok"},
            {"code": "posture", "status": "pass", "message": "ok"},
            {"code": "boot_ready", "status": "pass", "message": "ok"},
            {"code": "microvm_rootfs", "status": "skip", "message": "no KVM"}
        ],
        "ext_check": {"commit": "0123456789ab"}, "date": "2026-09-27T07:00:00Z",
        "flags": {"allow_network": false}, "result": "pass"
    })
}

fn catalogue(
    entry: &str,
    report: Option<&Value>,
) -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("README.md"), "# Catalogue\n")?;
    std::fs::create_dir(dir.path().join("templates"))?;
    std::fs::write(dir.path().join("templates/workstation.md"), entry)?;
    if let Some(report) = report {
        std::fs::write(
            dir.path().join("templates/workstation.ext-check.json"),
            report.to_string(),
        )?;
    }
    Ok(dir)
}

fn problems(dir: &Path) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    Ok(lint(dir)?.iter().map(ToString::to_string).collect())
}

fn has(found: &[String], field: &str) -> bool {
    found
        .iter()
        .any(|problem| problem.starts_with(&format!("templates/workstation.md: {field}: ")))
}

#[test]
fn tk_u_015_a_complete_template_entry_passes() -> TestResult {
    let dir = catalogue(&entry(), Some(&report()))?;
    assert_eq!(problems(dir.path())?, Vec::<String>::new());
    // A snippet that is the install's default passes too.
    let default = entry().replace(
        "capabilities = [\"shell\"]\n",
        "capabilities = [\"shell\"]\ndefault = true\n",
    );
    let dir = catalogue(&default, Some(&report()))?;
    assert_eq!(problems(dir.path())?, Vec::<String>::new());
    Ok(())
}

#[test]
fn tk_u_015_each_missing_template_field_is_named() -> TestResult {
    for field in [
        "Name", "Kind", "Source", "Version", "Base", "Image", "Tested", "Report",
    ] {
        let entry: String = entry()
            .lines()
            .filter(|line| !line.starts_with(&format!("| {field} |")))
            .flat_map(|line| [line, "\n"])
            .collect();
        let dir = catalogue(&entry, Some(&report()))?;
        let found = problems(dir.path())?;
        assert!(has(&found, field), "{field}: {found:?}");
    }
    Ok(())
}

#[test]
fn tk_u_015_a_template_report_must_pass_every_stage_for_this_image() -> TestResult {
    let change = |edit: &dyn Fn(&mut Value)| {
        let mut report = report();
        edit(&mut report);
        report
    };
    for (why, report, field) in [
        (
            "an MCP report",
            change(&|r| r["kind"] = json!("mcp")),
            "Report",
        ),
        (
            "a failed report",
            change(&|r| r["result"] = json!("fail")),
            "Report",
        ),
        (
            "posture skipped",
            change(&|r| r["checks"][1]["status"] = json!("skip")),
            "Report",
        ),
        (
            "boot missing",
            change(&|r| {
                r["checks"].as_array_mut().map(|checks| checks.remove(2));
            }),
            "Report",
        ),
        (
            "another image",
            change(&|r| r["target"]["image"] = json!(format!("sha256:{}", "1".repeat(64)))),
            "Image",
        ),
        (
            "another base",
            change(&|r| r["target"]["base"] = json!("minimal")),
            "Base",
        ),
        (
            "a dirty commit",
            change(&|r| r["ext_check"]["commit"] = json!("0123456789ab-dirty")),
            "Tested",
        ),
    ] {
        let dir = catalogue(&entry(), Some(&report))?;
        let found = problems(dir.path())?;
        assert!(has(&found, field), "{why}: {found:?}");
    }
    let dir = catalogue(&entry(), None)?;
    assert!(has(&problems(dir.path())?, "Report"));
    Ok(())
}

#[test]
fn tk_u_015_a_template_snippet_must_be_its_own_entry_and_valid() -> TestResult {
    for (from, to) in [
        ("name = \"workstation\"\n", "name = \"other\"\n"),
        (
            &format!("image = \"{IMAGE}\"") as &str,
            "image = \"tqa-runtime:workstation\"",
        ),
        (
            &format!("image = \"{IMAGE}\""),
            &format!("image = \"sha256:{}\"", "1".repeat(64)),
        ),
        (
            "capabilities = [\"shell\"]",
            "capabilities = [\"teleport\"]",
        ),
        ("[[templates]]", "[[template]]"),
    ] {
        let dir = catalogue(&entry().replace(from, to), Some(&report()))?;
        let found = problems(dir.path())?;
        assert!(has(&found, "Configuration"), "{to}: {found:?}");
    }
    Ok(())
}
