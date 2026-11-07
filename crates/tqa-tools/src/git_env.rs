//! Git without repository hooks (ADR-0035 §1): environment configuration that outranks a repository's own, so a
//! hook or file-system monitor planted in a shared workspace's `.git` never runs in another member's task. It isn't
//! a rule the agent meets; the agent's own `git -c` still works.

/// The variables every process that may run git gets: `core.hooksPath=/dev/null` and `core.fsmonitor=false`, set
/// after any passed-through environment so a runtime-level `GIT_CONFIG_*` can't undo them.
pub const GIT_SAFETY_ENV: [(&str, &str); 5] = [
    ("GIT_CONFIG_COUNT", "2"),
    ("GIT_CONFIG_KEY_0", "core.hooksPath"),
    ("GIT_CONFIG_VALUE_0", "/dev/null"),
    ("GIT_CONFIG_KEY_1", "core.fsmonitor"),
    ("GIT_CONFIG_VALUE_1", "false"),
];

/// A run's host keys in its processes' environment (ADR-0037 §3, §7): each key's variable holding its placeholder, and
/// git's settings that use it. A `basic` key gets a credential helper for its address that answers its user name (or
/// `git`) and the variable's value, matched by path when the key has a prefix; a `bearer` key gets the header. The
/// placeholder is only ever the variable's value or the header; the helper reads the variable when git calls it.
#[derive(Clone, Debug, Default)]
pub struct HostKeyEnv {
    variables: Vec<(String, String)>,
    git: Vec<(String, String)>,
}

impl HostKeyEnv {
    /// The environment for `grants`.
    #[must_use]
    pub fn new(grants: &[tqa_types::HostKeyGrant]) -> Self {
        Self {
            variables: grants
                .iter()
                .map(|grant| (grant.env().to_owned(), grant.placeholder().to_owned()))
                .collect(),
            git: grants.iter().flat_map(git_settings).collect(),
        }
    }

    /// Every variable a process gets: the keys', then git's configuration with [`GIT_SAFETY_ENV`]'s first, so a key
    /// never displaces the safety settings.
    #[must_use]
    pub fn variables(&self) -> Vec<(String, String)> {
        let safety = GIT_SAFETY_ENV
            .iter()
            .filter(|(name, _)| *name != "GIT_CONFIG_COUNT")
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()));
        let base = (GIT_SAFETY_ENV.len() - 1) / 2;
        let keyed = self
            .git
            .iter()
            .enumerate()
            .flat_map(|(index, (key, value))| {
                [
                    (format!("GIT_CONFIG_KEY_{}", base + index), key.clone()),
                    (format!("GIT_CONFIG_VALUE_{}", base + index), value.clone()),
                ]
            });
        self.variables
            .iter()
            .cloned()
            .chain(safety)
            .chain(keyed)
            .chain(std::iter::once((
                "GIT_CONFIG_COUNT".to_owned(),
                (base + self.git.len()).to_string(),
            )))
            .collect()
    }
}

/// Git's settings for one key: for `basic`, the credential helper and user name at its address (by path when it has a
/// prefix); for `bearer`, the header.
fn git_settings(grant: &tqa_types::HostKeyGrant) -> Vec<(String, String)> {
    let port = if grant.port() == 443 {
        String::new()
    } else {
        format!(":{}", grant.port())
    };
    // An IPv6 address goes in brackets in a URL (RFC 3986); a name or an IPv4 address as it is.
    let host = if grant.host().contains(':') {
        format!("[{}]", grant.host())
    } else {
        grant.host().to_owned()
    };
    let origin = format!("https://{host}{port}");
    let url = format!("{origin}{}", grant.path_prefix().unwrap_or_default());
    match grant.scheme() {
        tqa_types::HostKeyScheme::Basic => {
            let path = grant.path_prefix().map(|_| {
                (
                    format!("credential.{origin}.useHttpPath"),
                    "true".to_owned(),
                )
            });
            path.into_iter()
                .chain([
                    (
                        format!("credential.{url}.username"),
                        grant.user_name().unwrap_or("git").to_owned(),
                    ),
                    (
                        format!("credential.{url}.helper"),
                        format!(
                            "!f() {{ test \"$1\" = get && printf 'password=%s\\n' \"${}\"; }}; f",
                            grant.env()
                        ),
                    ),
                ])
                .collect()
        }
        tqa_types::HostKeyScheme::Bearer => vec![(
            format!("http.{url}.extraHeader"),
            format!("Authorization: Bearer {}", grant.placeholder()),
        )],
    }
}
