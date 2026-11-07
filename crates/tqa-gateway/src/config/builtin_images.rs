//! `[runtime.builtin_images]` (GW-U-120): the image of each named built-in environment, by its digest. A built-in named
//! here is that image (its label must still agree), never the image the engine's `tqa-runtime:<name>` tag points at:
//! for an install that must run exactly those images, and for test stacks on an engine shared with other builds. The
//! rest are found by their tag, as without the key.
use std::collections::BTreeMap;

use crate::GatewayError;

/// The pinned built-in images, validated: built-in names only, image IDs only.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BuiltinImages(BTreeMap<String, String>);

impl BuiltinImages {
    /// Validates the file's table.
    ///
    /// # Errors
    /// A name that isn't a built-in's, or an image that isn't an image ID.
    pub(crate) fn new(table: BTreeMap<String, String>) -> Result<Self, GatewayError> {
        for (name, image) in &table {
            if !crate::environments::builtin::BUILT_INS.contains(&name.as_str()) {
                return Err(GatewayError::Configuration(
                    "runtime.builtin_images names only the built-ins minimal, workstation and data",
                ));
            }
            if !digest(image) {
                return Err(GatewayError::Configuration(
                    "runtime.builtin_images takes image IDs (sha256: and 64 lowercase hex digits), never a tag",
                ));
            }
        }
        Ok(Self(table))
    }

    /// The image pinned for the built-in `name`, if any.
    pub(crate) fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }
}

/// An image ID as the engine reports it: `sha256:` and 64 lowercase hex digits.
fn digest(image: &str) -> bool {
    image.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

#[cfg(test)]
#[path = "builtin_images_tests.rs"]
mod tests;
