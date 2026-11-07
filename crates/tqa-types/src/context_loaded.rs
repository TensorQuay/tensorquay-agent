//! The `context.loaded` event (PRD FR-11): what the runtime loaded into the model context at the start or resume of a
//! run: instruction files, skills, and skill folders that were skipped with a stable reason. It is shown in the
//! context panel and never enters the model context itself.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

/// Most instruction files, skills or skipped folders listed in one `context.loaded` event.
pub const MAX_CONTEXT_LOADED_ITEMS: usize = 64;
/// Longest instruction file path, in UTF-8 bytes.
const MAX_INSTRUCTION_PATH_BYTES: usize = 4096;
/// Longest skill description, in characters.
const MAX_SKILL_DESCRIPTION_CHARS: usize = 280;
/// Longest skipped folder name, in UTF-8 bytes.
const MAX_SKILL_FOLDER_BYTES: usize = 255;

/// Where a skill comes from; the more specific tier wins a name clash.
#[derive(
    Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum SkillTier {
    /// Installed by the administrator for everyone.
    Install,
    /// The running user's own skills.
    User,
}
impl SkillTier {
    /// The tier's wire name, also used in the skills index.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::User => "user",
        }
    }
}

/// Why a skill folder was not loaded.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SkillSkipReason {
    /// The folder has no `SKILL.md`.
    MissingManifest,
    /// The front matter is not exactly `name` and `description` between two `---` lines, or a value is invalid.
    BadFrontMatter,
    /// The name is not the folder name.
    NameMismatch,
    /// The manifest or the folder exceeds its size bound.
    TooLarge,
    /// The folder or something inside it is a symbolic link.
    Symlink,
    /// The manifest is not UTF-8.
    NotUtf8,
    /// A more specific tier has a skill with the same name.
    Shadowed,
    /// More skills are available than a run takes.
    OverLimit,
    /// The folder could not be read.
    Unreadable,
}
impl SkillSkipReason {
    /// The stable wire code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::MissingManifest => "missing_manifest",
            Self::BadFrontMatter => "bad_front_matter",
            Self::NameMismatch => "name_mismatch",
            Self::TooLarge => "too_large",
            Self::Symlink => "symlink",
            Self::NotUtf8 => "not_utf8",
            Self::Shadowed => "shadowed",
            Self::OverLimit => "over_limit",
            Self::Unreadable => "unreadable",
        }
    }
}

/// An instruction file in the prompt, in prompt order.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LoadedInstruction {
    /// The workspace-relative path.
    #[schemars(length(min = 1, max = 4096))]
    pub path: String,
    /// Whether a size limit cut the file.
    pub truncated: bool,
}

/// A skill in the prompt's skills index.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LoadedSkill {
    /// The skill's name, equal to its folder name.
    #[schemars(regex(pattern = "^[a-z0-9-]{1,64}$"))]
    pub name: String,
    /// The tier it was loaded from.
    pub tier: SkillTier,
    /// Its one-line description.
    #[schemars(length(min = 1, max = 280))]
    pub description: String,
}

/// A skill folder that was not loaded, and why.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SkippedSkill {
    /// The folder name.
    #[schemars(length(min = 1, max = 255))]
    pub folder: String,
    /// Its tier.
    pub tier: SkillTier,
    /// Why it was skipped.
    pub reason: SkillSkipReason,
}

/// The payload of a `context.loaded` event, as the runtime assembles it before a run.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ContextLoaded {
    /// Instruction files in prompt order.
    pub instructions: Vec<LoadedInstruction>,
    /// Skills in index order.
    pub skills: Vec<LoadedSkill>,
    /// Skipped skill folders.
    pub skipped: Vec<SkippedSkill>,
}
impl ContextLoaded {
    /// Keeps at most [`MAX_CONTEXT_LOADED_ITEMS`] of each list, the first ones.
    #[must_use]
    pub fn bounded(mut self) -> Self {
        self.instructions.truncate(MAX_CONTEXT_LOADED_ITEMS);
        self.skills.truncate(MAX_CONTEXT_LOADED_ITEMS);
        self.skipped.truncate(MAX_CONTEXT_LOADED_ITEMS);
        self
    }
    /// The event, with each list bounded.
    #[must_use]
    pub fn into_event(self) -> crate::Event {
        let Self {
            instructions,
            skills,
            skipped,
        } = self.bounded();
        crate::Event::ContextLoaded {
            instructions,
            skills,
            skipped,
        }
    }
}

fn list<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
    field: &str,
    valid: impl Fn(&T) -> bool,
) -> Result<Vec<T>, D::Error> {
    let items = Vec::<T>::deserialize(deserializer)?;
    if items.len() > MAX_CONTEXT_LOADED_ITEMS {
        return Err(D::Error::custom(format!(
            "{field}: expected at most 64 items"
        )));
    }
    if !items.iter().all(valid) {
        return Err(D::Error::custom(format!("{field}: invalid item")));
    }
    Ok(items)
}
pub(crate) fn instructions<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<LoadedInstruction>, D::Error> {
    list(deserializer, "instructions", |item: &LoadedInstruction| {
        (1..=MAX_INSTRUCTION_PATH_BYTES).contains(&item.path.len())
    })
}
pub(crate) fn skills<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<LoadedSkill>, D::Error> {
    list(deserializer, "skills", |item: &LoadedSkill| {
        (1..=64).contains(&item.name.len())
            && item
                .name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            && (1..=MAX_SKILL_DESCRIPTION_CHARS).contains(&item.description.chars().count())
    })
}
pub(crate) fn skipped<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<SkippedSkill>, D::Error> {
    list(deserializer, "skipped", |item: &SkippedSkill| {
        (1..=MAX_SKILL_FOLDER_BYTES).contains(&item.folder.len())
    })
}
