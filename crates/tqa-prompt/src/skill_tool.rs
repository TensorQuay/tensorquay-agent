//! The `skill` tool (ADR-0029): progressive disclosure of the skills in the run's index. The index names each skill in
//! the prompt; this read-only tool returns one skill's instructions and the absolute paths of its helper files. It
//! lives beside the parser because `tqa-tools` cannot depend on `tqa-prompt` (G-07).
use crate::skills::{Skill, SkillSet, SkipReason, load};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::{fmt::Write as _, path::Path};
use tqa_tools::{Tool, ToolContext, ToolError, ToolName, ToolOutput, ToolSpec};

/// Helper files listed per skill; the rest are counted.
pub const MAX_LISTED_SKILL_FILES: usize = 100;
/// Bytes of skill names an unknown-skill error lists.
const KNOWN_NAMES_BYTES: usize = 300;

/// Loads one skill of the run's index; the set is fixed at run start, like the index.
pub struct SkillTool {
    spec: ToolSpec,
    skills: Vec<Skill>,
}
impl SkillTool {
    /// Constructs the tool for the skills discovered at run start.
    ///
    /// # Errors
    /// Never in practice: the fixed tool name is valid.
    pub fn new(set: &SkillSet) -> Result<Self, ToolError> {
        Ok(Self {
            spec: ToolSpec {
                name: ToolName::new("skill")?,
                description: "Load a skill from the skills index: its instructions and the absolute paths of its helper files. Load a skill before you use it. Skill files are read-only; run a helper with the shell tool by its absolute path.".into(),
                parameters: json!({"type":"object", "required":["name"], "additionalProperties":false,
                    "properties":{"name":{"type":"string", "pattern":"^[a-z0-9-]{1,64}$"}}}),
                parallel_safe: true,
                changes_workspace: false,
            },
            skills: set.skills.clone(),
        })
    }
    fn unknown(&self) -> ToolError {
        let detail = if self.skills.is_empty() {
            "unknown skill; no skills are available in this run".to_owned()
        } else {
            known_names(&self.skills)
        };
        ToolError::InvalidArgs {
            path: "/name".into(),
            detail,
        }
    }
}
fn known_names(skills: &[Skill]) -> String {
    let mut detail = String::from("unknown skill; available:");
    for skill in skills {
        if detail.len() + skill.name.len() + 1 > KNOWN_NAMES_BYTES {
            detail.push_str(" …");
            break;
        }
        detail.push(' ');
        detail.push_str(&skill.name);
    }
    detail
}
#[async_trait]
impl Tool for SkillTool {
    // Its schema is strict-valid: optional properties are nullable, and null means absent (TOOL-U-034).
    fn null_means_absent(&self) -> bool {
        true
    }
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    async fn call(&self, args: Value, cx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let name = args.get("name").and_then(Value::as_str).unwrap_or_default();
        let skill = self
            .skills
            .iter()
            .find(|skill| skill.name == name)
            .cloned()
            .ok_or_else(|| self.unknown())?;
        let task = tokio::task::spawn_blocking(move || render(&skill));
        tokio::select! {
            biased;
            () = cx.cancel.cancelled() => Err(ToolError::Cancelled),
            () = cx.sleeper.sleep(cx.limits.file_timeout) => Err(ToolError::Timeout),
            joined = task => joined.map_err(|_| ToolError::Io("the skill loader stopped".into()))?,
        }
    }
}

/// Reloads the skill with the discovery checks, so a folder changed since run start is refused, not trusted.
fn render(skill: &Skill) -> Result<ToolOutput, ToolError> {
    let changed = |reason: SkipReason| {
        ToolError::PathInvalid(format!(
            "skill {} changed after the run started and cannot be loaded ({})",
            skill.name,
            reason.code()
        ))
    };
    let (manifest, mut files) = load(&skill.name, &skill.path)
        .map_err(changed)?
        .ok_or_else(|| changed(SkipReason::MissingManifest))?;
    files.retain(|file| file != Path::new("SKILL.md"));
    let mut text = format!(
        "Skill {} ({}) in {}\n\n{}",
        skill.name,
        skill.tier.label(),
        skill.path.display(),
        manifest.body
    );
    if !text.ends_with('\n') {
        text.push('\n');
    }
    if files.is_empty() {
        text.push_str("\nNo helper files.\n");
        return Ok(ToolOutput::text(text));
    }
    let _ = writeln!(text, "\nHelper files ({}):", files.len());
    for file in files.iter().take(MAX_LISTED_SKILL_FILES) {
        let _ = writeln!(text, "{}", skill.path.join(file).display());
    }
    if let Some(more) = files
        .len()
        .checked_sub(MAX_LISTED_SKILL_FILES)
        .filter(|more| *more > 0)
    {
        let _ = writeln!(text, "[{more} more files not listed]");
    }
    Ok(ToolOutput::text(text))
}
