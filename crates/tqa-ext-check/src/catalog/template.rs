//! Template entries (`templates/<name>.md`): a passing template report for the entry's image and base, whose static
//! checks, posture and boot to ready all passed, and a snippet holding exactly its own `[[templates]]` entry.
use serde_json::Value;

use super::{Entry, MINIMAL_CONFIG, Problem};

/// The fields every template entry's leading table must have.
const TEMPLATE_FIELDS: [&str; 8] = [
    "Name", "Kind", "Source", "Version", "Base", "Image", "Tested", "Report",
];
/// The stages a template report must have passed; a skip is not a pass.
const STAGES: [&str; 3] = ["template_checks", "posture", "boot_ready"];

impl Entry<'_> {
    pub(super) fn lint_template(&self, problems: &mut Vec<Problem>) {
        for field in TEMPLATE_FIELDS {
            if self.field(field).is_none_or(str::is_empty) {
                self.push(problems, field, "missing");
            }
        }
        self.name_matches_file(problems);
        if self.field("Kind").is_some_and(|kind| kind != "template") {
            self.push(
                problems,
                "Kind",
                "an entry under templates/ has kind template",
            );
        }
        if let Some(report) = self.load_report(problems) {
            self.template_report(&report, problems);
        }
        self.template_snippet(problems);
    }

    fn template_report(&self, report: &Value, problems: &mut Vec<Problem>) {
        let expected = [
            (
                "/schema",
                Value::from("ext-check/1"),
                "the report is not ext-check/1",
            ),
            (
                "/kind",
                Value::from("template"),
                "the report is not for a template",
            ),
            ("/result", Value::from("pass"), "the report did not pass"),
            (
                "/flags/allow_network",
                Value::from(false),
                "the template was checked with network",
            ),
        ];
        if !self.expect(report, &expected, problems) {
            return;
        }
        let checks = report.pointer("/checks").and_then(Value::as_array);
        for stage in STAGES {
            let passed = checks.is_some_and(|checks| {
                checks
                    .iter()
                    .any(|check| check["code"] == stage && check["status"] == "pass")
            });
            if !passed {
                self.push(
                    problems,
                    "Report",
                    &format!("the report's {stage} stage did not pass"),
                );
            }
        }
        for (field, pointer, what) in [
            ("Image", "/target/image", "image"),
            ("Base", "/target/base", "base"),
        ] {
            let reported = report.pointer(pointer).and_then(Value::as_str);
            if self
                .field(field)
                .is_some_and(|value| Some(value) != reported)
            {
                self.push(
                    problems,
                    field,
                    &format!("differs from the report's {what}"),
                );
            }
        }
        self.check_tested(report, problems);
    }

    /// The snippet holds exactly this template's `[[templates]]` entry, with its image, and the gateway accepts it.
    /// The gateway needs exactly one default, so a snippet that isn't the default is checked as if it were.
    fn template_snippet(&self, problems: &mut Vec<Problem>) {
        let Some(snippet) = &self.snippet else {
            self.push(problems, "Configuration", "no toml block");
            return;
        };
        let parsed = snippet.parse::<toml::Table>().ok();
        let mut templates = parsed
            .filter(|table| table.len() == 1)
            .and_then(|mut table| table.remove("templates"))
            .and_then(|templates| match templates {
                toml::Value::Array(entries) => Some(entries),
                _ => None,
            })
            .filter(|entries| entries.len() == 1);
        let Some(entry) = templates.as_mut().and_then(|entries| entries.first_mut()) else {
            self.push(
                problems,
                "Configuration",
                "expected exactly one [[templates]] entry",
            );
            return;
        };
        let text = |key: &str| {
            entry
                .get(key)
                .and_then(toml::Value::as_str)
                .map(str::to_owned)
        };
        if text("name").as_deref() != self.field("Name") {
            self.push(
                problems,
                "Configuration",
                "the template's name differs from the entry's",
            );
        }
        if text("image").as_deref() != self.field("Image") {
            self.push(
                problems,
                "Configuration",
                "the template's image differs from the entry's",
            );
        }
        if let Some(table) = entry.as_table_mut() {
            table.entry("default").or_insert(toml::Value::Boolean(true));
        }
        let mut whole = toml::Table::new();
        whole.insert(
            "templates".into(),
            toml::Value::Array(templates.unwrap_or_default()),
        );
        let refusal = toml::to_string(&whole)
            .map_err(|_| "the snippet cannot be re-encoded".to_owned())
            .and_then(|text| {
                tqa_gateway::GatewayConfig::from_toml(&format!("{MINIMAL_CONFIG}{text}"))
                    .map(|_| ())
                    .map_err(|error| format!("the gateway refuses it: {error}"))
            });
        if let Err(message) = refusal {
            self.push(problems, "Configuration", &message);
        }
    }
}
