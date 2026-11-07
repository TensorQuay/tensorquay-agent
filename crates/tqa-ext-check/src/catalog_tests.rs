//! TK-U-015: the catalogue lint. Only complete entries with a passing, matching report and a valid snippet pass; each
//! refusal names the file and the field.
use std::path::Path;

use serde_json::{Value, json};

use super::catalog::lint;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const HASH: &str = "3f1c2d4e5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d";

fn entry() -> String {
    format!(
        "# words\n\nCounts the words in a text.\n\n\
         | Field | Value |\n|---|---|\n\
         | Name | words |\n| Kind | mcp |\n| Source | tests/e2e/servers/words.py in this repository |\n\
         | Version | this repository |\n| Transport | stdio |\n\
         | Tested | ext-check 0123456789ab, 2026-09-27T07:00:00Z |\n| Report | words.ext-check.json |\n\
         | Tools SHA-256 | {HASH} |\n\n\
         ## Configuration\n\n```toml\n[[mcp.servers]]\nname = \"words\"\ncommand = \"/usr/bin/python3\"\n\
         args = [\"/opt/tqa/servers/words.py\"]\n```\n\n\
         ## Tools\n\n| Tool | Model name |\n|---|---|\n\
         | count_words | mcp__words__count_words |\n\n## Notes\n\nNone.\n"
    )
}

fn report() -> Value {
    json!({
        "schema": "ext-check/1", "kind": "mcp",
        "target": {"transport": "stdio", "place": "template", "name": "words", "command": "/usr/bin/python3",
            "args": ["/opt/tqa/servers/words.py"], "env_names": [], "image": "sha256:0123"},
        "server": {"name": "words", "version": "1"},
        "tools": [{"name": "count_words", "model_name": "mcp__words__count_words"}],
        "tools_sha256": HASH,
        "checks": [{"code": "handshake", "status": "pass", "message": "ok"}],
        "ext_check": {"commit": "0123456789ab"}, "date": "2026-09-27T07:00:00Z",
        "flags": {"allow_network": false}, "result": "pass"
    })
}

/// A catalogue with the README and the one entry, optionally changed.
fn catalogue(
    entry: &str,
    report: Option<&Value>,
) -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("README.md"), "# Catalogue\n")?;
    std::fs::create_dir(dir.path().join("mcp"))?;
    std::fs::write(dir.path().join("mcp/words.md"), entry)?;
    if let Some(report) = report {
        std::fs::write(
            dir.path().join("mcp/words.ext-check.json"),
            report.to_string(),
        )?;
    }
    Ok(dir)
}

fn problems(dir: &Path) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    Ok(lint(dir)?.iter().map(ToString::to_string).collect())
}

fn with_report(change: impl FnOnce(&mut Value)) -> Value {
    let mut report = report();
    change(&mut report);
    report
}

#[test]
fn tk_u_015_a_complete_entry_passes() -> TestResult {
    let dir = catalogue(&entry(), Some(&report()))?;
    assert_eq!(problems(dir.path())?, Vec::<String>::new());
    // An empty catalogue with its README passes too.
    let empty = tempfile::tempdir()?;
    std::fs::write(empty.path().join("README.md"), "# Catalogue\n")?;
    assert_eq!(problems(empty.path())?, Vec::<String>::new());
    Ok(())
}

#[test]
fn tk_u_015_each_missing_field_is_named() -> TestResult {
    for field in [
        "Name",
        "Kind",
        "Source",
        "Version",
        "Transport",
        "Tested",
        "Report",
        "Tools SHA-256",
    ] {
        let entry: String = entry()
            .lines()
            .filter(|line| !line.starts_with(&format!("| {field} |")))
            .flat_map(|line| [line, "\n"])
            .collect();
        let dir = catalogue(&entry, Some(&report()))?;
        let found = problems(dir.path())?;
        assert!(
            found
                .iter()
                .any(|problem| problem.starts_with(&format!("mcp/words.md: {field}: "))),
            "{field}: {found:?}"
        );
    }
    Ok(())
}

#[test]
fn tk_u_015_the_sections_and_the_readme_are_required() -> TestResult {
    let without_config = entry().replace("```toml", "```text");
    let dir = catalogue(&without_config, Some(&report()))?;
    assert!(
        problems(dir.path())?
            .iter()
            .any(|p| p.starts_with("mcp/words.md: Configuration: "))
    );
    let without_tools = entry().replace("| Tool | Model name |", "| Something | Else |");
    let dir = catalogue(&without_tools, Some(&report()))?;
    assert!(
        problems(dir.path())?
            .iter()
            .any(|p| p.starts_with("mcp/words.md: Tools: "))
    );
    let dir = catalogue(&entry(), Some(&report()))?;
    std::fs::remove_file(dir.path().join("README.md"))?;
    assert_eq!(
        problems(dir.path())?,
        vec!["README.md: file: missing".to_owned()]
    );
    Ok(())
}

