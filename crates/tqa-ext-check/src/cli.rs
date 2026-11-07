//! The command line (TK-U-014). Parsing is pure: nothing is read or started here. A refusal is a usage
//! error that names the option or field and never echoes a value, since a value may be a secret typed by mistake.
use std::{collections::BTreeMap, fmt, path::PathBuf};

use secrecy::{ExposeSecret, SecretString};

/// The time a decision engine has to answer, as the gateway's engine record allows (ADR-0013): the default and bound.
const DEFAULT_ENGINE_TIMEOUT_MS: u64 = 2_000;
const MAX_ENGINE_TIMEOUT_MS: u64 = 10_000;

/// The longest one call may take, as for a catalogue server (ADR-0011): the default and the bound.
const DEFAULT_CALL_TIMEOUT_SECONDS: u32 = 60;
const MAX_CALL_TIMEOUT_SECONDS: u32 = 600;

/// The usage summary printed with a refusal and for `--help`.
pub const USAGE: &str = "\
usage: ext-check mcp stdio --name <name> (--host [--allow-network] | --template <image>)
                           [--env NAME=VALUE]... [--calls <file.json>] [--call-timeout <seconds>]
                           [--json <report.json>] -- <absolute command> [args...]
       ext-check template <image ID or registry digest> --gateway-config <gateway.toml> [--json <report.json>]
       ext-check catalog lint <catalogue directory>
       ext-check decision <engine base URL> [--network] [--timeout-ms <1-10000>] [--bearer-from-env <VAR>]
                          [--json <report.json>]
           (--bearer-from-env sends the variable's value as a bearer token only; an engine keyed by another
            header is checked without its key)
       ext-check --help";

/// What to run.
#[derive(Debug)]
#[non_exhaustive]
pub enum Invocation {
    /// Print the usage summary.
    Help,
    /// Check one stdio MCP server.
    McpStdio(StdioArgs),
    /// Check one template image.
    Template(TemplateArgs),
    /// Lint the catalogue in this directory.
    CatalogLint(PathBuf),
    /// Check one decision engine.
    Decision(DecisionArgs),
}

/// A checked `decision` invocation.
#[derive(Debug)]
#[non_exhaustive]
pub struct DecisionArgs {
    /// The engine's base URL: the suite posts to `<url>/v1/before_step` and `/v1/after_response`.
    pub url: url::Url,
    /// Whether a non-loopback address is allowed.
    pub network: bool,
    /// The time the engine has for each consultation.
    pub timeout_ms: u64,
    /// The environment variable holding the bearer credential, read when the run starts; never the value.
    pub bearer_env: Option<String>,
    /// Where to write the JSON report.
    pub json: Option<PathBuf>,
}

/// A checked `template` invocation.
#[derive(Debug)]
#[non_exhaustive]
pub struct TemplateArgs {
    /// The image pin: a local image ID or a registry digest, never a tag.
    pub image: String,
    /// The install's gateway configuration: its templates are the bases, its sandbox environment the policy.
    pub gateway_config: PathBuf,
    /// Where to write the JSON report.
    pub json: Option<PathBuf>,
}

/// Where a stdio server runs.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Place {
    /// On this host, inside a new network namespace unless the network is explicitly allowed.
    Host {
        /// Run without a network namespace (recorded in the report).
        allow_network: bool,
    },
    /// In a container from this local template image, without network.
    Template(String),
}

/// A checked `mcp stdio` invocation.
#[derive(Debug)]
#[non_exhaustive]
pub struct StdioArgs {
    /// The server's catalogue name, for its model-facing tool names.
    pub name: String,
    /// Where it runs.
    pub place: Place,
    /// Its absolute command.
    pub command: String,
    /// Its arguments.
    pub args: Vec<String>,
    /// Its environment; values stay secret-wrapped, so `Debug` and the report see names only.
    pub env: BTreeMap<String, SecretString>,
    /// Calls to make, from a fixture file.
    pub calls: Option<PathBuf>,
    /// The bound on each call.
    pub call_timeout_seconds: u32,
    /// Where to write the JSON report.
    pub json: Option<PathBuf>,
}

impl StdioArgs {
    /// The environment's names, sorted.
    #[must_use]
    pub fn env_names(&self) -> Vec<&str> {
        self.env.keys().map(String::as_str).collect()
    }
}

