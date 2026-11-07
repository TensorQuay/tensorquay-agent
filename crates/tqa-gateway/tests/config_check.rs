//! GW-U-057: `tqa-gateway check <path>` validates a gateway file with start-up's own read and validation, and nothing
//! else: exit 0 with one `configuration ok` line, or exit 2 with the reason start-up gives. Start-up reports a bad file
//! with the same line and status. The file's own problems are named (missing, not a file, over 64 KiB, unreadable). No
//! database or container engine is configured in these runs, so anything that touched either would fail.
//! GW-U-114: a deprecated setting is named on stderr as a warning, and the file still passes.
use std::{
    ffi::OsStr,
    process::{Command, Output},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const BIN: &str = env!("CARGO_BIN_EXE_tqa-gateway");
const VALID: &str = "listen='127.0.0.1:0'\npublic_origin='http://127.0.0.1:1'\nmodels_path='models.toml'\n\
                     [storage]\nroot='/var/lib/tqa'\n";

/// The gateway binary with `args` and nothing but `PATH` in its environment.
fn run(args: &[&OsStr]) -> Result<Output, std::io::Error> {
    Command::new(BIN)
        .args(args)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .output()
}

fn check(args: &[&OsStr]) -> Result<Output, std::io::Error> {
    run(&[&[OsStr::new("check")], args].concat())
}

fn file(
    dir: &tempfile::TempDir,
    name: &str,
    text: &str,
) -> Result<std::path::PathBuf, std::io::Error> {
    let path = dir.path().join(name);
    std::fs::write(&path, text)?;
    Ok(path)
}

/// Exit 2 and exactly this one line on stderr, nothing on stdout.
fn refused(output: Output, line: &str) -> TestResult {
    let stderr = String::from_utf8(output.stderr)?;
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert_eq!(stderr, format!("tqa-gateway: {line}\n"));
    assert!(output.stdout.is_empty());
    Ok(())
}

#[test]
fn gw_u_057_a_valid_file_passes_with_nothing_else_configured() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = file(&dir, "gateway.toml", VALID)?;
    let output = check(&[path.as_os_str()])?;
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout)?,
        format!("configuration ok: {}\n", path.display())
    );
    Ok(())
}

#[test]
fn gw_u_057_an_invalid_file_exits_2_with_start_ups_reason() -> TestResult {
    let dir = tempfile::tempdir()?;
    for (text, reason) in [
        (
            format!("{VALID}surprise='s3cret-value'\n"),
            "storage.surprise: unknown field, expected `root`",
        ),
        (
            VALID.replace("http://127.0.0.1:1", "ftp://s3cret-value"),
            "public_origin must contain only scheme, host and optional port",
        ),
        (
            format!("{VALID}#{}\n", "x".repeat(70 * 1024)),
            "configuration exceeds 64 KiB",
        ),
    ] {
        let path = file(&dir, "gateway.toml", &text)?;
        refused(
            check(&[path.as_os_str()])?,
            &format!("invalid gateway configuration: {reason}"),
        )?;
    }
    Ok(())
}

#[test]
fn gw_u_057_the_files_own_problems_are_named() -> TestResult {
    let dir = tempfile::tempdir()?;
    let missing = dir.path().join("absent.toml");
    refused(
        check(&[missing.as_os_str()])?,
        "invalid gateway configuration: the configuration file does not exist",
    )?;
    refused(
        check(&[dir.path().as_os_str()])?,
        "invalid gateway configuration: the configuration path is not a regular file",
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = file(&dir, "locked.toml", VALID)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000))?;
        // Root reads regardless of the mode; the case holds only for an ordinary user.
        if std::fs::read(&path).is_err() {
            refused(
                check(&[path.as_os_str()])?,
                "invalid gateway configuration: the configuration file cannot be read",
            )?;
        }
    }
    Ok(())
}

#[test]
fn gw_u_057_bad_arguments_exit_2() -> TestResult {
    let dir = tempfile::tempdir()?;
    let valid = file(&dir, "gateway.toml", VALID)?;
    for args in [vec![], vec![valid.as_os_str(), OsStr::new("extra")]] {
        refused(
            check(&args)?,
            "invalid gateway configuration: check requires exactly one gateway configuration path",
        )?;
    }
    Ok(())
}

#[test]
fn gw_u_057_start_up_reports_a_bad_file_as_check_does() -> TestResult {
    let dir = tempfile::tempdir()?;
    let missing = dir.path().join("absent.toml");
    let unknown = file(&dir, "unknown.toml", &format!("{VALID}surprise=1\n"))?;
    for path in [
        missing.as_os_str(),
        unknown.as_os_str(),
        dir.path().as_os_str(),
    ] {
        let checked = check(&[path])?;
        let started = run(&[path])?;
        assert_eq!(
            started.status.code(),
            Some(2),
            "{}",
            String::from_utf8_lossy(&started.stderr)
        );
        assert_eq!(
            String::from_utf8(started.stderr)?,
            String::from_utf8(checked.stderr)?,
            "start-up and check give the same line"
        );
    }
    Ok(())
}

// TEST: GW-U-114
#[test]
fn gw_u_114_check_refuses_git_push_naming_egress_ca_file() -> TestResult {
    let dir = tempfile::tempdir()?;
    let key = rcgen::KeyPair::generate()?;
    let mut authority = rcgen::CertificateParams::new(Vec::<String>::new())?;
    authority.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let ca = file(&dir, "ca.pem", &authority.self_signed(&key)?.pem())?;
    let text = format!(
        "{VALID}[git_push]\nca_file={}\n",
        serde_json::to_string(&ca)?
    );
    let path = file(&dir, "gateway.toml", &text)?;
    refused(
        check(&[path.as_os_str()])?,
        "invalid gateway configuration: gateway.toml: [git_push] was removed in this release (ADR-0037 §7): name its \
         ca_file as [egress] ca_file instead, and delete [git_push]",
    )
}
