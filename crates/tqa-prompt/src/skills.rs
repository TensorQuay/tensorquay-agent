//! The skills directory (PRD FR-11 and §6.1, PROMPT-U-002): folders with a `SKILL.md` in an install tier and a user
//! tier. Discovery is bounded and never fails a run: an unusable folder is skipped with a stable reason, which the
//! runtime reports to the context panel. Only a byte-stable index reaches the prompt; the body is loaded on demand.
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Skills per run across all tiers; the rest are skipped by name order.
pub const MAX_SKILLS: usize = 64;
/// The largest `SKILL.md`.
pub const SKILL_MANIFEST_BYTES: u64 = 16 * 1024;
/// The largest skill folder, all files together.
pub const SKILL_FOLDER_BYTES: u64 = 1024 * 1024;
/// The largest index in the prompt.
pub const SKILL_INDEX_BYTES: usize = 8 * 1024;
/// The longest description.
const DESCRIPTION_CHARS: usize = 280;
/// The longest name.
const NAME_BYTES: usize = 64;
/// Files and directories examined in one skill folder; a larger tree is too large.
const FOLDER_ENTRIES: usize = 1000;
const INDEX_HEADER: &str =
    "Skills available in this environment (load one with the `skill` tool before using it):\n";
const INDEX_MARKER: &str = "[skills index truncated: byte limit]\n";

pub use tqa_types::{SkillSkipReason as SkipReason, SkillTier};

/// A parsed `SKILL.md`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Manifest {
    /// The skill's name, equal to its folder name.
    pub name: String,
    /// One line the model sees before it chooses the skill.
    pub description: String,
    /// The instructions after the front matter.
    pub body: String,
}

/// A loaded skill.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Skill {
    /// The skill's name.
    pub name: String,
    /// Its description.
    pub description: String,
    /// Its tier.
    pub tier: SkillTier,
    /// Its folder.
    pub path: PathBuf,
}

/// A folder that was not loaded, and why.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SkippedSkill {
    /// The folder name.
    pub folder: String,
    /// Its tier.
    pub tier: SkillTier,
    /// Why.
    pub reason: SkipReason,
}

/// The skills of one run, sorted by name, and what was skipped, in tier and folder order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SkillSet {
    /// The loaded skills.
    pub skills: Vec<Skill>,
    /// The skipped folders.
    pub skipped: Vec<SkippedSkill>,
}

/// Parses a `SKILL.md` belonging to the folder `folder`.
///
/// # Errors
/// [`SkipReason::BadFrontMatter`] or [`SkipReason::NameMismatch`].
pub fn parse_manifest(folder: &str, text: &str) -> Result<Manifest, SkipReason> {
    let mut lines = text.split_inclusive('\n');
    let line = |raw: &str| raw.trim_end_matches(['\n', '\r']).to_owned();
    if lines.next().map(line).as_deref() != Some("---") {
        return Err(SkipReason::BadFrontMatter);
    }
    let (mut name, mut description) = (None, None);
    loop {
        let current = lines.next().map(line).ok_or(SkipReason::BadFrontMatter)?;
        if current == "---" {
            break;
        }
        let (key, value) = current.split_once(':').ok_or(SkipReason::BadFrontMatter)?;
        let slot = match key {
            "name" => &mut name,
            "description" => &mut description,
            _ => return Err(SkipReason::BadFrontMatter),
        };
        if slot.replace(value.trim().to_owned()).is_some() {
            return Err(SkipReason::BadFrontMatter);
        }
    }
    let (Some(name), Some(description)) = (name, description) else {
        return Err(SkipReason::BadFrontMatter);
    };
    let described = (1..=DESCRIPTION_CHARS).contains(&description.chars().count())
        && !description.chars().any(char::is_control);
    if !described {
        return Err(SkipReason::BadFrontMatter);
    }
    let valid_name = (1..=NAME_BYTES).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    if !valid_name || name != folder {
        return Err(SkipReason::NameMismatch);
    }
    Ok(Manifest {
        name,
        description,
        body: lines.collect(),
    })
}

