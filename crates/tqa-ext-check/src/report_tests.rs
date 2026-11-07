//! TK-U-014 (report): the `ext-check/1` report, its text form and the exit status.
use super::report::{Check, EXIT_ENVIRONMENT, EXIT_FAILED, EXIT_PASSED, Report, Status, Target};
use serde_json::{Value, json};

fn report(statuses: &[Status]) -> Report {
    let mut report = Report::new(
        Target::stdio_host(
            "words",
            "/usr/bin/python3",
            &["-c".into(), "x".into()],
            vec!["LANG_MODE".into()],
            false,
        ),
        "0123456789ab",
        "2026-09-27T06:00:00Z",
    );
    for (index, status) in statuses.iter().enumerate() {
        report.push(Check::new(
            &format!("check_{index}"),
            *status,
            "one sentence",
        ));
    }
    report
}

#[test]
fn tk_u_014_the_report_carries_every_field() -> Result<(), Box<dyn std::error::Error>> {
    let value = serde_json::to_value(report(&[Status::Pass, Status::Warn]))?;
    assert_eq!(value["schema"], "ext-check/1");
    assert_eq!(value["kind"], "mcp");
    assert_eq!(
        value["target"],
        json!({"transport": "stdio", "place": "host", "name": "words", "command": "/usr/bin/python3",
            "args": ["-c", "x"], "env_names": ["LANG_MODE"], "image": null})
    );
    assert_eq!(value["server"], Value::Null);
    assert_eq!(value["tools"], json!([]));
    assert_eq!(value["tools_sha256"], Value::Null);
    assert_eq!(value["ext_check"], json!({"commit": "0123456789ab"}));
    assert_eq!(value["date"], "2026-09-27T06:00:00Z");
    assert_eq!(value["flags"], json!({"allow_network": false}));
    assert_eq!(
        value["checks"],
        json!([
            {"code": "check_0", "status": "pass", "message": "one sentence"},
            {"code": "check_1", "status": "warn", "message": "one sentence"}
        ])
    );
    assert_eq!(value["result"], "pass");
    Ok(())
}

#[test]
fn tk_u_014_skipped_checks_never_count_as_passes() {
    assert_eq!(
        report(&[Status::Pass, Status::Warn, Status::Skip]).exit_code(),
        EXIT_PASSED
    );
    assert_eq!(
        report(&[Status::Pass, Status::Fail, Status::Skip]).exit_code(),
        EXIT_FAILED
    );
    // Nothing checked is not a pass: it is the environment's failure.
    assert_eq!(
        report(&[Status::Skip, Status::Skip]).exit_code(),
        EXIT_ENVIRONMENT
    );
    assert_eq!(report(&[]).exit_code(), EXIT_ENVIRONMENT);
    assert_eq!(report(&[Status::Skip]).result(), "nothing_checked");
    assert_eq!(report(&[Status::Fail]).result(), "fail");
}

#[test]
fn tk_u_014_the_text_form_has_one_line_per_check_and_a_summary() {
    let text = report(&[Status::Pass, Status::Warn, Status::Fail, Status::Skip]).text();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines,
        vec![
            "PASS check_0: one sentence",
            "WARN check_1: one sentence",
            "FAIL check_2: one sentence",
            "SKIP check_3: one sentence",
            "ext-check mcp words: fail (1 passed, 1 warning, 1 failed, 1 skipped; skipped checks are not passes)",
        ]
    );
}

#[test]
fn tk_i_002_a_template_report_names_the_image_and_what_it_resolved_to()
-> Result<(), Box<dyn std::error::Error>> {
    let mut report = Report::new(
        Target::template("sha256:abc"),
        "0123456789ab",
        "2026-09-27T09:00:00Z",
    );
    report.push(Check::new("template_checks", Status::Pass, "ok"));
    report.push(Check::new("posture_and_boot", Status::Skip, "later"));
    report.verified_template("workstation", "sha256:def");
    let value = serde_json::to_value(&report)?;
    assert_eq!(value["kind"], "template");
    assert_eq!(
        value["target"],
        json!({"image": "sha256:abc", "image_id": "sha256:def", "base": "workstation"})
    );
    assert_eq!(value["flags"], json!({"allow_network": false}));
    assert_eq!(value["result"], "pass");
    assert!(report.text().ends_with("ext-check template sha256:abc: pass (1 passed, 0 warnings, 0 failed, 1 skipped; skipped checks are not passes)\n"));
    Ok(())
}
