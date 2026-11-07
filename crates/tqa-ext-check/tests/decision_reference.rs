//! TK-I-004: `ext-check decision` against the reference engine (`examples/decision-reference/engine.py`, ADR-0013 point
//! 9), started as its author would start it, with a key: every canned consultation passes at every exposure level.
//! The other half of the row (a wrong engine fails on the rule it breaks) is TK-U-030's scripted engine.
use secrecy::SecretString;
use std::{
    io::{BufRead, BufReader},
    path::Path,
    process::{Child, Command, Stdio},
    time::Duration,
};
use tqa_ext_check::{
    decision::{Engine, check_engine},
    report::{Report, Status, Target},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// The engine process, killed when dropped.
struct Reference(Child);
impl Drop for Reference {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Starts the reference engine on a free loopback port, requiring `key`, and returns it with its base URL.
fn start(key: &str) -> Result<(Reference, String), Box<dyn std::error::Error>> {
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/decision-reference/engine.py");
    let mut child = Command::new("python3")
        .arg(script)
        .args(["--port", "0", "--bearer-from-env", "REFERENCE_ENGINE_KEY"])
        .env("REFERENCE_ENGINE_KEY", key)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let engine = Reference(child);
    // Its first line names the port: "engine: listening on 127.0.0.1:<port>".
    let mut lines = BufReader::new(stdout);
    let mut line = String::new();
    lines.read_line(&mut line)?;
    // Keep reading its decision lines, so its log never meets a closed pipe.
    std::thread::spawn(move || for _ in lines.lines() {});
    let address = line
        .trim()
        .strip_prefix("engine: listening on ")
        .ok_or("the engine didn't say where it listens")?;
    Ok((engine, format!("http://{address}")))
}

// TEST: TK-I-004
#[tokio::test]
async fn tk_i_004_the_reference_engine_passes_the_suite_at_every_level() -> TestResult {
    let (_engine, url) = start("synthetic-reference-key")?;
    let mut report = Report::new(
        Target::engine(&url, 2000, true, false),
        "0000000",
        "2026-09-29T00:00:00Z",
    );
    let target = Engine {
        url: url.parse()?,
        timeout: Duration::from_secs(2),
        credential: Some(SecretString::from("synthetic-reference-key")),
    };
    check_engine(&target, &mut report).await;
    let consultations: Vec<_> = report
        .checks()
        .iter()
        .filter(|check| check.code.starts_with("decision.") && check.code.matches('.').count() == 2)
        .collect();
    assert_eq!(consultations.len(), 18, "{}", report.text());
    assert!(
        consultations
            .iter()
            .all(|check| check.status == Status::Pass),
        "{}",
        report.text()
    );
    assert_eq!(report.result(), "pass");
    // Without the key the engine refuses, so every consultation fails as the loop would record it.
    let mut keyless = Report::new(
        Target::engine(&url, 2000, false, false),
        "0000000",
        "2026-09-29T00:00:00Z",
    );
    let target = Engine {
        credential: None,
        ..target
    };
    check_engine(&target, &mut keyless).await;
    assert_eq!(keyless.result(), "fail");
    assert!(keyless.text().contains("(unreachable)"));
    Ok(())
}
