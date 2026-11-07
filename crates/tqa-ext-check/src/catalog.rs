//! The catalogue lint (docs/catalog/README.md): each entry in `docs/catalog/` must be complete, carry a passing
//! `ext-check/1` report that matches it, and give a configuration snippet the gateway's own validator accepts. Each
//! problem names the file and the field.
use std::{
    fmt,
    path::{Path, PathBuf},
};

use serde_json::Value;

mod template;

/// The fields every MCP entry's leading table must have.
const MCP_FIELDS: [&str; 8] = [
    "Name",
    "Kind",
    "Source",
    "Version",
    "Transport",
    "Tested",
    "Report",
    "Tools SHA-256",
];
const FIELDS_HEADER: &str = "| Field | Value |";
const TOOLS_HEADER: &str = "| Tool | Model name |";
/// The smallest configuration the gateway accepts, for checking an entry's snippet inside it.
const MINIMAL_CONFIG: &str = "listen='127.0.0.1:0'\npublic_origin='http://127.0.0.1:1'\n\
                              models_path='models.toml'\n[storage]\nroot='/var/lib/tqa'\n";

/// One problem in one file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Problem {
    /// The file, relative to the catalogue.
    pub file: String,
    /// The field or section.
    pub field: String,
    /// What is wrong, without quoting a value.
    pub message: String,
}

impl fmt::Display for Problem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}: {}", self.file, self.field, self.message)
    }
}

/// Lints the catalogue at `dir`.
///
/// # Errors
/// Returns the I/O error when the catalogue can't be read.
pub fn lint(dir: &Path) -> Result<Vec<Problem>, std::io::Error> {
    let mut problems = Vec::new();
    if !dir.join("README.md").is_file() {
        problems.push(problem("README.md", "file", "missing"));
    }
    for path in entries(&dir.join("mcp"))? {
        let file = format!("mcp/{}", file_name(&path));
        let text = std::fs::read_to_string(&path)?;
        Entry::new(&file, &path, &text).lint(&mut problems);
    }
    for path in entries(&dir.join("templates"))? {
        let file = format!("templates/{}", file_name(&path));
        let text = std::fs::read_to_string(&path)?;
        Entry::new(&file, &path, &text).lint_template(&mut problems);
    }
    Ok(problems)
}

/// The Markdown entries in `dir`, sorted; none when it does not exist.
fn entries(dir: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<_, _>>()?;
    paths.retain(|path| path.extension().is_some_and(|extension| extension == "md"));
    paths.sort();
    Ok(paths)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn problem(file: &str, field: &str, message: &str) -> Problem {
    Problem {
        file: file.to_owned(),
        field: field.to_owned(),
        message: message.to_owned(),
    }
}

/// One entry, parsed.
struct Entry<'a> {
    file: &'a str,
    path: &'a Path,
    fields: Vec<(String, String)>,
    tools: Option<Vec<[String; 2]>>,
    snippet: Option<String>,
}

