//! Runs a checked invocation: the server's placement, the checks and the report (TK-U-014).
use std::{collections::BTreeMap, path::Path, process::Stdio, time::Duration};

use secrecy::ExposeSecret;
use serde_json::{Value, json};
use tqa_types::McpServer;

use crate::{
    cli::{DecisionArgs, Place, StdioArgs, TemplateArgs},
    mcp::{Limits, check_stdio, load_calls},
    report::{Check, Report, Status, Target},
};

/// The time a server's container gets to start, on top of the runtime's handshake bound.
const TEMPLATE_STARTUP: Duration = Duration::from_secs(20);
/// How often, and how far apart, a container's removal is retried (a start still in flight can recreate it).
const REMOVE_TRIES: u32 = 10;
const REMOVE_INTERVAL: Duration = Duration::from_millis(500);

/// Where `unshare` may live; the first that exists is used.
const UNSHARE: [&str; 2] = ["/usr/bin/unshare", "/bin/unshare"];

/// Why a run could not produce a report.
#[derive(Debug, Eq, PartialEq)]
pub enum RunError {
    /// The invocation is unusable (exit 2).
    Usage(String),
    /// This machine can't run the checks (exit 3).
    Environment(String),
}

/// Who made the report, and when.
#[derive(Clone, Debug)]
pub struct Provenance {
    /// The repository commit ext-check was built from.
    pub commit: String,
    /// The time of the run, RFC 3339.
    pub date: String,
}

/// Checks one stdio server and returns its report, written to `--json` when asked.
///
/// # Errors
/// Returns a usage error for an unusable fixture file, and an environment error when the server can't be placed or the
/// report can't be written.
pub fn run_stdio(args: &StdioArgs, provenance: &Provenance) -> Result<Report, RunError> {
    let calls = match &args.calls {
        Some(path) => load_calls(path).map_err(RunError::Usage)?,
        None => Vec::new(),
    };
    let env_names = args.env_names().into_iter().map(str::to_owned).collect();
    let (server, target, container) = match &args.place {
        Place::Host { allow_network } => (
            host_server(args, *allow_network)?,
            Target::stdio_host(
                &args.name,
                &args.command,
                &args.args,
                env_names,
                *allow_network,
            ),
            None,
        ),
        Place::Template(image) => {
            let (server, container) = template_server(args, image)?;
            (
                server,
                Target::stdio_template(&args.name, &args.command, &args.args, env_names, image),
                Some(container),
            )
        }
    };
    let workspace =
        tempfile::tempdir().map_err(|_| RunError::Environment("no temporary workspace".into()))?;
    let mut report = Report::new(target, &provenance.commit, &provenance.date);
    // A container's start is the engine's time, not the server's: a template run gets it on top of the runtime's bound.
    let startup = if container.is_some() {
        TEMPLATE_STARTUP
    } else {
        Duration::ZERO
    };
    let limits = Limits {
        handshake: tqa_runtime::mcp::HANDSHAKE + startup,
        call_timeout: Duration::from_secs(u64::from(args.call_timeout_seconds)),
    };
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|_| RunError::Environment("no async runtime".into()))?
        .block_on(check_stdio(
            &server,
            workspace.path(),
            &calls,
            &limits,
            &mut report,
        ));
    if let Some(container) = container {
        remove_container(&container);
    }
    if let Some(path) = &args.json {
        write_report(&report, path)?;
    }
    Ok(report)
}

/// The server in a container from the template image, placed as a sandbox would run it: never pulled, no network, the
/// image's own user (the sandbox user when it names none or root), a read-only root, no capabilities, `no_new_privs`, a
/// tmpfs `/workspace` as its working directory, and only the environment given (by name, so values stay off the command
/// line). The runtime's client talks to it through `docker run -i`. Returns the server and the container's name.
fn template_server(args: &StdioArgs, image: &str) -> Result<(McpServer, String), RunError> {
    let docker = on_path("docker")
        .ok_or_else(|| RunError::Environment("no docker client on PATH".into()))?;
    let inspected = std::process::Command::new(&docker)
        .args(["image", "inspect", "--format", "{{.Config.User}}", image])
        .output()
        .map_err(|_| RunError::Environment("the docker client could not run".into()))?;
    if !inspected.status.success() {
        return Err(RunError::Environment(
            "the template image is not on this engine; ext-check never pulls".into(),
        ));
    }
    let user = String::from_utf8_lossy(&inspected.stdout).trim().to_owned();
    let user =
        if user.is_empty() || user == "0" || user.starts_with("0:") || user.starts_with("root") {
            "1000:1000".to_owned()
        } else {
            user
        };
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let container = format!("tqa-extcheck-mcp-{}-{nanos}", std::process::id());
    let mut run: Vec<String> = [
        "run",
        "-i",
        "--pull",
        "never",
        "--network",
        "none",
        "--read-only",
        "--cap-drop",
        "ALL",
        "--security-opt",
        "no-new-privileges",
        "--user",
        &user,
        "--tmpfs",
        "/workspace:rw,exec,mode=1777",
        "--workdir",
        "/workspace",
        "--name",
        &container,
    ]
    .iter()
    .map(|arg| (*arg).to_owned())
    .collect();
    for name in args.env.keys() {
        run.extend(["-e".to_owned(), name.clone()]);
    }
    run.extend([
        "--entrypoint".to_owned(),
        args.command.clone(),
        image.to_owned(),
    ]);
    run.extend(args.args.iter().cloned());
    // The docker client's own environment: the values given, and how to reach this user's engine.
    let mut env: serde_json::Map<String, Value> = args
        .env
        .iter()
        .map(|(name, value)| (name.clone(), Value::from(value.expose_secret())))
        .collect();
    for name in ["DOCKER_HOST", "XDG_RUNTIME_DIR"] {
        if let Ok(value) = std::env::var(name) {
            env.insert(name.to_owned(), Value::from(value));
        }
    }
    let server = serde_json::from_value(json!({
        "name": args.name,
        "transport": {"kind": "stdio", "command": docker.to_string_lossy(), "args": run, "env": env},
        "call_timeout_seconds": args.call_timeout_seconds,
    }))
    .map_err(|_| RunError::Usage("the server's arguments exceed the runtime's bounds".into()))?;
    Ok((server, container))
}

