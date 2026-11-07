//! TK-U-014 (process): usage errors exit 2 on stderr without echoing values; `--help` prints the usage and exits 0.
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_tqa-ext-check");

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn tk_u_014_usage_errors_exit_2_without_echoing_values() -> TestResult {
    let output = Command::new(BIN)
        .args([
            "mcp",
            "stdio",
            "--name",
            "w",
            "--host",
            "--token=s3cret-value",
            "--",
            "/bin/w",
        ])
        .output()?;
    let stderr = String::from_utf8(output.stderr)?;
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("unknown option --token") && stderr.contains("usage:"),
        "{stderr}"
    );
    assert!(!stderr.contains("s3cret-value"), "{stderr}");
    assert!(output.stdout.is_empty());
    Ok(())
}

#[test]
fn tk_u_014_help_prints_the_usage_and_exits_0() -> TestResult {
    let output = Command::new(BIN).arg("--help").output()?;
    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8(output.stdout)?.starts_with("usage: ext-check mcp stdio"));
    Ok(())
}

const FAKE: &str = env!("CARGO_BIN_EXE_tqa-fake-mcp");

fn stdio(options: &[&str]) -> Command {
    let mut command = Command::new(BIN);
    command
        .args(["mcp", "stdio", "--name", "fake"])
        .args(options)
        .args(["--", FAKE])
        .env("TQA_EXT_CHECK_COMMIT", "0123456789ab");
    command
}

fn path(file: &std::path::Path) -> Result<&str, &'static str> {
    file.to_str().ok_or("path")
}

#[test]
fn tk_u_014_a_passing_server_exits_0_with_its_report() -> TestResult {
    let dir = tempfile::tempdir()?;
    let report_path = dir.path().join("report.json");
    let output = stdio(&[
        "--allow-network",
        "--host",
        "--env",
        "FAKE_MCP_MODE=normal",
        "--env",
        "API_MODE=s3cret-value",
        "--json",
        path(&report_path)?,
    ])
    .output()?;
    let stdout = String::from_utf8(output.stdout)?;
    assert_eq!(output.status.code(), Some(0), "{stdout}");
    assert!(
        stdout.contains("PASS handshake") && stdout.contains("ext-check mcp fake: pass"),
        "{stdout}"
    );
    let report: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&report_path)?)?;
    assert_eq!(report["schema"], "ext-check/1");
    assert_eq!(report["result"], "pass");
    assert_eq!(report["ext_check"]["commit"], "0123456789ab");
    assert_eq!(report["flags"]["allow_network"], true);
    assert_eq!(
        report["target"]["env_names"],
        serde_json::json!(["API_MODE", "FAKE_MCP_MODE"])
    );
    let text = report.to_string();
    for leak in [stdout.as_str(), text.as_str()] {
        assert!(!leak.contains("s3cret-value"));
    }
    Ok(())
}

#[test]
fn tk_u_014_a_failing_check_exits_1() -> TestResult {
    let output = stdio(&[
        "--allow-network",
        "--host",
        "--env",
        "FAKE_MCP_MODE=unknown_ok",
    ])
    .output()?;
    let stdout = String::from_utf8(output.stdout)?;
    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert!(stdout.contains("FAIL unknown_tool_not_error"), "{stdout}");
    Ok(())
}

#[test]
fn tk_u_014_a_bad_fixture_file_is_a_usage_error() -> TestResult {
    let dir = tempfile::tempdir()?;
    let calls = dir.path().join("calls.json");
    for fixture in [
        "{}",
        r#"[{"tool": "echo"}]"#,
        r#"[{"tool": "echo", "expect": "maybe"}]"#,
        "not json",
    ] {
        std::fs::write(&calls, fixture)?;
        let output = stdio(&["--allow-network", "--host", "--calls", path(&calls)?]).output()?;
        let stderr = String::from_utf8(output.stderr)?;
        assert_eq!(output.status.code(), Some(2), "{fixture}: {stderr}");
        assert!(stderr.contains("--calls"), "{stderr}");
    }
    std::fs::write(
        &calls,
        r#"[{"tool": "echo", "arguments": {"text": "hi"}, "expect": "ok", "contains": "hi"}]"#,
    )?;
    let output = stdio(&["--allow-network", "--host", "--calls", path(&calls)?]).output()?;
    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8(output.stdout)?.contains("PASS call:"));
    Ok(())
}

#[test]
fn tk_u_014_a_template_run_needs_a_pinned_image_that_is_already_here() -> TestResult {
    let output = stdio(&["--template", "tqa-runtime:workstation"]).output()?;
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8(output.stderr)?.contains("must be pinned"));
    // An absent image (or no docker client at all) is the environment's problem: never a pull.
    let output = stdio(&[
        "--template",
        "sha256:0000000000000000000000000000000000000000000000000000000000000001",
    ])
    .output()?;
    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8(output.stderr)?;
    assert!(
        stderr.contains("never pulls") || stderr.contains("no docker client"),
        "{stderr}"
    );
    Ok(())
}

#[test]
fn tk_u_014_a_host_run_has_no_network_unless_allowed() -> TestResult {
    // In a network namespace the server still passes; the report records that the network was not allowed.
    let dir = tempfile::tempdir()?;
    let report_path = dir.path().join("report.json");
    let output = stdio(&["--host", "--json", path(&report_path)?]).output()?;
    let code = output.status.code();
    if code == Some(3) {
        // No unprivileged network namespace here: an environment error that names the way out, never a silent run.
        assert!(String::from_utf8(output.stderr)?.contains("--allow-network"));
        return Ok(());
    }
    assert_eq!(code, Some(0), "{}", String::from_utf8_lossy(&output.stdout));
    let report: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&report_path)?)?;
    assert_eq!(report["flags"]["allow_network"], false);
    Ok(())
}

#[test]
fn tk_u_015_catalog_lint_exits_0_when_clean_and_1_with_problems() -> TestResult {
    let dir = tempfile::tempdir()?;
    let lint = || {
        Command::new(BIN)
            .args(["catalog", "lint"])
            .arg(dir.path())
            .output()
    };
    let output = lint()?;
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stdout)?,
        "README.md: file: missing\next-check catalog: 1 problem\n"
    );
    std::fs::write(dir.path().join("README.md"), "# Catalogue\n")?;
    let output = lint()?;
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout)?,
        "ext-check catalog: 0 problems\n"
    );
    let missing = Command::new(BIN)
        .args(["catalog", "lint", "/nonexistent/catalogue"])
        .output()?;
    assert_eq!(missing.status.code(), Some(1));
    Ok(())
}