#[test]
fn tk_u_015_the_entry_must_match_its_file_and_kind() -> TestResult {
    let dir = catalogue(
        &entry().replace("| Name | words |", "| Name | other |"),
        Some(&report()),
    )?;
    assert!(
        problems(dir.path())?
            .iter()
            .any(|p| p.starts_with("mcp/words.md: Name: "))
    );
    let dir = catalogue(
        &entry().replace("| Kind | mcp |", "| Kind | template |"),
        Some(&report()),
    )?;
    assert!(
        problems(dir.path())?
            .iter()
            .any(|p| p.starts_with("mcp/words.md: Kind: "))
    );
    Ok(())
}

#[test]
fn tk_u_015_a_missing_failed_or_foreign_report_is_refused() -> TestResult {
    let cases: Vec<(Option<Value>, &str)> = vec![
        (None, "Report"),
        (Some(json!("not an object")), "Report"),
        (
            Some(with_report(|r| r["schema"] = json!("ext-check/2"))),
            "Report",
        ),
        (Some(with_report(|r| r["result"] = json!("fail"))), "Report"),
        (
            Some(with_report(|r| r["kind"] = json!("template"))),
            "Report",
        ),
        (
            Some(with_report(|r| r["target"]["name"] = json!("other"))),
            "Report",
        ),
        // A catalogue entry is tested as a sandbox would run it: in a template image, without network.
        (
            Some(with_report(|r| r["target"]["place"] = json!("host"))),
            "Report",
        ),
        (
            Some(with_report(|r| r["flags"]["allow_network"] = json!(true))),
            "Report",
        ),
        // At a clean, known commit, as the entry says.
        (
            Some(with_report(|r| {
                r["ext_check"]["commit"] = json!("0123456789ab-dirty");
            })),
            "Tested",
        ),
        (
            Some(with_report(|r| r["ext_check"]["commit"] = json!("unknown"))),
            "Tested",
        ),
        (
            Some(with_report(|r| r["date"] = json!("2026-09-28T07:00:00Z"))),
            "Tested",
        ),
    ];
    for (report, field) in cases {
        let dir = catalogue(&entry(), report.as_ref())?;
        let found = problems(dir.path())?;
        assert!(
            found
                .iter()
                .any(|problem| problem.starts_with(&format!("mcp/words.md: {field}: "))),
            "{report:?}: {found:?}"
        );
    }
    Ok(())
}

#[test]
fn tk_u_015_a_tool_list_that_differs_from_the_report_is_refused() -> TestResult {
    let other_hash = with_report(|r| r["tools_sha256"] = json!("0".repeat(64)));
    let dir = catalogue(&entry(), Some(&other_hash))?;
    assert!(
        problems(dir.path())?
            .iter()
            .any(|p| p.starts_with("mcp/words.md: Tools SHA-256: "))
    );
    let other_name = entry().replace(
        "| count_words | mcp__words__count_words |",
        "| count_words | mcp__words__count |",
    );
    let dir = catalogue(&other_name, Some(&report()))?;
    assert!(
        problems(dir.path())?
            .iter()
            .any(|p| p.starts_with("mcp/words.md: Tools: "))
    );
    // ADR-0035: an MCP server has no permission class; an entry that still names one is refused.
    let with_class = entry().replace(
        "| Transport | stdio |\n",
        "| Transport | stdio |\n| Default class | execute |\n",
    );
    let dir = catalogue(&with_class, Some(&report()))?;
    assert!(
        problems(dir.path())?
            .iter()
            .any(|p| p.starts_with("mcp/words.md: Default class: ")),
        "{:?}",
        problems(dir.path())?
    );
    Ok(())
}

#[test]
fn tk_u_015_a_snippet_the_gateway_refuses_is_named() -> TestResult {
    for (from, to) in [
        (
            "command = \"/usr/bin/python3\"",
            "command = \"/workspace/words.py\"",
        ),
        ("command = \"/usr/bin/python3\"", "command = \"python3\""),
        ("name = \"words\"\n", "name = \"other\"\n"),
        ("[[mcp.servers]]", "[[mcp.server]]"),
        ("args = [", "surprise = [0]\nargs = ["),
    ] {
        let dir = catalogue(&entry().replace(from, to), Some(&report()))?;
        let found = problems(dir.path())?;
        assert!(
            found
                .iter()
                .any(|problem| problem.starts_with("mcp/words.md: Configuration: ")),
            "{to}: {found:?}"
        );
    }
    Ok(())
}
