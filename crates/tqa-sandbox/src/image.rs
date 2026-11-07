use std::{fmt, str::FromStr};

use crate::SandboxError;

/// An immutable registry reference or host-local image ID, both pinned by SHA-256.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageRef {
    source: Source,
    digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Source {
    Registry(String),
    LocalId,
}

impl ImageRef {
    /// Validates the repository name and digest without contacting a registry.
    ///
    /// # Errors
    /// Returns `InvalidSpec` for empty, unbounded or malformed identifiers.
    pub fn new(name: impl Into<String>, digest: impl Into<String>) -> Result<Self, SandboxError> {
        let name = name.into();
        if name.is_empty()
            || name.len() > 255
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"/.:_-".contains(&byte))
        {
            return Err(SandboxError::invalid("image", "invalid image name"));
        }
        Ok(Self {
            source: Source::Registry(name),
            digest: validated_digest(digest.into())?,
        })
    }

    /// Pins an image already built on the target host; it is never pulled from a registry.
    ///
    /// # Errors
    /// Returns `InvalidSpec` unless the ID is `sha256:` followed by 64 lowercase hex digits.
    pub fn local_id(digest: impl Into<String>) -> Result<Self, SandboxError> {
        Ok(Self {
            source: Source::LocalId,
            digest: validated_digest(digest.into())?,
        })
    }

    /// Reports whether this pin requires an image already present on the target host.
    #[must_use]
    pub const fn is_local(&self) -> bool {
        matches!(self.source, Source::LocalId)
    }

    /// Returns the immutable digest, including its algorithm prefix.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// Returns the registry repository name, or `None` for a host-local image ID.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        match &self.source {
            Source::Registry(name) => Some(name),
            Source::LocalId => None,
        }
    }
}

impl fmt::Display for ImageRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(name) = self.name() {
            write!(formatter, "{name}@")?;
        }
        formatter.write_str(&self.digest)
    }
}

impl FromStr for ImageRef {
    type Err = SandboxError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.rsplit_once('@') {
            Some((name, digest)) => Self::new(name, digest),
            None => Self::local_id(value),
        }
    }
}

fn validated_digest(digest: String) -> Result<String, SandboxError> {
    if !digest.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }) {
        return Err(SandboxError::invalid(
            "image",
            "a lowercase SHA-256 digest is required",
        ));
    }
    Ok(digest)
}
