//! RT-I-022 (upgrade guard): the sandbox environment an install is documented to configure (`deploy/README.md`, the
//! test stacks' `deploy/tests/compose.py` and the example configuration `deploy/examples/gateway.toml`, which must agree) sets every environment key each runtime image declares,
//! since the backend refuses an image that adds keys the gateway did not choose; and the Workstation image's `go`
//! works in a sandbox started with exactly that environment.
use std::{collections::BTreeMap, time::Duration};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// The image to check: `TQA_RUNTIME_TEST_IMAGE` for Workstation and `TQA_RUNTIME_IMAGE` for Minimal (the base) when
/// set, as the integration run sets both to a candidate's images; otherwise the default tag.
fn image(default: &str) -> String {
    let variable = if default.ends_with(":workstation") {
        "TQA_RUNTIME_TEST_IMAGE"
    } else {
        "TQA_RUNTIME_IMAGE"
    };
    std::env::var(variable)
        .ok()
        .filter(|image| !image.is_empty())
        .unwrap_or_else(|| default.to_owned())
}

/// Runs one Docker command within `timeout` and returns its standard output; a failure carries its output.
async fn docker(arguments: &[&str], timeout: Duration) -> Result<String> {
    let mut command = tokio::process::Command::new("docker");
    command.args(arguments).kill_on_drop(true);
    let output = tokio::time::timeout(timeout, command.output()).await??;
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    if output.status.success() {
        Ok(text)
    } else {
        Err(format!(
            "docker failed: {text}{}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into())
    }
}

/// The `[sandbox.env]` table that follows `marker` in `text` (a TOML string inside a Python literal, so lines are
/// separated by `\\n`), as its `KEY=value` lines up to the first line that is not one.
fn documented(text: &str, marker: &str) -> Result<BTreeMap<String, String>> {
    let start = text
        .find(marker)
        .ok_or(format!("{marker} is not documented"))?
        + marker.len();
    let is_entry = |line: &&str| {
        line.split_once('=').is_some_and(|(key, _)| {
            !key.is_empty()
                && key
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte == b'_')
        })
    };
    Ok(text[start..]
        .split("\\n")
        .take_while(is_entry)
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.trim_matches(['"', '\'']).to_owned()))
        .collect())
}

/// The `[sandbox.env]` table of a TOML file with real line breaks (the example configuration), as `KEY = "value"` lines up
/// to the first line that is not one.
fn configured(text: &str) -> Result<BTreeMap<String, String>> {
    let start = text
        .find("\n[sandbox.env]\n")
        .ok_or("[sandbox.env] is not in the example configuration")?
        + "\n[sandbox.env]\n".len();
    Ok(text[start..]
        .lines()
        .map_while(|line| {
            let (key, value) = line.split_once('=')?;
            let key = key.trim();
            (!key.is_empty()
                && key
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte == b'_'))
            .then(|| {
                (
                    key.to_owned(),
                    value.trim().trim_matches(['"', '\'']).to_owned(),
                )
            })
        })
        .collect())
}

/// The documented environment, checked to be the same in the guide, the test stacks and the example configuration.
fn environment() -> Result<BTreeMap<String, String>> {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../deploy");
    let guide = documented(
        &std::fs::read_to_string(format!("{root}/README.md"))?,
        "[sandbox.env]\\n",
    )?;
    let stacks = documented(
        &std::fs::read_to_string(format!("{root}/tests/compose.py"))?,
        "[sandbox.env]\\n",
    )?;
    assert_eq!(
        guide, stacks,
        "deploy/README.md and deploy/tests/compose.py document the same sandbox environment"
    );
    // An install that starts from the example as-is must get the same environment, or every image is refused.
    let example = configured(&std::fs::read_to_string(format!(
        "{root}/examples/gateway.toml"
    ))?)?;
    assert_eq!(
        guide, example,
        "deploy/examples/gateway.toml sets the documented sandbox environment"
    );
    assert!(!guide.is_empty(), "a documented sandbox environment");
    Ok(guide)
}

