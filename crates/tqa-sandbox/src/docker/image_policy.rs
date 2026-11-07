use crate::{SandboxError, SandboxSpec};
use bollard::models::ImageInspect;

pub(super) fn validate_image(image: &ImageInspect, spec: &SandboxSpec) -> Result<(), SandboxError> {
    let matches_pin = if spec.image.is_local() {
        image.id.as_deref() == Some(spec.image.digest())
    } else {
        image.repo_digests.as_ref().is_some_and(|digests| {
            digests.iter().any(|digest| {
                digest
                    .rsplit_once('@')
                    .is_some_and(|(_, digest)| digest == spec.image.digest())
            })
        })
    };
    if !matches_pin {
        return Err(SandboxError::ImageDigestMismatch);
    }
    if let Some(config) = &image.config {
        if config
            .volumes
            .as_ref()
            .is_some_and(|volumes| !volumes.is_empty())
        {
            return Err(SandboxError::invalid(
                "image",
                "image-declared volumes would add unapproved mounts",
            ));
        }
        let unset: Vec<Option<&str>> = config
            .env
            .iter()
            .flatten()
            .filter_map(|entry| match entry.split_once('=') {
                Some((key, _)) if spec.env.contains_key(key) => None,
                Some((key, _)) => Some(Some(key)),
                None => Some(None),
            })
            .collect();
        if !unset.is_empty() {
            return Err(SandboxError::InvalidSpec {
                field: "image",
                reason: unset_environment(&unset).into(),
            });
        }
    }
    Ok(())
}

/// Keys named in one refusal, and the bytes shown of each.
const LISTED_KEYS: usize = 8;
const KEY_BYTES: usize = 64;

/// Names the image environment keys the specification leaves unset, so an operator can add them. Only key names
/// appear, bounded in count and length; values and entries without `=` are never echoed.
pub(crate) fn unset_environment(unset: &[Option<&str>]) -> String {
    let names: Vec<String> = unset
        .iter()
        .take(LISTED_KEYS)
        .map(|key| key.map_or_else(|| "an entry without '='".to_owned(), shown))
        .collect();
    let more = match unset.len().checked_sub(LISTED_KEYS) {
        Some(hidden) if hidden > 0 => format!(", and {hidden} more"),
        _ => String::new(),
    };
    format!(
        "the image declares environment keys the specification does not set: {}{more}",
        names.join(", ")
    )
}

/// A key as shown in a refusal: printable name characters only, truncated.
fn shown(key: &str) -> String {
    let mut name: String = key
        .chars()
        .take(KEY_BYTES)
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '-') {
                character
            } else {
                '?'
            }
        })
        .collect();
    if key.chars().count() > KEY_BYTES {
        name.push_str("...");
    }
    name
}
