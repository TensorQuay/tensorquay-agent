//! The `ext-check/1` report (TK-U-014): one entry per check with a stable code, the target's identity, and
//! never a credential, an environment value or stderr text.
use std::fmt::Write;

use serde::Serialize;

/// Every check that ran passed (warnings allowed).
pub const EXIT_PASSED: u8 = 0;
/// A check failed.
pub const EXIT_FAILED: u8 = 1;
/// The command line was refused.
pub const EXIT_USAGE: u8 = 2;
/// The environment could not run the checks (or nothing was checked).
pub const EXIT_ENVIRONMENT: u8 = 3;

/// A check's outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// It held.
    Pass,
    /// It held with a note the catalogue entry should state.
    Warn,
    /// It did not hold.
    Fail,
    /// It could not run here; never counted as a pass.
    Skip,
}

impl Status {
    const fn label(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Warn => "WARN",
            Self::Fail => "FAIL",
            Self::Skip => "SKIP",
        }
    }
}

/// One check.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Check {
    /// Its stable code.
    pub code: String,
    /// Its outcome.
    pub status: Status,
    /// One sentence; never a value from the server's environment or stderr.
    pub message: String,
}

impl Check {
    /// A check with its code, outcome and sentence.
    #[must_use]
    pub fn new(code: &str, status: Status, message: &str) -> Self {
        Self {
            code: code.to_owned(),
            status,
            message: message.to_owned(),
        }
    }
}

/// A stdio server as it was run.
#[derive(Clone, Debug, Serialize)]
pub struct StdioTarget {
    transport: &'static str,
    place: &'static str,
    name: String,
    command: String,
    args: Vec<String>,
    env_names: Vec<String>,
    image: Option<String>,
    #[serde(skip)]
    allow_network: bool,
}

/// A template image as it was checked.
#[derive(Clone, Debug, Serialize)]
pub struct TemplateTarget {
    /// The pin given: a local image ID or a registry digest.
    image: String,
    /// The local image ID it resolved to, once verified.
    image_id: Option<String>,
    /// The base template it derives from, once verified.
    base: Option<String>,
}

/// A decision engine as it was asked.
#[derive(Clone, Debug, Serialize)]
pub struct EngineTarget {
    /// Its base URL, without any credential (the command line refuses one in the URL).
    url: String,
    /// The time it had to answer each consultation.
    timeout_ms: u64,
    /// Whether a credential was sent; never its value or where it came from.
    credential: bool,
    /// Whether a non-loopback address was allowed.
    #[serde(skip)]
    network: bool,
}

/// What was checked.
#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub enum Target {
    /// A stdio MCP server.
    Stdio(StdioTarget),
    /// A sandbox template image.
    Template(TemplateTarget),
    /// A `decision/v1` engine.
    Engine(EngineTarget),
}

impl Target {
    /// A stdio server run on this host.
    #[must_use]
    pub fn stdio_host(
        name: &str,
        command: &str,
        args: &[String],
        env_names: Vec<String>,
        allow_network: bool,
    ) -> Self {
        Self::Stdio(StdioTarget {
            transport: "stdio",
            place: "host",
            name: name.to_owned(),
            command: command.to_owned(),
            args: args.to_vec(),
            env_names,
            image: None,
            allow_network,
        })
    }

    /// A stdio server run in a container from a template image, without network.
    #[must_use]
    pub fn stdio_template(
        name: &str,
        command: &str,
        args: &[String],
        env_names: Vec<String>,
        image: &str,
    ) -> Self {
        Self::Stdio(StdioTarget {
            transport: "stdio",
            place: "template",
            name: name.to_owned(),
            command: command.to_owned(),
            args: args.to_vec(),
            env_names,
            image: Some(image.to_owned()),
            allow_network: false,
        })
    }

    /// A template image, by its pin.
    #[must_use]
    pub fn template(image: &str) -> Self {
        Self::Template(TemplateTarget {
            image: image.to_owned(),
            image_id: None,
            base: None,
        })
    }

    /// A decision engine at `url`, asked within `timeout_ms`, with or without a credential.
    #[must_use]
    pub fn engine(url: &str, timeout_ms: u64, credential: bool, network: bool) -> Self {
        Self::Engine(EngineTarget {
            url: url.to_owned(),
            timeout_ms,
            credential,
            network,
        })
    }

