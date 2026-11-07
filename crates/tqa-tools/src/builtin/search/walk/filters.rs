//! Path selection and explicit dot-component opt-in without overriding ignore rules.
use super::Options;
use crate::{MAX_PATH_BYTES, MAX_PATH_COMPONENTS, ToolError, WorkspaceRoot};
use globset::{Glob, GlobMatcher};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(super) struct Filters {
    root: PathBuf,
    target: PathBuf,
    glob: Option<GlobMatcher>,
    dots: Vec<GlobMatcher>,
}
impl Filters {
    pub(super) fn new(workspace: &WorkspaceRoot, options: &Options<'_>) -> Result<Self, ToolError> {
        let target = workspace.resolve(options.path)?;
        reject_symlinks(workspace, options.path)?;
        let relative = target
            .strip_prefix(workspace.path())
            .map_err(|_| ToolError::PathOutsideWorkspace)?;
        let mut dots = Vec::new();
        for part in relative
            .iter()
            .filter_map(|part| part.to_str())
            .chain(options.glob.unwrap_or("").split('/'))
        {
            if part.starts_with('.') && part != "." && part != ".." {
                dots.push(compile(part)?);
            }
        }
        Ok(Self {
            root: workspace.path().into(),
            target,
            glob: options.glob.map(compile).transpose()?,
            dots,
        })
    }
    pub(super) fn matches(&self, entry: &ignore::DirEntry) -> Result<bool, ToolError> {
        let Some(kind) = entry.file_type() else {
            return Ok(false);
        };
        if kind.is_symlink() || !(kind.is_file() || kind.is_dir()) {
            return Ok(false);
        }
        let path = entry.path();
        if !(path.starts_with(&self.target) || kind.is_dir() && self.target.starts_with(path)) {
            return Ok(false);
        }
        let relative = path
            .strip_prefix(&self.root)
            .map_err(|_| ToolError::PathOutsideWorkspace)?;
        let text = relative
            .to_str()
            .ok_or_else(|| ToolError::PathInvalid("search paths must be UTF-8".into()))?;
        if text.len() > MAX_PATH_BYTES
            || relative.components().count() > MAX_PATH_COMPONENTS
            || text.chars().any(char::is_control)
        {
            return Err(ToolError::PathInvalid(
                "search path exceeds bounds or contains control characters".into(),
            ));
        }
        if relative
            .iter()
            .filter_map(|part| part.to_str())
            .any(|part| part.starts_with('.') && !self.dots.iter().any(|glob| glob.is_match(part)))
        {
            return Ok(false);
        }
        Ok(kind.is_dir()
            || self
                .glob
                .as_ref()
                .is_none_or(|glob| glob.is_match(relative)))
    }
}
fn compile(pattern: &str) -> Result<GlobMatcher, ToolError> {
    Glob::new(pattern)
        .map(|glob| glob.compile_matcher())
        .map_err(|_| ToolError::invalid("/glob", "invalid path glob"))
}
fn reject_symlinks(workspace: &WorkspaceRoot, path: &str) -> Result<(), ToolError> {
    let path = Path::new(path);
    let relative = if path.is_absolute() {
        path.strip_prefix(workspace.path())
            .map_err(|_| ToolError::PathOutsideWorkspace)?
    } else {
        path
    };
    let mut current = workspace.path().to_path_buf();
    for component in relative.components() {
        current.push(component);
        if fs::symlink_metadata(&current)?.file_type().is_symlink() {
            return Err(ToolError::PathInvalid(
                "search path must not traverse symlinks".into(),
            ));
        }
    }
    Ok(())
}