/// A refused command line.
#[derive(Debug, Eq, PartialEq)]
pub struct Usage(String);

impl fmt::Display for Usage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for Usage {}

fn usage(message: impl Into<String>) -> Usage {
    Usage(message.into())
}

/// Parses the arguments after the program name.
///
/// # Errors
/// Returns a usage error naming what is missing, unknown or invalid, without echoing any value.
pub fn parse(args: Vec<String>) -> Result<Invocation, Usage> {
    let mut args = args.into_iter();
    match args.next().as_deref() {
        None => Err(usage("expected a kind: mcp, template, catalog or decision")),
        Some("--help" | "-h") => Ok(Invocation::Help),
        Some("mcp") => match args.next().as_deref() {
            Some("stdio") => stdio(&args.collect::<Vec<_>>()).map(Invocation::McpStdio),
            Some("http") => {
                if args.any(|arg| arg == "--network") {
                    Err(usage("mcp http is not available yet"))
                } else {
                    Err(usage(
                        "mcp http reaches a remote server, so it needs an explicit --network",
                    ))
                }
            }
            _ => Err(usage("expected a transport: mcp stdio")),
        },
        Some("template") => template(&args.collect::<Vec<_>>()).map(Invocation::Template),
        Some("catalog") => match (args.next().as_deref(), args.next(), args.next()) {
            (Some("lint"), Some(dir), None) => Ok(Invocation::CatalogLint(dir.into())),
            _ => Err(usage("expected catalog lint <catalogue directory>")),
        },
        Some("decision") => decision(&args.collect::<Vec<_>>()).map(Invocation::Decision),
        Some(_) => Err(usage(
            "unknown kind; expected mcp, template, catalog or decision",
        )),
    }
}

#[derive(Default)]
struct Options {
    name: Option<String>,
    host: bool,
    allow_network: bool,
    template: Option<String>,
    env: BTreeMap<String, SecretString>,
    calls: Option<PathBuf>,
    call_timeout: Option<u32>,
    json: Option<PathBuf>,
}

fn stdio(args: &[String]) -> Result<StdioArgs, Usage> {
    let split = args.iter().position(|arg| arg == "--");
    let (options, command) = match split {
        Some(at) => (&args[..at], &args[at + 1..]),
        None => (args, &[][..]),
    };
    let options = options_of(options)?;
    let place = match (options.host, options.template) {
        (true, None) => Place::Host {
            allow_network: options.allow_network,
        },
        (false, Some(image)) if !options.allow_network && pinned(&image) => Place::Template(image),
        (false, Some(_)) if !options.allow_network => {
            return Err(usage(
                "the --template image must be pinned: sha256:<64 hex> or <repository>@sha256:<64 hex>",
            ));
        }
        (false, Some(_)) => {
            return Err(usage(
                "--allow-network applies only to --host; a template never has network",
            ));
        }
        _ => return Err(usage("expected exactly one of --host or --template")),
    };
    let name = options.name.ok_or_else(|| usage("expected --name"))?;
    let Some((program, program_args)) = command.split_first() else {
        return Err(usage("expected a command after --"));
    };
    validate(&name, program, program_args, &options.env)?;
    Ok(StdioArgs {
        name,
        place,
        command: program.clone(),
        args: program_args.to_vec(),
        env: options.env,
        calls: options.calls,
        call_timeout_seconds: options.call_timeout.unwrap_or(DEFAULT_CALL_TIMEOUT_SECONDS),
        json: options.json,
    })
}

