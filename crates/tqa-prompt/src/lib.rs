//! Versioned system prompt assembly and bounded repository instruction discovery.
mod compaction;
mod error;
mod fallback;
mod instructions;
mod skill_tool;
mod skills;
mod system;

pub use compaction::{COMPACTION_PROMPT, COMPACTION_PROMPT_VERSION};
pub use error::PromptError;
pub use fallback::{
    FALLBACK_INSTRUCTIONS, FALLBACK_PROMPT_VERSION, FallbackCall, FallbackFailure, FallbackReply,
    Rejection, ToolCheck, explicit_streamable_len, fallback_instructions, parse_reply,
    parse_reply_detailed, parse_text_calls, parse_text_reply, render_call, streamable_len,
};
pub use instructions::{
    InstructionFile, MAX_INSTRUCTION_FILE_BYTES, MAX_INSTRUCTION_TOTAL_BYTES, discover, render,
    render_profile_instructions,
};
pub use skill_tool::{MAX_LISTED_SKILL_FILES, SkillTool};
pub use skills::{
    MAX_SKILLS, Manifest, SKILL_FOLDER_BYTES, SKILL_INDEX_BYTES, SKILL_MANIFEST_BYTES, Skill,
    SkillSet, SkillTier, SkipReason, SkippedSkill, discover_skills, parse_manifest,
    render_skill_index,
};
pub use system::{PROMPT_VERSION, WorkspaceFacts, system_prompt};

/// Returns this crate's package name.
#[must_use]
pub fn crate_name() -> &'static str {
    "tqa-prompt"
}

#[cfg(test)]
mod tests {
    #[test]
    fn returns_crate_name() {
        assert_eq!(super::crate_name(), "tqa-prompt");
    }
}
