//! Host metadata for the performance protocol (TEST-PLAN §15.1): a result without its environment is not a result.
#![allow(dead_code, reason = "Each benchmark binary uses the parts it needs.")]
use serde_json::{Value, json};
use std::{error::Error, process::Command};

/// Runs a metadata command with a fixed locale and returns its trimmed output.
///
/// # Errors
/// A command that cannot start or fails.
pub fn command(program: &str, arguments: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = Command::new(program)
        .args(arguments)
        .env("LC_ALL", "C")
        .output()?;
    if !output.status.success() {
        return Err(format!("performance metadata command failed: {program}").into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

pub fn load() -> Result<String, Box<dyn Error>> {
    command("uptime", &[])
}

pub fn environment() -> Result<Value, Box<dyn Error>> {
    let (host_model, cpu, memory_bytes, os_version) = if cfg!(target_os = "macos") {
        (
            command("sysctl", &["-n", "hw.model"])?,
            command("sysctl", &["-n", "machdep.cpu.brand_string"])?,
            command("sysctl", &["-n", "hw.memsize"])?.parse::<u64>()?,
            command("sw_vers", &["-productVersion"])?,
        )
    } else {
        let cpu = command("lscpu", &["--json"])?;
        let value: Value = serde_json::from_str(&cpu)?;
        let model = value["lscpu"]
            .as_array()
            .and_then(|rows| rows.iter().find(|row| row["field"] == "Model name:"))
            .and_then(|row| row["data"].as_str())
            .ok_or("missing CPU model")?
            .to_owned();
        (
            "unavailable".into(),
            model,
            command("getconf", &["_PHYS_PAGES"])?
                .parse::<u64>()?
                .checked_mul(command("getconf", &["PAGE_SIZE"])?.parse::<u64>()?)
                .ok_or("memory overflow")?,
            command("uname", &["-sr"])?,
        )
    };
    Ok(json!({"utc":command("date", &["-u", "+%Y%m%dT%H%M%SZ"])?,
        "os":std::env::consts::OS, "os_version":os_version, "arch":std::env::consts::ARCH,
        "kernel":command("uname", &["-sr"])?, "host_model":host_model, "cpu":cpu,
        "logical_cores":std::thread::available_parallelism()?.get(), "memory_bytes":memory_bytes,
        "commit":command("git", &["rev-parse", "HEAD"])?,
        "dirty_tree":!command("git", &["status", "--porcelain"])?.is_empty(),
        "toolchain":command("rustc", &["--version"])?, "cargo":command("cargo", &["--version"])?}))
}
