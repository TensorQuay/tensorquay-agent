//! Tool processes are the OOM killer's preferred victims, never the runtime (SBX-C-003, ARCHITECTURE §4.5). Each one
//! starts as `/bin/sh`, raises its own score, then `exec`s the program with its arguments: the same pid, process group
//! and argv as a direct start, so exit codes, signals, quoting and group kills are unchanged.
use std::ffi::OsStr;
use tokio::process::Command;

#[cfg(test)]
#[path = "process_score_tests.rs"]
mod tests;

/// Above the runtime (0 on Docker, -1000 in a microVM), so memory pressure ends a tool, not the run; raising needs no
/// privilege.
pub(crate) const TOOL_OOM_SCORE_ADJ: i32 = 500;

const SCORE_FILE: &str = "/proc/self/oom_score_adj";

/// The shell script that raises the score in `target`, ignoring a failed write (a read-only `/proc`), then runs `"$@"`.
fn raise_and_exec(target: &str) -> String {
    // `2>/dev/null` comes first, so even a failed redirection stays silent.
    format!("printf {TOOL_OOM_SCORE_ADJ} 2>/dev/null >{target}; exec \"$@\"")
}

/// A command for `program` that starts at [`TOOL_OOM_SCORE_ADJ`]; add its arguments with `arg`/`args` as usual. It assumes
/// `/bin/sh`, which every runtime image and microVM rootfs has.
///
/// # Errors
/// `NotFound` when `program` is not an executable file, looked up in `path` (the `PATH` the caller gives the child)
/// unless it contains a `/`. This keeps the error a direct start would give at spawn, because the wrapper itself
/// always starts.
pub fn tool_command(program: impl AsRef<OsStr>, path: Option<&OsStr>) -> std::io::Result<Command> {
    let program = program.as_ref();
    resolve(program, path)?;
    let mut command = Command::new("/bin/sh");
    command
        .arg("-c")
        .arg(raise_and_exec(SCORE_FILE))
        .arg("sh")
        .arg(program);
    Ok(command)
}

/// Finds `program` as a direct start would: as given when it contains a `/`, else in each directory of `path`.
fn resolve(program: &OsStr, path: Option<&OsStr>) -> std::io::Result<()> {
    use std::os::unix::{ffi::OsStrExt, fs::PermissionsExt};
    let executable = |candidate: &std::path::Path| {
        std::fs::metadata(candidate)
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
    };
    let found = if program.as_bytes().contains(&b'/') {
        executable(std::path::Path::new(program))
    } else {
        // An empty entry means the child's current directory, which this process cannot see; it is skipped.
        path.is_some_and(|path| {
            std::env::split_paths(path)
                .filter(|directory| !directory.as_os_str().is_empty())
                .any(|directory| executable(&directory.join(program)))
        })
    };
    if found {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "the program is not available",
        ))
    }
}