/// Discovers the skills of the given tier roots; a missing root is an empty tier. Performs bounded synchronous I/O.
#[must_use]
pub fn discover_skills(roots: &[(SkillTier, &Path)]) -> SkillSet {
    let mut set = SkillSet::default();
    let mut found: Vec<Skill> = Vec::new();
    for &(tier, root) in roots {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        let mut folders: Vec<(String, PathBuf)> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| Some((entry.file_name().into_string().ok()?, entry.path())))
            .collect();
        folders.sort();
        for (folder, path) in folders {
            match load(&folder, &path) {
                Ok(Some((manifest, _))) => found.push(Skill {
                    name: manifest.name,
                    description: manifest.description,
                    tier,
                    path,
                }),
                Ok(None) => {}
                Err(reason) => set.skipped.push(SkippedSkill {
                    folder,
                    tier,
                    reason,
                }),
            }
        }
    }
    // The most specific tier wins a name clash; the loser is reported, never dropped silently.
    found.sort_by(|a, b| a.name.cmp(&b.name).then(b.tier.cmp(&a.tier)));
    for skill in found {
        let shadowed = set
            .skills
            .last()
            .is_some_and(|kept: &Skill| kept.name == skill.name);
        let reason = if shadowed {
            Some(SkipReason::Shadowed)
        } else if set.skills.len() == MAX_SKILLS {
            Some(SkipReason::OverLimit)
        } else {
            None
        };
        match reason {
            Some(reason) => set.skipped.push(SkippedSkill {
                folder: skill.name,
                tier: skill.tier,
                reason,
            }),
            None => set.skills.push(skill),
        }
    }
    set
}

/// Loads one entry of a tier root with its files: `Ok(None)` for a plain file, which is not a skill.
pub(crate) fn load(
    folder: &str,
    path: &Path,
) -> Result<Option<(Manifest, Vec<PathBuf>)>, SkipReason> {
    let kind = fs::symlink_metadata(path)
        .map_err(|_| SkipReason::Unreadable)?
        .file_type();
    if kind.is_symlink() {
        return Err(SkipReason::Symlink);
    }
    if !kind.is_dir() {
        return Ok(None);
    }
    let manifest_path = path.join("SKILL.md");
    let manifest = match fs::symlink_metadata(&manifest_path) {
        Ok(metadata) if metadata.file_type().is_symlink() => return Err(SkipReason::Symlink),
        Ok(metadata) if !metadata.is_file() => return Err(SkipReason::MissingManifest),
        Ok(metadata) if metadata.len() > SKILL_MANIFEST_BYTES => return Err(SkipReason::TooLarge),
        Ok(_) => fs::read(&manifest_path).map_err(|_| SkipReason::Unreadable)?,
        Err(_) => return Err(SkipReason::MissingManifest),
    };
    let files = skill_files(path)?;
    let text = String::from_utf8(manifest).map_err(|_| SkipReason::NotUtf8)?;
    parse_manifest(folder, &text).map(|manifest| Some((manifest, files)))
}

/// Lists the files of a skill folder, relative to it and sorted, without following links: no symbolic link anywhere,
/// at most [`SKILL_FOLDER_BYTES`] in total and a bounded number of entries.
fn skill_files(root: &Path) -> Result<Vec<PathBuf>, SkipReason> {
    let (mut pending, mut files) = (vec![root.to_path_buf()], Vec::new());
    let (mut total, mut entries) = (0_u64, 0_usize);
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).map_err(|_| SkipReason::Unreadable)? {
            let path = entry.map_err(|_| SkipReason::Unreadable)?.path();
            entries += 1;
            let metadata = fs::symlink_metadata(&path).map_err(|_| SkipReason::Unreadable)?;
            if metadata.file_type().is_symlink() {
                return Err(SkipReason::Symlink);
            }
            if metadata.is_dir() {
                pending.push(path);
            } else {
                total = total.saturating_add(metadata.len());
                files.push(
                    path.strip_prefix(root)
                        .map_err(|_| SkipReason::Unreadable)?
                        .to_path_buf(),
                );
            }
            if total > SKILL_FOLDER_BYTES || entries > FOLDER_ENTRIES {
                return Err(SkipReason::TooLarge);
            }
        }
    }
    files.sort();
    Ok(files)
}

/// The index for the instruction slot: one line per skill in name order, bounded with a marker; empty without skills.
#[must_use]
pub fn render_skill_index(set: &SkillSet) -> String {
    if set.skills.is_empty() {
        return String::new();
    }
    let mut index = INDEX_HEADER.to_owned();
    for skill in &set.skills {
        let line = format!(
            "- {} ({}): {}\n",
            skill.name,
            skill.tier.label(),
            skill.description
        );
        if index.len() + line.len() + INDEX_MARKER.len() > SKILL_INDEX_BYTES {
            index.push_str(INDEX_MARKER);
            break;
        }
        index.push_str(&line);
    }
    index
}
