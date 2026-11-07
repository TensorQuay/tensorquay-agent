//! Sandbox-local shell execution with an owned process group.
mod capture;
pub(crate) mod group;
use super::{arguments, file_io, specification};
use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolSpec};
use async_trait::async_trait;
use capture::Capture;
use group::ProcessGroup;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{process::Stdio, time::Duration};
use tokio::{io::AsyncReadExt, process::Command};

/// The loopback names and addresses a command's proxy variables leave out (TOOL-U-060); the sandbox's environment
/// lists the same (`tqa-sandbox` `docker/config.rs`, SBX-U-018).
const LOOPBACK_BYPASS: &str = "localhost,127.0.0.1,::1";

/// Runs a foreground shell; the surrounding sandbox enforces filesystem isolation.
pub struct Shell {
    spec: ToolSpec,
    proxy: Option<crate::ProxyEndpoint>,
    host_keys: crate::HostKeyEnv,
    ca_bundle: Option<std::path::PathBuf>,
    /// A Desktop session: screen input is the computer tool's (TOOL-U-064).
    desktop: bool,
}
impl Shell {
    /// Constructs the builtin shell schema.
    #[must_use]
    pub fn new() -> Self {
        Self {
            proxy: None,
            host_keys: crate::HostKeyEnv::default(),
            ca_bundle: None,
            desktop: false,
            spec: ToolSpec {
                // A success may have changed files: a repeated failure counts afresh after it (backlog #18).
                changes_workspace: true,
                ..specification(
                    "shell",
                    "Run a command inside the sandbox",
                    json!({
                        "type":"object","required":["command","cwd","timeout_ms"],"additionalProperties":false,
                        "properties":{"command":{"type":"string"},"cwd":{"type":["string","null"]},"timeout_ms":{"type":["integer","null"],"minimum":1}}
                    }),
                    false,
                )
            },
        }
    }
    /// Supplies the validated gateway proxy to children without inheriting ambient proxy values.
    /// The endpoint carries no credential; the gateway host itself bypasses the proxy.
    #[must_use]
    pub fn with_proxy(mut self, proxy: crate::ProxyEndpoint) -> Self {
        self.proxy = Some(proxy);
        self
    }
    /// Points every command's TLS clients at `bundle`, the image's roots plus the install CA (ADR-0037 §6).
    #[must_use]
    pub fn with_ca_bundle(mut self, bundle: std::path::PathBuf) -> Self {
        self.ca_bundle = Some(bundle);
        self
    }
    /// On a Desktop session (ADR-0039): the description and results point screen input to the computer tool
    /// (TOOL-U-064).
    #[must_use]
    pub fn with_desktop(mut self) -> Self {
        self.desktop = true;
        self.spec.description = format!(
            "{} On this desktop, open applications from here if you like, but act on the screen with the computer \
             tool: input sent from the shell isn't checked, counted or shown to the person.",
            self.spec.description
        );
        self
    }
    /// Gives every command the run's host keys (ADR-0037 §3): each key's variable and git's settings for it.
    #[must_use]
    pub fn with_host_keys(mut self, grants: &[tqa_types::HostKeyGrant]) -> Self {
        self.host_keys = crate::HostKeyEnv::new(grants);
        self
    }
}
/// The variables that point Python (`ssl`, `requests`, pip), Node, curl and git at a CA bundle.
pub const CA_BUNDLE_VARIABLES: [&str; 6] = [
    "SSL_CERT_FILE",
    "REQUESTS_CA_BUNDLE",
    "PIP_CERT",
    "NODE_EXTRA_CA_CERTS",
    "CURL_CA_BUNDLE",
    "GIT_SSL_CAINFO",
];
impl Default for Shell {
    fn default() -> Self {
        Self::new()
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    command: String,
    cwd: Option<String>,
    timeout_ms: Option<u64>,
}
#[async_trait]
impl Tool for Shell {
    // Its schema is strict-valid: optional properties are nullable, and null means absent (TOOL-U-034).
    fn null_means_absent(&self) -> bool {
        true
    }
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    async fn call(&self, args: Value, cx: &ToolContext) -> Result<ToolOutput, ToolError> {
        cx.check()?;
        let args: Args = arguments(args)?;
        if args.timeout_ms == Some(0) {
            return Err(ToolError::invalid(
                "/timeout_ms",
                "timeout must be positive",
            ));
        }
        let workspace = cx.workspace.clone();
        let cwd = args.cwd.unwrap_or_else(|| ".".into());
        let cwd = file_io::run(cx, move |_| {
            let path = workspace.resolve(&cwd)?;
            if !path.is_dir() {
                return Err(ToolError::PathInvalid("cwd must be a directory".into()));
            }
            Ok(path)
        })
        .await?;
        let timeout = args.timeout_ms.map_or(cx.limits.shell_timeout, |ms| {
            Duration::from_millis(ms).min(cx.limits.shell_timeout)
        });
        if cx.clock.now().checked_add(timeout).is_none() {
            return Err(ToolError::invalid("/timeout_ms", "deadline overflow"));
        }
        let screen_input = self.desktop && names_screen_input(&args.command);
        // Looked up on the PATH the child gets (environment() passes the runtime's), as a direct start would.
        let mut command =
            crate::process_score::tool_command("sh", std::env::var_os("PATH").as_deref())?;
        command
            .arg("-c")
            .arg(args.command)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .process_group(0);
        environment(&mut command, cx).await?;
        // The run's keys and git's settings, the repository-safety ones first (ADR-0035 §1, ADR-0037 §7).
        command.envs(self.host_keys.variables());
        if let Some(bundle) = &self.ca_bundle {
            for name in CA_BUNDLE_VARIABLES {
                command.env(name, bundle);
            }
        }
        if let Some(proxy) = &self.proxy {
            for name in ["HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy"] {
                command.env(name, proxy.as_str());
            }
            // Loopback never goes through the proxy: a task reaches its own local servers (TOOL-U-060, c16k B10).
            let bypass = format!("{},{LOOPBACK_BYPASS}", proxy.host());
            command.env("NO_PROXY", &bypass);
            command.env("no_proxy", &bypass);
            // Node's fetch and https ignore the variables above unless told (Node 24's env proxy; G9c, SBX-C-019).
            command.env("NODE_USE_ENV_PROXY", "1");
        }
        cx.check()?;
        let mut group = ProcessGroup::spawn(&mut command)?;
        let result = tokio::select! {
            biased;
            () = cx.cancel.cancelled() => Err(ToolError::Cancelled),
            () = cx.sleeper.sleep(timeout) => Err(ToolError::Timeout),
            result = tokio::time::timeout(timeout, collect(&mut group, cx.limits.shell_output_bytes)) => result.unwrap_or(Err(ToolError::Timeout)),
        };
        let mut output = outcome(result, group.finish(cx.sleeper.as_ref()).await)?;
        if screen_input {
            output.text.push_str(SCREEN_INPUT_NOTE);
        }
        Ok(output)
    }
}

/// What a Desktop session's shell result ends with when its command names an X input tool (TOOL-U-064, c16k real-world
/// A10 and A12b). A nudge, not a control: the command has run.
const SCREEN_INPUT_NOTE: &str = "\n[the desktop's screen is the computer tool's: input sent from the shell isn't \
                                 checked, counted or shown to the person]";
/// X input tools by name: a whole word of the command (`xdotool.md` and `xdotoolbox` aren't).
const SCREEN_INPUT_TOOLS: [&str; 5] = ["xdotool", "xte", "ydotool", "pyautogui", "XTest"];

fn names_screen_input(command: &str) -> bool {
    command
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-'))
        .filter_map(|word| word.rsplit('/').next())
        .any(|word| SCREEN_INPUT_TOOLS.contains(&word))
}
async fn environment(command: &mut Command, cx: &ToolContext) -> Result<(), ToolError> {
    command.env_clear();
    for name in ["PATH", "LANG", "TERM"]
        .into_iter()
        .chain(cx.env_allow.iter().map(String::as_str))
    {
        if name.is_empty() || name.contains(['=', '\0']) {
            return Err(ToolError::invalid("/env_allow", "invalid environment name"));
        }
        if name != "HOME"
            && let Some(value) = std::env::var_os(name)
        {
            command.env(name, value);
        }
    }
    let workspace = cx.workspace.clone();
    let home = super::file_io::run(cx, move |cancel| {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        super::file_io::cancelled(cancel)?;
        let home = workspace.resolve(".tqa/home")?;
        if home != workspace.path().join(".tqa/home") {
            return Err(ToolError::PathInvalid(
                "shell home must not be symlinked".into(),
            ));
        }
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&home)?;
        std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700))?;
        Ok(home)
    })
    .await?;
    command.env("HOME", home);
    Ok(())
}
async fn collect(group: &mut ProcessGroup, max: usize) -> Result<ToolOutput, ToolError> {
    let mut stdout = group
        .child
        .stdout
        .take()
        .ok_or_else(|| ToolError::Io("stdout pipe missing".into()))?;
    let mut stderr = group
        .child
        .stderr
        .take()
        .ok_or_else(|| ToolError::Io("stderr pipe missing".into()))?;
    let (mut out, mut err) = ([0; 8192], [0; 8192]);
    let (mut out_open, mut err_open) = (true, true);
    let mut captured = Capture::new(max);
    let mut status = None;
    let mut deadline = tokio::time::Instant::now();
    let mut background = false;
    while out_open || err_open || status.is_none() {
        tokio::select! {
            result = group.child.wait(), if status.is_none() => {
                status = Some(result?);
                group.reaped = true;
                background = group.has_members()?;
                deadline = tokio::time::Instant::now() + Duration::from_millis(250);
            }
            () = tokio::time::sleep_until(deadline), if status.is_some() => {
                background = true;
                break;
            }
            result = stdout.read(&mut out), if out_open => {
                let count = result?;
                out_open = count != 0;
                captured.push(&out[..count], false);
            }
            result = stderr.read(&mut err), if err_open => {
                let count = result?;
                err_open = count != 0;
                captured.push(&err[..count], true);
            }
        }
    }
    let mut output = ToolOutput::text(if background {
        captured.finish_with_notice("[background processes were stopped when the command returned]")
    } else {
        captured.finish()
    });
    output.exit_code = status.and_then(|status| status.code());
    Ok(output)
}

fn outcome(
    primary: Result<ToolOutput, ToolError>,
    cleanup: Result<(), ToolError>,
) -> Result<ToolOutput, ToolError> {
    if let Err(error) = &cleanup {
        tracing::warn!(code = error.code(), "process group cleanup failed");
    }
    match primary {
        Err(error) => Err(error),
        Ok(output) => cleanup.map(|()| output),
    }
}

#[cfg(test)]
mod tests;
