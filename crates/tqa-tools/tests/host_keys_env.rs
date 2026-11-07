//! RT-I-037 (ADR-0037 §3, §7): every command of a run gets each of its keys' variables holding the placeholder, and git
//! the settings that use them: a `basic` key's credential helper, only inside its path prefix, and a `bearer` key's
//! header. Nothing else in the environment changes, and the repository-safety settings stay first.
#[path = "support/context.rs"]
mod support;
use serde_json::json;
use std::{collections::BTreeMap, error::Error};
use tqa_testkit::workspace_fixture;
use tqa_tools::{Tool, builtin::Shell};
use tqa_types::HostKeyGrant;

type TestResult = Result<(), Box<dyn Error>>;

const GITHUB: &str = "tqa_ph_github0000000000000000000000000000000000000";
const API: &str = "tqa_ph_api0000000000000000000000000000000000000000";
const PLAIN: &str = "tqa_ph_plain00000000000000000000000000000000000000";

fn grants() -> Result<Vec<HostKeyGrant>, Box<dyn Error>> {
    Ok(vec![
        HostKeyGrant::new(
            "GITHUB_WORK".into(),
            ("github.com".into(), 443, Some("/acme".into())),
            ("basic".into(), Some("x-access-token".into())),
            GITHUB.into(),
        )?,
        HostKeyGrant::new(
            "API_TOKEN".into(),
            ("api.example.com".into(), 8443, None),
            ("bearer".into(), None),
            API.into(),
        )?,
        HostKeyGrant::new(
            "GITEA".into(),
            ("gitea.example.org".into(), 443, None),
            ("basic".into(), None),
            PLAIN.into(),
        )?,
    ])
}

async fn run(
    shell: &Shell,
    cx: &tqa_tools::ToolContext,
    command: &str,
) -> Result<String, Box<dyn Error>> {
    let output = shell.call(json!({ "command": command }), cx).await?;
    assert_eq!(output.exit_code, Some(0), "{command}: {}", output.text);
    Ok(output.text)
}

fn variables(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|line| line.split_once('='))
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect()
}

/// Git that reads only the tool's environment, never the host's own configuration.
const GIT: &str = "GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 GIT_TERMINAL_PROMPT=0 git";

// TEST: RT-I-037
#[tokio::test]
async fn rt_i_037_each_key_is_a_variable_and_nothing_else_changes() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = &support::context(&fixture);
    let keyed = Shell::new().with_host_keys(&grants()?);
    let with = variables(&run(&keyed, cx, "env").await?);
    let without = variables(&run(&Shell::new(), cx, "env").await?);
    for (name, placeholder) in [
        ("GITHUB_WORK", GITHUB),
        ("API_TOKEN", API),
        ("GITEA", PLAIN),
    ] {
        assert_eq!(
            with.get(name).map(String::as_str),
            Some(placeholder),
            "{name}"
        );
    }
    // The safety settings keep their places; the keys' settings follow them.
    assert_eq!(with["GIT_CONFIG_KEY_0"], "core.hooksPath");
    assert_eq!(with["GIT_CONFIG_KEY_1"], "core.fsmonitor");
    let count: usize = with["GIT_CONFIG_COUNT"].parse()?;
    assert!(count > 2, "{count}");
    // Nothing else changes: only the keys' variables and git's settings differ.
    for (name, value) in &with {
        if without.get(name) != Some(value) {
            assert!(
                ["GITHUB_WORK", "API_TOKEN", "GITEA"].contains(&name.as_str())
                    || name.starts_with("GIT_CONFIG_"),
                "{name} changed"
            );
        }
    }
    assert!(without.keys().all(|name| with.contains_key(name)));
    Ok(())
}

// TEST: RT-I-037
#[tokio::test]
async fn rt_i_037_git_uses_a_key_only_where_it_goes() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = &support::context(&fixture);
    let keyed = Shell::new().with_host_keys(&grants()?);
    // A basic key: git's credential helper answers inside the key's path, with its user name, and nowhere else.
    let fill = |url: &str| format!("printf 'url={url}\\n\\n' | {GIT} credential fill 2>&1 || true");
    let inside = run(&keyed, cx, &fill("https://github.com/acme/tools.git")).await?;
    assert!(
        inside.contains("username=x-access-token")
            && inside.contains(&format!("password={GITHUB}")),
        "{inside}"
    );
    let outside = run(&keyed, cx, &fill("https://github.com/other/tools.git")).await?;
    assert!(!outside.contains(GITHUB), "{outside}");
    // Without a stored user name, git is given one.
    let plain = run(&keyed, cx, &fill("https://gitea.example.org/team/repo.git")).await?;
    assert!(
        plain.contains("username=git") && plain.contains(&format!("password={PLAIN}")),
        "{plain}"
    );
    // A bearer key: the header on its host and port.
    let header = run(
        &keyed,
        cx,
        &format!(
            "{GIT} config --get-urlmatch http.extraHeader https://api.example.com:8443/v1/repo.git"
        ),
    )
    .await?;
    assert_eq!(header.trim(), format!("Authorization: Bearer {API}"));
    let elsewhere = run(
        &keyed,
        cx,
        &format!("{GIT} config --get-urlmatch http.extraHeader https://api.example.com/v1/repo.git || true"),
    )
    .await?;
    assert!(!elsewhere.contains(API), "{elsewhere}");
    Ok(())
}

const ADDRESSED: &str = "tqa_ph_addr000000000000000000000000000000000000000";
const BY_V4: &str = "tqa_ph_byv4000000000000000000000000000000000000000";

// TEST: TOOL-U-041
/// A key for an exact address (ADR-0037 Amendment 2): git reads an IPv6 address only in brackets in a URL, and an IPv4
/// address as it is.
#[tokio::test]
async fn tool_u_041_git_finds_a_key_for_an_exact_address() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = &support::context(&fixture);
    let keyed = Shell::new().with_host_keys(&[
        HostKeyGrant::new(
            "INTERNAL_GIT".into(),
            ("fd00::5".into(), 8443, Some("/team".into())),
            ("basic".into(), None),
            ADDRESSED.into(),
        )?,
        HostKeyGrant::new(
            "INTERNAL_API".into(),
            ("192.0.2.10".into(), 443, None),
            ("bearer".into(), None),
            BY_V4.into(),
        )?,
    ]);
    let fill = |url: &str| format!("printf 'url={url}\\n\\n' | {GIT} credential fill 2>&1 || true");
    let inside = run(&keyed, cx, &fill("https://[fd00::5]:8443/team/repo.git")).await?;
    assert!(
        inside.contains(&format!("password={ADDRESSED}")),
        "{inside}"
    );
    for other in [
        "https://[fd00::6]:8443/team/repo.git",
        "https://[fd00::5]/team/repo.git",
    ] {
        let outside = run(&keyed, cx, &fill(other)).await?;
        assert!(!outside.contains(ADDRESSED), "{other}: {outside}");
    }
    let header = run(
        &keyed,
        cx,
        &format!("{GIT} config --get-urlmatch http.extraHeader https://192.0.2.10/v1/repo.git"),
    )
    .await?;
    assert_eq!(header.trim(), format!("Authorization: Bearer {BY_V4}"));
    Ok(())
}