/// Removes the server's container (not run with `--rm`, so the client exits as soon as the server does and an exit
/// on closed input is timed as the server's own). A client stopped mid-start can leave the
/// engine creating it after a first removal, so removal repeats until the engine no longer knows the name.
fn remove_container(container: &str) {
    let Some(docker) = on_path("docker") else {
        return;
    };
    let quiet = |args: &[&str]| {
        std::process::Command::new(&docker)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    };
    for _ in 0..REMOVE_TRIES {
        let _ = quiet(&["rm", "-f", container]);
        if !quiet(&["container", "inspect", container]) {
            return;
        }
        std::thread::sleep(REMOVE_INTERVAL);
    }
}

/// The first executable named `name` on PATH.
fn on_path(name: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|directory| directory.join(name))
            .find(|candidate| candidate.is_file())
    })
}

/// The template check's refusal for credential files in the image's own layers, naming their paths and rules.
fn secret_files_check(files: &tqa_sandbox::template::secret_files::SecretFiles) -> Check {
    let listed: Vec<String> = files
        .items
        .iter()
        .map(|item| format!("{} ({})", item.path, item.rule.as_str()))
        .collect();
    Check::new(
        "image_secret_file",
        Status::Fail,
        &format!(
            "credential files in the image's own layers ({} in all): {}; remove them, or acknowledge them in the \
             install file's template entry",
            files.total,
            listed.join(", ")
        ),
    )
}

/// The template check's refusal for own layers past a bound of the credential-file scan, naming the setting.
fn scan_limit_check(bound: tqa_sandbox::template::secret_files::Bound, limit: u64) -> Check {
    Check::new(
        "image_scan_limit",
        Status::Fail,
        &format!(
            "the image's own layers exceed the credential-file scan's {} bound ({limit}); raise [template_checks] \
             max_scan_{} in the install file to admit it",
            bound.as_str(),
            bound.as_str()
        ),
    )
}

/// Checks one template image as the gateway's template checker would (ADR-0032), with the install's configuration.
///
/// # Errors
/// Returns a usage error for an unreadable or refused configuration, and an environment error when the engine can't
/// answer or the report can't be written.
pub fn run_template(args: &TemplateArgs, provenance: &Provenance) -> Result<Report, RunError> {
    let source = std::fs::read_to_string(&args.gateway_config)
        .map_err(|_| RunError::Usage("the --gateway-config file cannot be read".into()))?;
    let config = tqa_gateway::GatewayConfig::from_toml(&source).map_err(|error| {
        RunError::Usage(format!("the gateway configuration is refused: {error}"))
    })?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|_| RunError::Environment("no async runtime".into()))?;
    let outcome = runtime.block_on(tqa_gateway::check_template_image(
        &config,
        &args.image,
        None,
    ));
    let mut report = Report::new(
        Target::template(&args.image),
        &provenance.commit,
        &provenance.date,
    );
    match outcome {
        // A developer's image has no install-file entry, so nothing is acknowledged and there are no warnings.
        tqa_gateway::CheckOutcome::Verified { base, image_id, .. } => {
            report.push(Check::new(
                "template_checks",
                Status::Pass,
                &format!(
                    "derived from the {base} template; its protocol label, image policy and runtime binary hold"
                ),
            ));
            report.verified_template(&base, &image_id);
            let install = Install::read(&source)?;
            let stub_image = install.template_image(&base).ok_or_else(|| {
                RunError::Environment(
                    "the base template's image is not in the configuration".into(),
                )
            })?;
            let checks = runtime
                .block_on(crate::boot::posture_and_boot(
                    &image_id,
                    &stub_image,
                    &install.sandbox_env,
                ))
                .map_err(|unavailable| RunError::Environment(unavailable.0))?;
            for check in checks {
                report.push(check);
            }
        }
        tqa_gateway::CheckOutcome::SecretFiles(files) => report.push(secret_files_check(&files)),
        tqa_gateway::CheckOutcome::ScanLimit { bound, limit } => {
            report.push(scan_limit_check(bound, limit));
        }
        tqa_gateway::CheckOutcome::Refused(reason) => report.push(Check::new(
            reason.as_str(),
            Status::Fail,
            "the template checks (ADR-0032) refuse this image",
        )),
        tqa_gateway::CheckOutcome::Unavailable => {
            return Err(RunError::Environment(
                "the container engine could not answer the template checks".into(),
            ));
        }
    }
    report.push(Check::new(
        "microvm_rootfs",
        Status::Skip,
        "the microVM rootfs checks run only on a KVM host",
    ));
    if let Some(path) = &args.json {
        write_report(&report, path)?;
    }
    Ok(report)
}

