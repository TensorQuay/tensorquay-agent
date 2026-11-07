//! Model-facing tool names (ADR-0011 decision 4, TOOL-U-029). Providers accept only `[a-zA-Z0-9_-]{1,64}` and refuse dots,
//! so an MCP tool `mcp.<server>.<tool>` is shown to the model as `mcp__<server>__<tool>`, folded to `[a-z0-9_]` and cut to 64
//! bytes, with `_2`, `_3`… on a collision, in the order of the original names. Builtins keep their names. The table is built
//! from a run's fixed catalogue, so it is the same on every step and on replay, and it maps both ways.

use std::collections::{BTreeMap, BTreeSet};

use crate::ToolName;

/// The largest model-facing tool name, in bytes, that every provider accepts.
pub const MAX_MODEL_TOOL_NAME_BYTES: usize = 64;
/// The model-facing prefix of an MCP tool; no builtin may start with it.
pub(crate) const MCP_MODEL_PREFIX: &str = "mcp__";

/// The two-way map between a run's tool names and the names the model sees.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ModelNames {
    to_model: BTreeMap<ToolName, String>,
    to_internal: BTreeMap<String, ToolName>,
}

impl ModelNames {
    /// Builds the table for a catalogue; duplicates are counted once.
    #[must_use]
    pub fn new<'a>(names: impl IntoIterator<Item = &'a ToolName>) -> Self {
        let mut table = Self::default();
        // Sorted, so the suffixes follow the original names whatever order the catalogue arrives in.
        for name in names.into_iter().collect::<BTreeSet<_>>() {
            let model = match name.mcp_parts() {
                None => name.as_str().to_owned(),
                Some((server, tool)) => {
                    table.free(&format!("{MCP_MODEL_PREFIX}{server}__{}", fold(tool)))
                }
            };
            table.to_internal.insert(model.clone(), name.clone());
            table.to_model.insert(name.clone(), model);
        }
        table
    }

    /// The name the model sees for a tool of this catalogue.
    #[must_use]
    pub fn model(&self, internal: &ToolName) -> Option<&str> {
        self.to_model.get(internal).map(String::as_str)
    }

    /// The tool a model-facing name stands for, if this catalogue has it.
    #[must_use]
    pub fn internal(&self, model: &str) -> Option<&ToolName> {
        self.to_internal.get(model)
    }

    /// The model-facing name for a journalled name; a name outside this catalogue is returned as given.
    #[must_use]
    pub fn to_model<'a>(&'a self, internal: &'a str) -> &'a str {
        self.to_model.get(internal).map_or(internal, String::as_str)
    }

    /// The journalled name for a name the model returned; a name outside this catalogue is returned as given, so the
    /// registry refuses it as unknown.
    #[must_use]
    pub fn to_internal<'a>(&'a self, model: &'a str) -> &'a str {
        self.to_internal.get(model).map_or(model, ToolName::as_str)
    }

    /// The base name cut to the limit, or the first free suffixed variant of it.
    fn free(&self, base: &str) -> String {
        let first = cut(base, MAX_MODEL_TOOL_NAME_BYTES);
        if !self.to_internal.contains_key(first) {
            return first.to_owned();
        }
        // A catalogue holds at most MAX_TOOLS names, so a free suffix exists within this range.
        (2..=tqa_types::MAX_TOOLS + 1)
            .map(|n| {
                let suffix = format!("_{n}");
                format!(
                    "{}{suffix}",
                    cut(base, MAX_MODEL_TOOL_NAME_BYTES - suffix.len())
                )
            })
            .find(|candidate| !self.to_internal.contains_key(candidate))
            .unwrap_or_default()
    }
}

/// Lowercases and replaces every byte outside `[a-z0-9_]` with `_`; MCP tool names are ASCII by the name rule.
fn fold(tool: &str) -> String {
    tool.bytes()
        .map(|byte| match byte.to_ascii_lowercase() {
            lower @ (b'a'..=b'z' | b'0'..=b'9' | b'_') => char::from(lower),
            _ => '_',
        })
        .collect()
}

/// At most `max` bytes of ASCII text.
fn cut(text: &str, max: usize) -> &str {
    &text[..text.len().min(max)]
}