fn options_of(args: &[String]) -> Result<Options, Usage> {
    let mut options = Options::default();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = |flag: &str| {
            args.next()
                .cloned()
                .ok_or_else(|| usage(format!("{flag} needs a value")))
        };
        match arg.as_str() {
            "--name" => once(&mut options.name, value("--name")?, "--name")?,
            "--template" => once(&mut options.template, value("--template")?, "--template")?,
            "--host" => options.host = true,
            "--allow-network" => options.allow_network = true,
            "--calls" => once(&mut options.calls, value("--calls")?.into(), "--calls")?,
            "--json" => once(&mut options.json, value("--json")?.into(), "--json")?,
            "--call-timeout" => {
                let seconds = value("--call-timeout")?
                    .parse::<u32>()
                    .ok()
                    .filter(|seconds| (1..=MAX_CALL_TIMEOUT_SECONDS).contains(seconds))
                    .ok_or_else(|| usage("--call-timeout must be 1 through 600 seconds"))?;
                once(&mut options.call_timeout, seconds, "--call-timeout")?;
            }
            "--env" => {
                let entry = value("--env")?;
                let (name, env_value) = entry
                    .split_once('=')
                    .ok_or_else(|| usage("--env expects NAME=VALUE"))?;
                let previous = options
                    .env
                    .insert(name.to_owned(), SecretString::from(env_value));
                if previous.is_some() {
                    return Err(usage("--env names a variable twice"));
                }
            }
            flag if flag.starts_with('-') => {
                // Only the part before `=`: `--token=<value>` must not echo the value.
                let flag = flag.split_once('=').map_or(flag, |(flag, _)| flag);
                let flag: String = flag.chars().take(32).collect();
                return Err(usage(format!("unknown option {flag}")));
            }
            _ => return Err(usage("unexpected argument before --")),
        }
    }
    Ok(options)
}

fn template(args: &[String]) -> Result<TemplateArgs, Usage> {
    let Some((image, rest)) = args
        .split_first()
        .filter(|(image, _)| !image.starts_with('-'))
    else {
        return Err(usage(
            "expected an image pin: a local image ID or a registry digest",
        ));
    };
    if !pinned(image) {
        return Err(usage(
            "the image must be pinned: sha256:<64 hex> or <repository>@sha256:<64 hex>, never a tag",
        ));
    }
    let (mut gateway_config, mut json) = (None, None);
    let mut rest = rest.iter();
    while let Some(arg) = rest.next() {
        let mut value = |flag: &str| {
            rest.next()
                .cloned()
                .ok_or_else(|| usage(format!("{flag} needs a value")))
        };
        match arg.as_str() {
            "--gateway-config" => once(
                &mut gateway_config,
                value("--gateway-config")?.into(),
                "--gateway-config",
            )?,
            "--json" => once(&mut json, value("--json")?.into(), "--json")?,
            flag if flag.starts_with('-') => {
                let flag = flag.split_once('=').map_or(flag, |(flag, _)| flag);
                let flag: String = flag.chars().take(32).collect();
                return Err(usage(format!("unknown option {flag}")));
            }
            _ => return Err(usage("unexpected argument after the image pin")),
        }
    }
    Ok(TemplateArgs {
        image: image.clone(),
        gateway_config: gateway_config.ok_or_else(|| usage("expected --gateway-config"))?,
        json,
    })
}

fn decision(args: &[String]) -> Result<DecisionArgs, Usage> {
    let Some((address, rest)) = args.split_first().filter(|(url, _)| !url.starts_with('-')) else {
        return Err(usage("expected the engine's base URL"));
    };
    let (mut network, mut timeout_ms, mut bearer_env, mut json) = (false, None, None, None);
    let mut rest = rest.iter();
    while let Some(arg) = rest.next() {
        let mut value = |flag: &str| {
            rest.next()
                .cloned()
                .ok_or_else(|| usage(format!("{flag} needs a value")))
        };
        match arg.as_str() {
            "--network" => network = true,
            "--timeout-ms" => {
                let millis = value("--timeout-ms")?
                    .parse::<u64>()
                    .ok()
                    .filter(|millis| (1..=MAX_ENGINE_TIMEOUT_MS).contains(millis))
                    .ok_or_else(|| usage("--timeout-ms must be 1 through 10000"))?;
                once(&mut timeout_ms, millis, "--timeout-ms")?;
            }
            "--bearer-from-env" => {
                let name = value("--bearer-from-env")?;
                if !env_name(&name) {
                    return Err(usage(
                        "--bearer-from-env expects an environment variable's name, such as ENGINE_KEY",
                    ));
                }
                once(&mut bearer_env, name, "--bearer-from-env")?;
            }
            "--json" => once(&mut json, value("--json")?.into(), "--json")?,
            flag if flag.starts_with('-') => {
                let flag = flag.split_once('=').map_or(flag, |(flag, _)| flag);
                let flag: String = flag.chars().take(32).collect();
                return Err(usage(format!("unknown option {flag}")));
            }
            _ => return Err(usage("unexpected argument after the engine's URL")),
        }
    }
    Ok(DecisionArgs {
        url: engine_url(address, network)?,
        network,
        timeout_ms: timeout_ms.unwrap_or(DEFAULT_ENGINE_TIMEOUT_MS),
        bearer_env,
        json,
    })
}