/// What the posture and boot checks need from the install's gateway file, read from the same TOML the gateway's
/// validator accepted: each template's image, and the sandbox environment (values included, as a sandbox gets them).
struct Install {
    templates: BTreeMap<String, String>,
    sandbox_env: BTreeMap<String, String>,
}

impl Install {
    fn read(source: &str) -> Result<Self, RunError> {
        let table: toml::Table = source
            .parse()
            .map_err(|_| RunError::Usage("the gateway configuration is not TOML".into()))?;
        let text = |value: &toml::Value| value.as_str().map(str::to_owned);
        let templates = table
            .get("templates")
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|template| {
                Some((text(template.get("name")?)?, text(template.get("image")?)?))
            })
            .collect();
        let sandbox_env = table
            .get("sandbox")
            .and_then(|sandbox| sandbox.get("env"))
            .and_then(toml::Value::as_table)
            .into_iter()
            .flatten()
            .filter_map(|(name, value)| Some((name.clone(), text(value)?)))
            .collect();
        Ok(Self {
            templates,
            sandbox_env,
        })
    }

    fn template_image(&self, name: &str) -> Option<String> {
        self.templates.get(name).cloned()
    }
}

/// The server as the runtime would start it, inside a new user and network namespace unless the network is allowed.
fn host_server(args: &StdioArgs, allow_network: bool) -> Result<McpServer, RunError> {
    let (command, server_args) = if allow_network {
        (args.command.clone(), args.args.clone())
    } else {
        let unshare = isolated().ok_or_else(|| {
            RunError::Environment(
                "no unprivileged network namespace here; --allow-network runs the server without one".into(),
            )
        })?;
        let mut wrapped = vec!["--user".to_owned(), "--net".to_owned(), "--".to_owned()];
        wrapped.push(args.command.clone());
        wrapped.extend(args.args.iter().cloned());
        (unshare.to_owned(), wrapped)
    };
    let env: serde_json::Map<String, Value> = args
        .env
        .iter()
        .map(|(name, value)| (name.clone(), Value::from(value.expose_secret())))
        .collect();
    serde_json::from_value(json!({
        "name": args.name,
        "transport": {"kind": "stdio", "command": command, "args": server_args, "env": env},
        "call_timeout_seconds": args.call_timeout_seconds,
    }))
    .map_err(|_| RunError::Usage("the server's arguments exceed the runtime's bounds".into()))
}

/// `unshare`, if it can create a user and network namespace here.
fn isolated() -> Option<&'static str> {
    let unshare = UNSHARE.into_iter().find(|path| Path::new(path).exists())?;
    std::process::Command::new(unshare)
        .args(["--user", "--net", "true"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()
        .filter(std::process::ExitStatus::success)
        .map(|_| unshare)
}

/// Checks one decision engine (TK-U-030) and returns its report, written to `--json` when asked. The credential is read
/// from the environment variable the command line named, and goes only into the requests.
///
/// # Errors
/// Returns a usage error when the named variable is unset or empty, and an environment error when there is no async
/// runtime or the report can't be written.
pub fn run_decision(args: &DecisionArgs, provenance: &Provenance) -> Result<Report, RunError> {
    let credential = match &args.bearer_env {
        Some(name) => Some(
            std::env::var(name)
                .ok()
                .filter(|value| !value.is_empty())
                .map(secrecy::SecretString::from)
                .ok_or_else(|| {
                    RunError::Usage("the variable --bearer-from-env names is unset or empty".into())
                })?,
        ),
        None => None,
    };
    let target = Target::engine(
        args.url.as_str(),
        args.timeout_ms,
        credential.is_some(),
        args.network,
    );
    let mut report = Report::new(target, &provenance.commit, &provenance.date);
    let engine = crate::decision::Engine {
        url: args.url.clone(),
        timeout: Duration::from_millis(args.timeout_ms),
        credential,
    };
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|_| RunError::Environment("no async runtime".into()))?
        .block_on(crate::decision::check_engine(&engine, &mut report));
    if let Some(path) = &args.json {
        write_report(&report, path)?;
    }
    Ok(report)
}

fn write_report(report: &Report, path: &Path) -> Result<(), RunError> {
    let mut text = serde_json::to_string_pretty(report)
        .map_err(|_| RunError::Environment("the report could not be encoded".into()))?;
    text.push('\n');
    std::fs::write(path, text)
        .map_err(|_| RunError::Environment("the --json file could not be written".into()))
}