// TEST: RT-I-022
#[test]
fn rt_i_022_the_guide_the_stacks_and_the_example_set_the_same_environment() -> Result<()> {
    // Runs without Docker, so a drift shows on every run, not only on the image suites.
    assert_eq!(
        environment()?
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["HOME", "PATH"]
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires Docker and the built tqa-runtime:base and tqa-runtime:workstation images"]
async fn rt_i_022_the_documented_sandbox_environment_admits_every_runtime_image() -> Result<()> {
    let documented = environment()?;
    for default in ["tqa-runtime:base", "tqa-runtime:workstation"] {
        let selected = image(default);
        let declared = docker(
            &[
                "image",
                "inspect",
                "--format",
                "{{json .Config.Env}}",
                &selected,
            ],
            Duration::from_secs(30),
        )
        .await?;
        let declared: Vec<String> = serde_json::from_str(declared.trim())?;
        let missing: Vec<&str> = declared
            .iter()
            .filter_map(|entry| entry.split_once('=').map(|(key, _)| key))
            .filter(|key| !documented.contains_key(*key))
            .collect();
        assert!(
            missing.is_empty(),
            "{selected} declares keys the documented environment does not set: {missing:?}"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires Docker and the built tqa-runtime:workstation image"]
async fn rt_i_022_go_works_in_a_workstation_sandbox_with_the_documented_environment() -> Result<()>
{
    let documented = environment()?;
    let selected = image("tqa-runtime:workstation");
    let mut arguments: Vec<String> = [
        "run",
        "--rm",
        "--network=none",
        "--read-only",
        "--cap-drop=ALL",
        "--security-opt=no-new-privileges:true",
        "--pids-limit=256",
        "--memory=512m",
        "--tmpfs=/tmp:rw,nosuid,nodev,noexec,size=16777216,mode=1777",
        "--tmpfs=/workspace:rw,nosuid,nodev,exec,size=134217728,uid=1000,gid=1000,mode=0700",
        "--entrypoint=/usr/bin/tini",
    ]
    .map(str::to_owned)
    .to_vec();
    // Exactly the documented keys, as the backend sets them; `env -i` drops whatever else the image declares.
    arguments.extend(
        documented
            .iter()
            .map(|(key, value)| format!("--env={key}={value}")),
    );
    arguments.push(selected);
    arguments.extend(["--", "/usr/bin/env", "-i"].map(str::to_owned));
    arguments.extend(
        documented
            .iter()
            .map(|(key, value)| format!("{key}={value}")),
    );
    // As the shell tool runs a command: its own private home under the workspace.
    arguments.extend(
        [
            "HOME=/workspace/.tqa/home",
            "/bin/sh",
            "-c",
            "mkdir -p \"$HOME\" && go version && go env GOTOOLCHAIN",
        ]
        .map(str::to_owned),
    );
    let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let output = docker(&arguments, Duration::from_secs(90)).await?;
    let mut lines = output.lines();
    assert!(
        lines
            .next()
            .is_some_and(|line| line.starts_with("go version go1.")),
        "{output}"
    );
    assert_eq!(
        lines.next(),
        Some("local"),
        "no toolchain download: {output}"
    );
    Ok(())
}

/// Runs `command` with `/bin/sh -c` in `selected`, sandboxed as the backend runs it and with exactly the documented
/// environment (and the shell tool's private home), and returns its standard output.
async fn in_documented_sandbox(selected: &str, command: &str) -> Result<String> {
    let documented = environment()?;
    let mut arguments: Vec<String> = [
        "run",
        "--rm",
        "--network=none",
        "--read-only",
        "--cap-drop=ALL",
        "--security-opt=no-new-privileges:true",
        "--pids-limit=256",
        "--memory=512m",
        "--tmpfs=/tmp:rw,nosuid,nodev,noexec,size=16777216,mode=1777",
        "--tmpfs=/workspace:rw,nosuid,nodev,exec,size=134217728,uid=1000,gid=1000,mode=0700",
        "--entrypoint=/usr/bin/tini",
    ]
    .map(str::to_owned)
    .to_vec();
    arguments.extend(
        documented
            .iter()
            .map(|(key, value)| format!("--env={key}={value}")),
    );
    arguments.push(selected.to_owned());
    arguments.extend(["--", "/usr/bin/env", "-i"].map(str::to_owned));
    arguments.extend(
        documented
            .iter()
            .map(|(key, value)| format!("{key}={value}")),
    );
    arguments.extend(["HOME=/workspace/.tqa/home", "/bin/sh", "-c", command].map(str::to_owned));
    let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
    docker(&arguments, Duration::from_secs(90)).await
}

// TEST: RT-I-035
#[tokio::test]
#[ignore = "requires Docker and the built tqa-runtime:base and tqa-runtime:workstation images"]
async fn rt_i_035_python_is_python3_in_every_runtime_image_and_workstation_has_the_data_libraries()
-> Result<()> {
    // A task, or a model's habit, may say `python`; Debian names it python3. Both built-ins answer to either name.
    for default in ["tqa-runtime:base", "tqa-runtime:workstation"] {
        let selected = image(default);
        let output = in_documented_sandbox(
            &selected,
            "python -c 'import sys; print(sys.version_info[0])' && python3 -c 'print(3)'",
        )
        .await?;
        assert_eq!(
            output.split_whitespace().collect::<Vec<_>>(),
            ["3", "3"],
            "{selected}"
        );
    }
    // F24: Workstation has pandas, matplotlib and openpyxl (Debian's, the owner's licence decision), working headless
    // in the sandbox: a CSV read and summed, a chart saved, a spreadsheet written and read back. Minimal stays small.
    let workstation = image("tqa-runtime:workstation");
    let script = "cd /tmp && printf 'region,total\\nnorth,3\\nsouth,4\\n' > t.csv && MPLBACKEND=Agg python -c \"\
import pandas, matplotlib.pyplot as plot, openpyxl
frame = pandas.read_csv('t.csv')
plot.bar(frame.region, frame.total); plot.savefig('t.png')
book = openpyxl.Workbook(); book.active['A1'] = int(frame.total.sum()); book.save('t.xlsx')
print(openpyxl.load_workbook('t.xlsx').active['A1'].value, open('t.png', 'rb').read(4) == bytes([137, 80, 78, 71]))\"";
    let output = in_documented_sandbox(&workstation, script).await?;
    assert_eq!(
        output.split_whitespace().collect::<Vec<_>>(),
        ["7", "True"],
        "{workstation}: {output}"
    );
    Ok(())
}

/// RT-I-050 (the c14b first hour, F10; PROMPT-U-020): the command the system prompt gives for a PDF, as it is written
/// there, makes one in the Workstation's documented sandbox: Chromium headless, its inner sandbox off as the image runs
/// it, printing HTML from the working folder. A model that follows the prompt gets a PDF, not "no PDF library".
#[tokio::test]
#[ignore = "requires Docker and the built tqa-runtime:workstation image"]
async fn rt_i_050_the_prompts_pdf_command_makes_a_pdf_in_the_workstation() -> Result<()> {
    let workstation = image("tqa-runtime:workstation");
    let script = "cd /tmp && printf '<h1>Q3</h1><table><tr><td>North</td><td>3</td></tr></table>' > in.html \
&& chromium --headless --no-sandbox --print-to-pdf=out.pdf in.html 2>/dev/null; head -c 5 out.pdf";
    let output = in_documented_sandbox(&workstation, script).await?;
    assert_eq!(output.trim(), "%PDF-", "{workstation}: {output}");
    Ok(())
}
