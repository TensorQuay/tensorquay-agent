//! Collects the run environment (TEST-PLAN §15.1) from git, Docker, `/proc` and the route summary.
//! Whether the route uses native tools is declared by the operator: the public API does not expose it.
use super::result::{Environment, Host};
use crate::client::RouteSummary;
use std::{collections::BTreeMap, io, path::Path};
use tokio::process::Command;

/// Where the environment comes from.
#[derive(Debug)]
pub struct Sources<'a> {
    /// The repository checkout the stack was built from.
    pub repository: &'a Path,
    /// Image references by role, for example `("runtime", "tqa-runtime:base")`.
    pub images: &'a [(&'a str, &'a str)],
    /// The route under test.
    pub route: &'a RouteSummary,
    /// Whether that route uses native tool calls, as configured.
    pub native_tools: bool,
    /// Whether to record the GPU (a local model).
    pub gpu: bool,
    /// The driver flags, with paths but never secrets.
    pub flags: Vec<String>,
}

/// Collects the environment; the result writer validates it.
///
/// # Errors
/// A command that cannot start or fails, or an unreadable `/proc` file.
pub async fn collect(sources: Sources<'_>) -> io::Result<Environment> {
    let repository = sources.repository.as_os_str().to_owned();
    let git = |args: &[&str]| {
        let mut command = Command::new("git");
        command.arg("-C").arg(&repository).args(args);
        output(command)
    };
    let commit = git(&["rev-parse", "HEAD"]).await?;
    let dirty = !git(&["status", "--porcelain"]).await?.is_empty();
    let mut images = BTreeMap::new();
    for (role, reference) in sources.images {
        let mut command = Command::new("docker");
        command.args(["image", "inspect", "--format", "{{.Id}}", reference]);
        images.insert((*role).to_owned(), output(command).await?);
    }
    let mut docker = Command::new("docker");
    docker.args(["version", "--format", "{{.Server.Version}}"]);
    let gpu = if sources.gpu {
        let mut smi = Command::new("nvidia-smi");
        smi.args(["--query-gpu=name", "--format=csv,noheader"]);
        Some(output(smi).await?)
    } else {
        None
    };
    let host = Host {
        cpu: cpu_model(&std::fs::read_to_string("/proc/cpuinfo")?),
        cores: std::thread::available_parallelism()
            .map_or(0, |cores| u32::try_from(cores.get()).unwrap_or(u32::MAX)),
        memory_bytes: memory_bytes(&std::fs::read_to_string("/proc/meminfo")?),
        kernel: std::fs::read_to_string("/proc/sys/kernel/osrelease")?
            .trim()
            .to_owned(),
        docker: output(docker).await?,
        gpu,
    };
    Ok(Environment {
        commit,
        dirty,
        images,
        route: sources.route.name.clone(),
        native_tools: sources.native_tools,
        fallback_prompt_version: tqa_prompt::FALLBACK_PROMPT_VERSION,
        model: sources.route.model.clone(),
        host,
        driver_version: env!("CARGO_PKG_VERSION").to_owned(),
        flags: sources.flags,
    })
}

async fn output(mut command: Command) -> io::Result<String> {
    let output = command.output().await?;
    if !output.status.success() {
        return Err(io::Error::other("an environment command failed"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// The first `model name` of `/proc/cpuinfo`; empty when absent, which validation refuses.
fn cpu_model(cpuinfo: &str) -> String {
    cpuinfo
        .lines()
        .find_map(|line| {
            line.strip_prefix("model name")
                .and_then(|rest| rest.split_once(':'))
        })
        .map_or_else(String::new, |(_, name)| name.trim().to_owned())
}

/// `MemTotal` of `/proc/meminfo` in bytes; zero when absent, which validation refuses.
fn memory_bytes(meminfo: &str) -> u64 {
    meminfo
        .lines()
        .find_map(|line| line.strip_prefix("MemTotal:"))
        .and_then(|rest| rest.trim().strip_suffix("kB"))
        .and_then(|kib| kib.trim().parse::<u64>().ok())
        .map_or(0, |kib| kib.saturating_mul(1024))
}

#[cfg(test)]
mod tests;
