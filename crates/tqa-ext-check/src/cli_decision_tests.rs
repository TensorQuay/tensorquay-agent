//! TK-U-031: `ext-check decision`'s command line. A loopback engine needs nothing more; any other address needs an
//! explicit `--network` and `https`. The credential is named by its environment variable, never given on the command
//! line, and no refusal echoes a value.
use super::cli::{Invocation, parse};

fn args(line: &[&str]) -> Vec<String> {
    line.iter().map(|arg| (*arg).to_owned()).collect()
}
fn refusal(line: &[&str]) -> String {
    match parse(args(line)) {
        Err(usage) => usage.to_string(),
        Ok(_) => format!("accepted: {line:?}"),
    }
}

// TEST: TK-U-031
#[test]
fn tk_u_031_a_decision_invocation_is_parsed_with_its_defaults() -> Result<(), String> {
    let Invocation::Decision(plain) =
        parse(args(&["decision", "http://127.0.0.1:8700"])).map_err(|usage| usage.to_string())?
    else {
        return Err("not a decision invocation".into());
    };
    assert_eq!(plain.url.as_str(), "http://127.0.0.1:8700/");
    assert_eq!(plain.timeout_ms, 2000);
    assert_eq!(plain.bearer_env, None);
    assert!(!plain.network && plain.json.is_none());
    let Invocation::Decision(full) = parse(args(&[
        "decision",
        "https://engine.example/policy",
        "--network",
        "--timeout-ms",
        "5000",
        "--bearer-from-env",
        "ENGINE_KEY",
        "--json",
        "report.json",
    ]))
    .map_err(|usage| usage.to_string())?
    else {
        return Err("not a decision invocation".into());
    };
    assert_eq!(full.timeout_ms, 5000);
    assert_eq!(full.bearer_env.as_deref(), Some("ENGINE_KEY"));
    assert!(full.network);
    assert_eq!(
        full.json.as_deref(),
        Some(std::path::Path::new("report.json"))
    );
    for loopback in ["http://localhost:9000", "http://[::1]:9000"] {
        assert!(parse(args(&["decision", loopback])).is_ok(), "{loopback}");
    }
    Ok(())
}

// TEST: TK-U-031
#[test]
fn tk_u_031_a_remote_engine_needs_network_and_https_and_bad_values_are_refused() {
    for (line, expected) in [
        (&["decision"][..], "expected the engine's base URL"),
        (
            &["decision", "https://engine.example"][..],
            "needs an explicit --network",
        ),
        (
            &["decision", "http://engine.example", "--network"][..],
            "a remote engine needs https",
        ),
        (&["decision", "ftp://127.0.0.1"][..], "http:// or https://"),
        (&["decision", "not a url"][..], "http:// or https://"),
        (
            &["decision", "http://127.0.0.1/#part"][..],
            "without a fragment",
        ),
        (
            &["decision", "http://127.0.0.1", "--timeout-ms", "0"][..],
            "--timeout-ms must be 1 through 10000",
        ),
        (
            &["decision", "http://127.0.0.1", "--timeout-ms", "10001"][..],
            "--timeout-ms must be 1 through 10000",
        ),
        (
            &[
                "decision",
                "http://127.0.0.1",
                "--bearer-from-env",
                "lower case",
            ][..],
            "--bearer-from-env expects",
        ),
        (
            &["decision", "http://127.0.0.1", "--json"][..],
            "--json needs a value",
        ),
        (
            &["decision", "http://127.0.0.1", "--json", "a", "--json", "b"][..],
            "--json given twice",
        ),
        (
            &["decision", "http://127.0.0.1", "extra"][..],
            "unexpected argument",
        ),
    ] {
        let message = refusal(line);
        assert!(message.contains(expected), "{line:?}: {message}");
    }
}

// TEST: TK-U-031
#[test]
fn tk_u_031_a_credential_is_never_taken_or_echoed_from_the_command_line() {
    for line in [
        &["decision", "http://user:synthetic-secret@127.0.0.1"][..],
        &["decision", "http://127.0.0.1", "--token=synthetic-secret"][..],
        &[
            "decision",
            "http://127.0.0.1",
            "--bearer",
            "synthetic-secret",
        ][..],
    ] {
        let message = refusal(line);
        assert!(!message.starts_with("accepted"), "{line:?}");
        assert!(!message.contains("synthetic-secret"), "{line:?}: {message}");
    }
    assert!(
        refusal(&["decision", "http://user:synthetic-secret@127.0.0.1"])
            .contains("without a name or password")
    );
}