impl<'a> Entry<'a> {
    fn new(file: &'a str, path: &'a Path, text: &str) -> Self {
        let fields = table(text, FIELDS_HEADER)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|row| match row.as_slice() {
                [name, value] => Some((name.clone(), value.clone())),
                _ => None,
            })
            .collect();
        let tools = table(text, TOOLS_HEADER).map(|rows| {
            rows.into_iter()
                .filter_map(|row| <[String; 2]>::try_from(row).ok())
                .collect()
        });
        Self {
            file,
            path,
            fields,
            tools,
            snippet: toml_block(text),
        }
    }

    fn field(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value.as_str())
    }

    fn push(&self, problems: &mut Vec<Problem>, field: &str, message: &str) {
        problems.push(problem(self.file, field, message));
    }

    fn lint(&self, problems: &mut Vec<Problem>) {
        for field in MCP_FIELDS {
            if self.field(field).is_none_or(str::is_empty) {
                self.push(problems, field, "missing");
            }
        }
        let name = self.field("Name");
        self.name_matches_file(problems);
        if self.field("Kind").is_some_and(|kind| kind != "mcp") {
            self.push(problems, "Kind", "an entry under mcp/ has kind mcp");
        }
        if self
            .field("Transport")
            .is_some_and(|transport| transport != "stdio")
        {
            self.push(
                problems,
                "Transport",
                "only stdio entries can be tested yet",
            );
        }
        if self.field("Default class").is_some() {
            self.push(
                problems,
                "Default class",
                "an MCP server has no permission class (ADR-0035); remove the field",
            );
        }
        if let Some(report) = self.load_report(problems)
            && self.mcp_report(&report, problems)
        {
            self.match_report(&report, problems);
        }
        self.lint_snippet(name.unwrap_or_default(), problems);
    }

    fn name_matches_file(&self, problems: &mut Vec<Problem>) {
        let stem = self
            .path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned());
        if self
            .field("Name")
            .is_some_and(|name| Some(name) != stem.as_deref())
        {
            self.push(problems, "Name", "does not match the file name");
        }
    }

    /// Whether each (pointer, value) holds in the report; each that doesn't is a `Report` problem.
    fn expect(
        &self,
        report: &Value,
        expected: &[(&str, Value, &str)],
        problems: &mut Vec<Problem>,
    ) -> bool {
        let mut sound = true;
        for (pointer, value, message) in expected {
            if report.pointer(pointer) != Some(value) {
                self.push(problems, "Report", message);
                sound = false;
            }
        }
        sound
    }

    /// The entry's report file, as a JSON object.
    fn load_report(&self, problems: &mut Vec<Problem>) -> Option<Value> {
        let name = self.field("Report")?;
        if name.contains('/') || name.starts_with('.') {
            self.push(
                problems,
                "Report",
                "names a file outside the entry's directory",
            );
            return None;
        }
        let path = self.path.with_file_name(name);
        let Ok(text) = std::fs::read_to_string(&path) else {
            self.push(problems, "Report", "the report file is missing");
            return None;
        };
        let report = serde_json::from_str::<Value>(&text)
            .ok()
            .filter(Value::is_object);
        if report.is_none() {
            self.push(problems, "Report", "the report is not a JSON object");
        }
        report
    }

    /// A passing `ext-check/1` report for this server, tested in a template image without network.
    fn mcp_report(&self, report: &Value, problems: &mut Vec<Problem>) -> bool {
        let expected = [
            (
                "/schema",
                Value::from("ext-check/1"),
                "the report is not ext-check/1",
            ),
            (
                "/kind",
                Value::from("mcp"),
                "the report is not for an MCP server",
            ),
            ("/result", Value::from("pass"), "the report did not pass"),
            (
                "/target/name",
                Value::from(self.field("Name").unwrap_or_default()),
                "the report is for another server",
            ),
            (
                "/target/place",
                Value::from("template"),
                "the server was not tested in a template image",
            ),
            (
                "/flags/allow_network",
                Value::from(false),
                "the server was tested with network",
            ),
        ];
        self.expect(report, &expected, problems)
    }

    /// The report was made at a clean, known commit, and the entry's `Tested` says when.
    fn check_tested(&self, report: &Value, problems: &mut Vec<Problem>) {
        let commit = report
            .pointer("/ext_check/commit")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let date = report
            .pointer("/date")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !(7..=40).contains(&commit.len()) || !commit.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            self.push(
                problems,
                "Tested",
                "the report was not made at a clean, known commit",
            );
        } else if self
            .field("Tested")
            .is_some_and(|tested| tested != format!("ext-check {commit}, {date}"))
        {
            self.push(
                problems,
                "Tested",
                "does not match the report's commit and date",
            );
        }
    }

    fn match_report(&self, report: &Value, problems: &mut Vec<Problem>) {
        self.check_tested(report, problems);
        let hash = report.pointer("/tools_sha256").and_then(Value::as_str);
        if self
            .field("Tools SHA-256")
            .is_some_and(|field| Some(field) != hash)
        {
            self.push(problems, "Tools SHA-256", "does not match the report");
        }
        let Some(tools) = &self.tools else {
            self.push(problems, "Tools", "no | Tool | Model name | table");
            return;
        };
        let reported: Vec<[String; 2]> = report
            .pointer("/tools")
            .and_then(Value::as_array)
            .map(|tools| {
                tools
                    .iter()
                    .map(|tool| {
                        ["name", "model_name"]
                            .map(|key| tool[key].as_str().unwrap_or_default().to_owned())
                    })
                    .collect()
            })
            .unwrap_or_default();
        if *tools != reported {
            self.push(
                problems,
                "Tools",
                "the table differs from the report's tools",
            );
        }
    }

    /// The snippet holds exactly this server's `[[mcp.servers]]` entry, and the gateway accepts it.
    fn lint_snippet(&self, name: &str, problems: &mut Vec<Problem>) {
        let Some(snippet) = &self.snippet else {
            self.push(problems, "Configuration", "no toml block");
            return;
        };
        let parsed = snippet.parse::<toml::Table>().ok();
        let servers = parsed
            .as_ref()
            .filter(|table| table.len() == 1)
            .and_then(|table| table.get("mcp"))
            .and_then(toml::Value::as_table)
            .filter(|mcp| mcp.len() == 1)
            .and_then(|mcp| mcp.get("servers"))
            .and_then(toml::Value::as_array);
        match servers.map(Vec::as_slice) {
            Some([server]) if server.get("name").and_then(toml::Value::as_str) == Some(name) => {}
            Some([_]) => self.push(
                problems,
                "Configuration",
                "the server's name differs from the entry's",
            ),
            _ => self.push(
                problems,
                "Configuration",
                "expected exactly one [[mcp.servers]] entry",
            ),
        }
        if let Err(error) =
            tqa_gateway::GatewayConfig::from_toml(&format!("{MINIMAL_CONFIG}{snippet}"))
        {
            self.push(
                problems,
                "Configuration",
                &format!("the gateway refuses it: {error}"),
            );
        }
    }
}

/// The rows after `header`, until the table ends, without its separator line.
fn table(text: &str, header: &str) -> Option<Vec<Vec<String>>> {
    let mut lines = text.lines().skip_while(|line| line.trim() != header);
    lines.next()?;
    Some(
        lines
            .take_while(|line| line.trim_start().starts_with('|'))
            .filter(|line| {
                !line.trim_start().starts_with("|---") && !line.trim_start().starts_with("|:")
            })
            .map(|line| {
                line.trim()
                    .trim_matches('|')
                    .split('|')
                    .map(|cell| cell.trim().to_owned())
                    .collect()
            })
            .collect(),
    )
}

/// The first fenced `toml` block.
fn toml_block(text: &str) -> Option<String> {
    let mut lines = text.lines().skip_while(|line| line.trim() != "```toml");
    lines.next()?;
    let mut block = String::new();
    for line in lines {
        if line.trim() == "```" {
            return Some(block);
        }
        block.push_str(line);
        block.push('\n');
    }
    None
}