/// The engine's address: `http` or `https`, no credential or fragment; loopback unless `--network`, and then `https`.
fn engine_url(address: &str, network: bool) -> Result<url::Url, Usage> {
    let url = url::Url::parse(address)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https") && url.host().is_some())
        .ok_or_else(|| usage("the engine's URL must be an http:// or https:// address"))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(usage(
            "the engine's URL must be without a name or password; use --bearer-from-env",
        ));
    }
    if url.fragment().is_some() {
        return Err(usage("the engine's URL must be without a fragment"));
    }
    let loopback = match url.host() {
        Some(url::Host::Domain(domain)) => domain == "localhost",
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    };
    if !loopback && !network {
        return Err(usage(
            "the engine isn't on this machine, so it needs an explicit --network",
        ));
    }
    if !loopback && url.scheme() != "https" {
        return Err(usage(
            "a remote engine needs https, as the gateway requires",
        ));
    }
    Ok(url)
}

/// An environment variable's name: capitals, digits and `_`, not starting with a digit.
fn env_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with(|first: char| first.is_ascii_digit())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

/// A local image ID (`sha256:` and 64 hex digits) or a registry digest (`<repository>@` and one).
fn pinned(image: &str) -> bool {
    let id = image.rsplit_once('@').map_or(
        image,
        |(repository, id)| {
            if repository.is_empty() { "" } else { id }
        },
    );
    id.strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn once<T>(slot: &mut Option<T>, value: T, flag: &str) -> Result<(), Usage> {
    if slot.replace(value).is_some() {
        return Err(usage(format!("{flag} given twice")));
    }
    Ok(())
}

/// The gateway's own `[[mcp.servers]]` validator (GW-U-036) decides, so ext-check and an install can't disagree: the
/// entry is checked inside a minimal configuration, and the refusal names only the field.
fn validate(
    name: &str,
    command: &str,
    args: &[String],
    env: &BTreeMap<String, SecretString>,
) -> Result<(), Usage> {
    let env: BTreeMap<&str, &str> = env
        .iter()
        .map(|(name, value)| (name.as_str(), value.expose_secret()))
        .collect();
    let entry = toml_entry(name, command, args, &env).map_err(|()| usage("invalid arguments"))?;
    let source = format!(
        "listen='127.0.0.1:0'\npublic_origin='http://127.0.0.1:1'\nmodels_path='models.toml'\n\
         [storage]\nroot='/var/lib/tqa'\n[[mcp.servers]]\n{entry}"
    );
    tqa_gateway::GatewayConfig::from_toml(&source)
        .map(|_| ())
        .map_err(|error| usage(field_of(&error.to_string())))
}

fn toml_entry(
    name: &str,
    command: &str,
    args: &[String],
    env: &BTreeMap<&str, &str>,
) -> Result<String, ()> {
    // JSON strings are valid TOML basic strings, so each value is quoted exactly.
    let quote = |value: &str| serde_json::to_string(value).map_err(|_| ());
    let args: Vec<String> = args
        .iter()
        .map(|arg| quote(arg))
        .collect::<Result<_, _>>()?;
    let env: Vec<String> = env
        .iter()
        .map(|(name, value)| Ok(format!("{} = {}", quote(name)?, quote(value)?)))
        .collect::<Result<_, ()>>()?;
    Ok(format!(
        "name = {}\ncommand = {}\nargs = [{}]\nenv = {{ {} }}\n",
        quote(name)?,
        quote(command)?,
        args.join(", "),
        env.join(", ")
    ))
}

/// The gateway's refusal names the `[[mcp.servers]]` field (`name`, `command`, `args`, `env`); anything else is
/// reported without detail, since it can't come from these arguments.
fn field_of(refusal: &str) -> String {
    ["name", "command", "args", "env"]
        .iter()
        .find(|field| refusal.contains(&format!("[[mcp.servers]] {field}")))
        .map_or_else(
            || "the server entry is invalid".to_owned(),
            |field| format!("invalid server {field} (the [[mcp.servers]] rules)"),
        )
}
