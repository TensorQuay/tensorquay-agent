//! `task.toml` (TK-U-002): parsed once, bounded and checked against the task directory before any run.
use serde::Deserialize;
use std::path::{Component, Path};

/// Largest accepted manifest; a real one is a few hundred bytes.
const MANIFEST_BYTES: u64 = 64 * 1024;

/// One task of the set, as declared by its directory's `task.toml`.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Stable identifier; equal to the directory name.
    pub id: String,
    /// A one-line human title.
    pub title: String,
    /// Free tags such as a language or `fallback-relevant`.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Limits the driver enforces by cancelling the run.
    pub budgets: Budgets,
    /// Workspace paths the model must not change; the check restores and compares them.
    #[serde(default)]
    pub protected: Vec<String>,
    /// How the check runs.
    pub check: Check,
    /// The task is sized to need at least one compaction on a small window. A real-model attempt without one
    /// is reported as a harness expectation, never as the model's failure.
    #[serde(default)]
    pub expects_compaction: bool,
    /// The environment template the conversation runs in, for example `workstation`; absent uses the run's.
    #[serde(default)]
    pub template: Option<String>,
}

/// Per-attempt budgets.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Budgets {
    /// Completed steps, 1 through 500 (the product's own ceiling).
    pub max_steps: u32,
    /// Wall-clock seconds, 1 through 86,400.
    pub wall_clock_secs: u32,
    /// Prompt plus completion tokens summed over the attempt, 1 through 50,000,000.
    pub max_tokens: u64,
}

/// The container image family the check runs in.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CheckImage {
    /// The runtime image the agent used.
    Runtime,
    /// The Workstation template, for toolchains the runtime image lacks.
    Workstation,
}

/// Check settings.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Check {
    /// The image family.
    pub image: CheckImage,
    /// Seconds before the check is stopped and reported as broken, 1 through 3,600.
    pub timeout_secs: u32,
}

/// Why a task directory is not a valid task.
#[derive(Debug, Eq, PartialEq)]
pub enum ManifestError {
    /// `task.toml` is missing, unreadable or larger than its bound.
    Unreadable,
    /// The TOML does not match the schema; the parser's message says where.
    Schema(String),
    /// A field is outside its bound; the name says which.
    Invalid(&'static str),
    /// A required file or directory is missing: `task.md`, `seed/` or `check/run.sh`.
    Missing(&'static str),
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreadable => {
                formatter.write_str("task.toml is missing, unreadable or too large")
            }
            Self::Schema(message) => {
                write!(formatter, "task.toml does not match the schema: {message}")
            }
            Self::Invalid(field) => write!(formatter, "task.toml field {field} is out of bounds"),
            Self::Missing(part) => write!(formatter, "the task has no {part}"),
        }
    }
}
impl std::error::Error for ManifestError {}

impl Manifest {
    /// Reads and validates the task in `directory`.
    ///
    /// # Errors
    /// Any [`ManifestError`].
    pub fn load(directory: &Path) -> Result<Self, ManifestError> {
        let path = directory.join("task.toml");
        let size = std::fs::metadata(&path)
            .map_err(|_| ManifestError::Unreadable)?
            .len();
        if size > MANIFEST_BYTES {
            return Err(ManifestError::Unreadable);
        }
        let text = std::fs::read_to_string(&path).map_err(|_| ManifestError::Unreadable)?;
        let manifest: Self =
            toml::from_str(&text).map_err(|error| ManifestError::Schema(error.message().into()))?;
        manifest.validate(directory)?;
        Ok(manifest)
    }
    fn validate(&self, directory: &Path) -> Result<(), ManifestError> {
        let named = directory.file_name().and_then(|name| name.to_str()) == Some(self.id.as_str());
        let id_chars = self
            .id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
        check(named && id_chars && (1..=64).contains(&self.id.len()), "id")?;
        check((1..=120).contains(&self.title.chars().count()), "title")?;
        let template = self.template.as_deref().unwrap_or("default");
        let template_chars = template
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
        check(
            template_chars && (1..=64).contains(&template.len()),
            "template",
        )?;
        let budgets = &self.budgets;
        check((1..=500).contains(&budgets.max_steps), "budgets.max_steps")?;
        check(
            (1..=86_400).contains(&budgets.wall_clock_secs),
            "budgets.wall_clock_secs",
        )?;
        check(
            (1..=50_000_000).contains(&budgets.max_tokens),
            "budgets.max_tokens",
        )?;
        check(
            (1..=3600).contains(&self.check.timeout_secs),
            "check.timeout_secs",
        )?;
        check(
            self.protected.iter().all(|path| relative(path)),
            "protected",
        )?;
        let text = std::fs::read_to_string(directory.join("task.md")).unwrap_or_default();
        check(!text.trim().is_empty(), "task.md").map_err(|_| ManifestError::Missing("task.md"))?;
        if !directory.join("seed").is_dir() {
            return Err(ManifestError::Missing("seed/"));
        }
        if !directory.join("check/run.sh").is_file() {
            return Err(ManifestError::Missing("check/run.sh"));
        }
        Ok(())
    }
}

/// The most follow-up messages a task may have, and the largest one.
const MAX_FOLLOWUPS: usize = 100;
const FOLLOWUP_BYTES: u64 = 16 * 1024;

/// The task's follow-up messages from `followups/*.md`, in file-name order; none when the directory is absent.
///
/// # Errors
/// [`ManifestError::Invalid`] naming `followups` for anything but 1 to 100 non-empty `.md` files of at most 16 KiB.
pub fn load_followups(directory: &Path) -> Result<Vec<String>, ManifestError> {
    let folder = directory.join("followups");
    if !folder.exists() {
        return Ok(Vec::new());
    }
    let invalid = || ManifestError::Invalid("followups");
    let mut paths: Vec<_> = std::fs::read_dir(&folder)
        .map_err(|_| invalid())?
        .map(|entry| entry.map(|entry| entry.path()).map_err(|_| invalid()))
        .collect::<Result<_, _>>()?;
    paths.sort();
    check((1..=MAX_FOLLOWUPS).contains(&paths.len()), "followups")?;
    paths
        .iter()
        .map(|path| {
            let markdown = path.extension().is_some_and(|extension| extension == "md");
            let size = std::fs::metadata(path).map_err(|_| invalid())?.len();
            check(
                markdown && path.is_file() && size <= FOLLOWUP_BYTES,
                "followups",
            )?;
            let text = std::fs::read_to_string(path).map_err(|_| invalid())?;
            check(!text.trim().is_empty(), "followups")?;
            Ok(text)
        })
        .collect()
}

pub(super) fn check(valid: bool, field: &'static str) -> Result<(), ManifestError> {
    if valid {
        Ok(())
    } else {
        Err(ManifestError::Invalid(field))
    }
}

/// A normal relative path that stays inside the workspace.
fn relative(path: &str) -> bool {
    !path.is_empty()
        && Path::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

#[cfg(test)]
mod tests;
