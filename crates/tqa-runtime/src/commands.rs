//! The usual commands the environment has, found on the runtime's own PATH inside the sandbox, so the system prompt can
//! name them (PROMPT-U-015): the image is what it is, whatever its labels say.
use std::{ffi::OsStr, os::unix::fs::PermissionsExt, path::Path};

/// The commands the prompt may name, in the order it names them. Bounded, so the line stays short.
pub(crate) const USUAL: &[&str] = &[
    "python3", "python", "pip3", "node", "npm", "npx", "go", "java", "cargo", "rustc", "gcc",
    "make", "git", "curl", "wget", "jq", "sqlite3", "chromium", "R",
];

/// The usual commands found as executable files on `path` (a PATH value), in [`USUAL`]'s order.
pub(crate) fn available(path: &OsStr) -> Vec<&'static str> {
    let directories: Vec<_> = std::env::split_paths(path).collect();
    USUAL
        .iter()
        .copied()
        .filter(|name| {
            directories
                .iter()
                .any(|directory| executable(&directory.join(name)))
        })
        .collect()
}

fn executable(path: &Path) -> bool {
    // Follows symlinks (`python` is a link to python3 in the runtime images).
    std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;
