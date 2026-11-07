//! TOOL-U-025: tool processes are the OOM killer's preferred victims; the command itself runs exactly as before.
use super::{TOOL_OOM_SCORE_ADJ, raise_and_exec, tool_command};
use std::process::Output;

type Result = std::result::Result<(), Box<dyn std::error::Error>>;

async fn run(mut command: tokio::process::Command) -> std::io::Result<Output> {
    command.output().await
}

fn own_score() -> std::io::Result<String> {
    std::fs::read_to_string("/proc/self/oom_score_adj").map(|text| text.trim().to_owned())
}

#[tokio::test]
async fn tool_u_025_a_tool_process_runs_at_the_raised_score_and_the_runtime_keeps_its_own() -> Result
{
    let before = own_score()?;
    let mut command = tool_command("sh", Some(std::ffi::OsStr::new("/usr/bin:/bin")))?;
    command.args(["-c", "cat /proc/self/oom_score_adj"]);
    let output = run(command).await?;
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout)?.trim(),
        TOOL_OOM_SCORE_ADJ.to_string()
    );
    assert_eq!(own_score()?, before, "the runtime's own score is unchanged");
    Ok(())
}

#[tokio::test]
async fn tool_u_025_the_wrapped_command_behaves_exactly_like_sh_c() -> Result {
    let cases = [
        r#"printf '%s|%s' "$0" 'a "quoted" arg'"#,
        "echo out; echo err >&2; exit 7",
        "exit 0",
        "no-such-command-anywhere",
        "/etc",
        "kill -TERM $$",
        "echo $$ $(ps -o pgid= -p $$ 2>/dev/null | tr -d ' ')",
    ];
    for case in cases {
        let mut direct = tokio::process::Command::new("sh");
        direct.args(["-c", case]);
        let mut wrapped = tool_command("sh", Some(std::ffi::OsStr::new("/usr/bin:/bin")))?;
        wrapped.args(["-c", case]);
        let (direct, wrapped) = (run(direct).await?, run(wrapped).await?);
        assert_eq!(wrapped.status.code(), direct.status.code(), "{case}");
        assert_eq!(
            std::os::unix::process::ExitStatusExt::signal(&wrapped.status),
            std::os::unix::process::ExitStatusExt::signal(&direct.status),
            "{case}"
        );
        if !case.contains("$$") {
            assert_eq!(wrapped.stdout, direct.stdout, "{case}");
            assert_eq!(
                String::from_utf8_lossy(&wrapped.stderr),
                String::from_utf8_lossy(&direct.stderr),
                "{case}"
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn tool_u_025_the_exec_keeps_the_pid_and_the_process_group() -> Result {
    let mut command = tool_command("sh", Some(std::ffi::OsStr::new("/usr/bin:/bin")))?;
    command
        .args([
            "-c",
            "echo $$; ps -o pgid= -p $$ 2>/dev/null || cat /proc/$$/stat",
        ])
        .process_group(0);
    let child = command.stdout(std::process::Stdio::piped()).spawn()?;
    let pid = child.id().ok_or("pid")?;
    let output = child.wait_with_output().await?;
    let text = String::from_utf8(output.stdout)?;
    assert_eq!(
        text.lines().next(),
        Some(pid.to_string().as_str()),
        "the command is the spawned process itself"
    );
    assert!(
        text.lines().skip(1).any(|line| line
            .split_whitespace()
            .any(|field| field == pid.to_string())),
        "it leads its own process group: {text}"
    );
    Ok(())
}

#[tokio::test]
async fn tool_u_025_a_failed_score_write_changes_nothing_the_command_shows() -> Result {
    let script = raise_and_exec("/nonexistent/oom_score_adj");
    let mut command = tokio::process::Command::new("/bin/sh");
    command.args([
        "-c",
        &script,
        "sh",
        "sh",
        "-c",
        "echo out; echo err >&2; exit 3",
    ]);
    let output = run(command).await?;
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(output.stdout, b"out\n");
    assert_eq!(output.stderr, b"err\n", "the failed write is silent");
    Ok(())
}

#[test]
fn tool_u_025_a_missing_program_is_not_found_before_anything_starts() -> Result {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir()?;
    let bin = directory.path().join("bin");
    std::fs::create_dir(&bin)?;
    std::fs::write(bin.join("tool"), "#!/bin/sh\n")?;
    std::fs::set_permissions(bin.join("tool"), std::fs::Permissions::from_mode(0o755))?;
    std::fs::write(bin.join("plain"), "not executable")?;
    std::fs::create_dir(bin.join("folder"))?;
    let path = std::ffi::OsString::from(format!("/nonexistent:{}", bin.display()));
    assert!(
        tool_command("tool", Some(&path)).is_ok(),
        "found on the caller's PATH"
    );
    assert!(
        tool_command(bin.join("tool"), None).is_ok(),
        "an absolute path needs no PATH"
    );
    for (program, path) in [
        ("tool", None),
        ("absent", Some(path.as_os_str())),
        ("plain", Some(path.as_os_str())),
        ("folder", Some(path.as_os_str())),
    ] {
        let error = tool_command(program, path).err().ok_or(program)?;
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound, "{program}");
    }
    let error = tool_command(bin.join("absent"), None)
        .err()
        .ok_or("absolute")?;
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    Ok(())
}

#[test]
fn tool_u_025_an_empty_path_entry_is_not_the_runtimes_directory() -> Result {
    use std::os::unix::fs::PermissionsExt;
    // An empty entry means "the current directory" to a direct start, which is the child's, not the runtime's: the
    // lookup skips it rather than check the wrong directory.
    let here = std::env::current_dir()?;
    let name = format!("tqa-empty-entry-{}", std::process::id());
    let program = here.join(&name);
    std::fs::write(&program, "#!/bin/sh\n")?;
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))?;
    let found = tool_command(&name, Some(std::ffi::OsStr::new(":/nonexistent")));
    std::fs::remove_file(&program)?;
    assert_eq!(
        found.err().map(|error| error.kind()),
        Some(std::io::ErrorKind::NotFound)
    );
    Ok(())
}