    const fn kind(&self) -> &'static str {
        match self {
            Self::Stdio(_) => "mcp",
            Self::Template(_) => "template",
            Self::Engine(_) => "decision",
        }
    }

    fn label(&self) -> &str {
        match self {
            Self::Stdio(stdio) => &stdio.name,
            Self::Template(template) => &template.image,
            Self::Engine(engine) => &engine.url,
        }
    }

    const fn allow_network(&self) -> bool {
        match self {
            Self::Stdio(stdio) => stdio.allow_network,
            Self::Template(_) => false,
            Self::Engine(engine) => engine.network,
        }
    }
}

/// The server's own `serverInfo`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ServerInfo {
    /// Its name.
    pub name: String,
    /// Its version.
    pub version: String,
}

/// One tool as a run would see it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ToolEntry {
    /// The server's own name.
    pub name: String,
    /// The name the model sees.
    pub model_name: String,
}

#[derive(Clone, Debug, Serialize)]
struct ExtCheck {
    commit: String,
}

#[derive(Clone, Debug, Serialize)]
struct Flags {
    allow_network: bool,
}

/// The whole report.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    schema: &'static str,
    kind: &'static str,
    target: Target,
    /// The server's `serverInfo`, once the handshake completed.
    pub server: Option<ServerInfo>,
    /// The tools, once listed.
    pub tools: Vec<ToolEntry>,
    /// SHA-256 of the canonical tool list, once listed.
    pub tools_sha256: Option<String>,
    checks: Vec<Check>,
    ext_check: ExtCheck,
    date: String,
    flags: Flags,
    result: &'static str,
}

impl Report {
    /// An empty report for `target`, made by ext-check at `commit` on `date` (RFC 3339).
    #[must_use]
    pub fn new(target: Target, commit: &str, date: &str) -> Self {
        let flags = Flags {
            allow_network: target.allow_network(),
        };
        Self {
            schema: "ext-check/1",
            kind: target.kind(),
            target,
            server: None,
            tools: Vec::new(),
            tools_sha256: None,
            checks: Vec::new(),
            ext_check: ExtCheck {
                commit: commit.to_owned(),
            },
            date: date.to_owned(),
            flags,
            result: "nothing_checked",
        }
    }

    /// Records what a verified template resolved to: its base template and local image ID.
    pub fn verified_template(&mut self, base: &str, image_id: &str) {
        if let Target::Template(template) = &mut self.target {
            template.base = Some(base.to_owned());
            template.image_id = Some(image_id.to_owned());
        }
    }

    /// Records a check.
    pub fn push(&mut self, check: Check) {
        self.checks.push(check);
        self.result = self.result();
    }

    /// The checks so far.
    #[must_use]
    pub fn checks(&self) -> &[Check] {
        &self.checks
    }

    fn count(&self, status: Status) -> usize {
        self.checks
            .iter()
            .filter(|check| check.status == status)
            .count()
    }

    /// `fail` if any check failed, `pass` if at least one passed, otherwise `nothing_checked`.
    #[must_use]
    pub fn result(&self) -> &'static str {
        if self.count(Status::Fail) > 0 {
            "fail"
        } else if self.count(Status::Pass) + self.count(Status::Warn) > 0 {
            "pass"
        } else {
            "nothing_checked"
        }
    }

    /// The process exit status for this report.
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        match self.result() {
            "fail" => EXIT_FAILED,
            "pass" => EXIT_PASSED,
            _ => EXIT_ENVIRONMENT,
        }
    }

    /// One line per check, then a summary.
    #[must_use]
    pub fn text(&self) -> String {
        let mut text = String::new();
        for check in &self.checks {
            let _ = writeln!(
                text,
                "{} {}: {}",
                check.status.label(),
                check.code,
                check.message
            );
        }
        let _ = writeln!(
            text,
            "ext-check {} {}: {} ({} passed, {} warning{}, {} failed, {} skipped; skipped checks are not passes)",
            self.kind,
            self.target.label(),
            self.result(),
            self.count(Status::Pass),
            self.count(Status::Warn),
            if self.count(Status::Warn) == 1 {
                ""
            } else {
                "s"
            },
            self.count(Status::Fail),
            self.count(Status::Skip),
        );
        text
    }
}
