//! Secret-store names that belong to one user (FR-19, FR-24) or to a host key (ADR-0037): a user's own Git and connector
//! credentials, and host keys' values. Only their own calls use them; no administrator may write them through the
//! shared secrets, and no route or connector may name them.

/// The name prefixes of per-user credentials, compared without case.
pub const PER_USER_SECRET_PREFIXES: [&str; 3] = ["git-", "connector-", "host-"];

/// The longest secret name after `secrets/`.
pub const MAX_SECRET_NAME_BYTES: usize = 120;

/// Whether `value` is a secret-store reference, `secrets/<name>` with a name of 1 through 120 letters, digits, `-` or
/// `_`: what a route's `key_ref` must be, never a secret value itself (SEC-033).
#[must_use]
pub fn is_secret_reference(value: &str) -> bool {
    value.strip_prefix("secrets/").is_some_and(|name| {
        (1..=MAX_SECRET_NAME_BYTES).contains(&name.len())
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    })
}

/// Whether `name` (with or without the `secrets/` prefix) names a user's own credential.
#[must_use]
pub fn is_per_user_secret(name: &str) -> bool {
    let name = name.strip_prefix("secrets/").unwrap_or(name);
    PER_USER_SECRET_PREFIXES.iter().any(|prefix| {
        name.get(..prefix.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
    })
}

#[cfg(test)]
mod tests {
    use super::{is_per_user_secret, is_secret_reference};

    // TEST: SEC-033
    #[test]
    fn sec_033_only_secrets_slash_a_bounded_name_is_a_reference() {
        let longest = format!("secrets/{}", "n".repeat(120));
        for value in [
            "secrets/provider",
            "secrets/a",
            "secrets/A-b_9",
            longest.as_str(),
        ] {
            assert!(is_secret_reference(value), "{value}");
        }
        let long = format!("secrets/{}", "n".repeat(121));
        for value in [
            "sk-live-abc",
            "secrets/",
            "secrets/has space",
            "secrets/a/b",
            "secrets/é",
            "Secrets/x",
            "https://keys.example/k",
            "",
            long.as_str(),
        ] {
            assert!(!is_secret_reference(value), "{value}");
        }
    }

    // TEST: SEC-031
    #[test]
    fn sec_031_per_user_names_are_recognised_with_or_without_the_prefix_and_in_any_case() {
        for name in [
            "git-abc",
            "secrets/git-abc",
            "GIT-abc",
            "secrets/connector-abc",
            "Connector-x",
            // A host key's value (ADR-0037).
            "secrets/host-abc",
            "HOST-x",
        ] {
            assert!(is_per_user_secret(name), "{name}");
        }
        for name in [
            "openai",
            "secrets/github-token",
            "secrets/connectors",
            "secrets/hosts",
            "gi",
            "secrets/",
        ] {
            assert!(!is_per_user_secret(name), "{name}");
        }
    }
}
