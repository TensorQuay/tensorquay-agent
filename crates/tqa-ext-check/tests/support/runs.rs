//! Each `ext-check` run names what it creates after its own process ID (`tqa-extcheck-<pid>-…`,
//! `tqa-extcheck-mcp-<pid>-…`), so a test that records its runs checks for its own leftovers only. Tests run in parallel
//! and must not count one another's live resources (TK-I-002, TK-I-003).
use std::{
    io::Write as _,
    process::{Command, Output, Stdio},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Runs `command` to completion, recording its process ID in `runs`.
pub fn run(command: &mut Command, runs: &mut Vec<u32>) -> Result<Output> {
    let child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    runs.push(child.id());
    Ok(child.wait_with_output()?)
}

/// The IDs of the `kinds` (container, network, volume) whose names start with `prefix` and one of `runs`' IDs.
pub fn leftover(prefix: &str, runs: &[u32], kinds: &[&str]) -> Result<String> {
    if runs.is_empty() {
        return Err("no run was recorded, so nothing would be checked".into());
    }
    let mut left = Vec::new();
    for pid in runs {
        for kind in kinds {
            let mut command = Command::new("docker");
            command.args([
                *kind,
                "ls",
                "-q",
                "--filter",
                &format!("name={prefix}{pid}-"),
            ]);
            if *kind == "container" {
                command.arg("-a");
            }
            let output = command.output()?;
            if !output.status.success() {
                return Err(format!("docker {kind} ls failed").into());
            }
            left.extend(
                String::from_utf8(output.stdout)?
                    .split_whitespace()
                    .map(str::to_owned),
            );
        }
    }
    // Containers of other runs (a test running in parallel, or an earlier run's leftover) are shown, not counted.
    let decoy = format!("{prefix}0-decoy-");
    let others: Vec<String> = containers(&[format!("name={prefix}")])?
        .into_iter()
        .filter(|id| !left.iter().any(|own| id.starts_with(own.as_str())))
        .collect();
    let names = Command::new("docker")
        .args(["inspect", "--format", "{{.Id}} {{.Name}}"])
        .args(&others)
        .output()?;
    let others: Vec<String> = String::from_utf8(names.stdout)?
        .lines()
        .filter(|line| !line.contains(&decoy))
        .filter_map(|line| line.split_whitespace().next().map(str::to_owned))
        .collect();
    note("not this test's", &others)?;
    Ok(left.join("\n"))
}

/// A created, never started container named like another run's (process ID 0, which no run has): what a test running
/// in parallel leaves live. Removed when dropped.
pub struct Decoy(String);

impl Decoy {
    pub fn new(prefix: &str, image: &str) -> Result<Self> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let name = format!("{prefix}0-decoy-{}-{nanos}", std::process::id());
        let output = Command::new("docker")
            .args(["create", "--name", &name, "--network", "none", image])
            .output()?;
        if !output.status.success() {
            return Err(format!("creating the decoy {name} failed").into());
        }
        Ok(Self(name))
    }
}

impl Drop for Decoy {
    fn drop(&mut self) {
        let _ = Command::new("docker").args(["rm", "-f", &self.0]).output();
    }
}

/// The full IDs of all containers, running or not, matching every `docker ps` filter in `filters`.
pub fn containers(filters: &[String]) -> Result<Vec<String>> {
    let mut command = Command::new("docker");
    command.args(["ps", "-a", "-q", "--no-trunc"]);
    for filter in filters {
        command.args(["--filter", filter]);
    }
    let output = command.output()?;
    if !output.status.success() {
        return Err("docker ps failed".into());
    }
    Ok(String::from_utf8(output.stdout)?
        .split_whitespace()
        .map(str::to_owned)
        .collect())
}

/// Prints each of `ids` as `<label>, from <creation time>: <id> <name> (<state>)`, on stderr; they don't fail the
/// test. A container that is gone by now is skipped.
pub fn note(label: &str, ids: &[String]) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let output = Command::new("docker")
        .args([
            "inspect",
            "--format",
            "{{.Created}} {{.Id}} {{.Name}} {{.State.Status}}",
        ])
        .args(ids)
        .output()?;
    for line in String::from_utf8(output.stdout)?.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if let [created, id, name, state] = fields[..] {
            writeln!(
                std::io::stderr(),
                "{label}, from {created}: {} {} ({state})",
                &id[..12.min(id.len())],
                name.trim_start_matches('/')
            )?;
        }
    }
    Ok(())
}
