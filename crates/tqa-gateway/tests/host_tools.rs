//! SEC-041: the gateway never runs git or any other tool on the host (ARCHITECTURE §12: an agent can plant code that
//! git would run, through hooks, `core.fsmonitor`, `core.sshCommand`, or filter and diff drivers; clone, commit and push
//! run inside a sandbox for the workspace). Two source-level checks, no Docker needed:
//! - the gateway image's final stage is `FROM scratch` holding only the static binary: no git, no shell, nothing to run;
//! - no gateway source starts a process, directly or through the `tqa-tools` items that do (the cloner, the push
//!   process, the shell and its process group).
use std::path::{Path, PathBuf};

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn sec_041_the_gateway_image_holds_only_its_binary() -> Result<(), Box<dyn std::error::Error>> {
    let dockerfile = std::fs::read_to_string(repository().join("images/gateway/Dockerfile"))?;
    let instructions: Vec<&str> = dockerfile
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    let last_stage = instructions
        .iter()
        .rposition(|line| line.to_ascii_uppercase().starts_with("FROM "))
        .ok_or("no FROM")?;
    assert_eq!(
        instructions[last_stage], "FROM scratch",
        "the final stage must be empty but for the binary"
    );
    let files: Vec<&&str> = instructions[last_stage + 1..]
        .iter()
        .filter(|line| {
            let keyword = line
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_ascii_uppercase();
            matches!(keyword.as_str(), "COPY" | "ADD" | "RUN")
        })
        .collect();
    assert_eq!(
        files,
        [&"COPY --from=build /build/target/release/tqa-gateway /tqa-gateway"],
        "the final stage adds exactly the gateway binary"
    );
    Ok(())
}

/// Every `.rs` file under `directory`.
fn sources(directory: &Path, found: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            sources(&path, found)?;
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
    Ok(())
}

/// What starts a process: the standard and Tokio process APIs, raw exec, and the `tqa-tools` items that run one.
const PROCESS: [&str; 10] = [
    "std::process::Command",
    "process::Command",
    "tokio::process",
    "Command::new",
    "process::Stdio",
    "execv",
    "posix_spawn",
    "GitCloner",
    "builtin::shell",
    "ProcessGroup",
];

#[test]
fn sec_041_no_gateway_source_starts_a_process() -> Result<(), Box<dyn std::error::Error>> {
    let mut files = Vec::new();
    sources(&repository().join("crates/tqa-gateway/src"), &mut files)?;
    assert!(
        files.len() > 50,
        "the source walk found only {} files",
        files.len()
    );
    let mut found = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file)?;
        for (number, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            for name in PROCESS.iter().filter(|name| code.contains(*name)) {
                found.push(format!("{}:{}: {name}", file.display(), number + 1));
            }
        }
    }
    assert!(
        found.is_empty(),
        "the gateway must not start processes on the host:\n{}",
        found.join("\n")
    );
    Ok(())
}
