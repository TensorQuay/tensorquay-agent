//! Exact environment boundary and a single bounded read of the mounted credential.
use crate::RuntimeError;
use secrecy::SecretString;
use std::{fs, io::Read as _, path::PathBuf};
use url::Url;

/// Raw startup inputs: three required variables, an optional proxy origin, `/workspace` and `/skills`.
#[derive(Debug)]
pub struct RuntimeInputs {
    /// HTTP(S) gateway origin, without a path, credentials, query or fragment.
    pub gateway_url: String,
    /// Optional credential-free proxy origin, on the same host as the gateway.
    pub egress_proxy_url: Option<String>,
    /// Read-only regular file holding the one runtime credential.
    pub token_file: PathBuf,
    /// Existing private state mount outside the workspace.
    pub state_dir: PathBuf,
    /// Existing workspace mount; configurable only by the library's test caller.
    pub workspace: PathBuf,
    /// The read-only skills mount, with the `install` and `user` tiers below it; it may be absent. Configurable only
    /// by the library's test caller.
    pub skills: PathBuf,
    /// The backend property `sync_before_result` (`TQA_SYNC_BEFORE_RESULT`): flush the workspace before journaling
    /// each tool result. The microVM backend sets it; Docker does not.
    pub sync_before_result: bool,
}

/// Parses `TQA_SYNC_BEFORE_RESULT`: absent is false, exactly `1` is true, anything else is refused (fail closed).
///
/// # Errors
/// `Configuration("TQA_SYNC_BEFORE_RESULT")` for any other value.
pub fn sync_before_result(value: Option<&str>) -> Result<bool, RuntimeError> {
    match value {
        None => Ok(false),
        Some("1") => Ok(true),
        Some(_) => Err(RuntimeError::Configuration("TQA_SYNC_BEFORE_RESULT")),
    }
}
impl RuntimeInputs {
    /// Reads the documented startup variables; ambient HTTP proxy and logging variables are ignored.
    ///
    /// # Errors
    /// Rejects missing or non-Unicode configuration values without including their contents.
    pub fn from_env() -> Result<Self, RuntimeError> {
        let read = |key| std::env::var(key).map_err(|_| RuntimeError::Configuration(key));
        Ok(Self {
            gateway_url: read("TQA_GATEWAY_URL")?,
            egress_proxy_url: match std::env::var("TQA_EGRESS_PROXY_URL") {
                Ok(value) => Some(value),
                Err(std::env::VarError::NotPresent) => None,
                Err(_) => return Err(RuntimeError::Configuration("TQA_EGRESS_PROXY_URL")),
            },
            token_file: read("TQA_RUNTIME_TOKEN_FILE")?.into(),
            state_dir: read("TQA_STATE_DIR")?.into(),
            workspace: "/workspace".into(),
            skills: "/skills".into(),
            sync_before_result: sync_before_result(
                match std::env::var("TQA_SYNC_BEFORE_RESULT") {
                    Ok(value) => Some(value),
                    Err(std::env::VarError::NotPresent) => None,
                    Err(_) => return Err(RuntimeError::Configuration("TQA_SYNC_BEFORE_RESULT")),
                }
                .as_deref(),
            )?,
        })
    }
}

/// Validated process settings; the credential has no printable debug representation.
#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    pub(crate) origin: Url,
    pub(crate) proxy: Option<tqa_tools::ProxyEndpoint>,
    pub(crate) token: SecretString,
    pub(crate) state_dir: PathBuf,
    pub(crate) workspace: PathBuf,
    pub(crate) skills: PathBuf,
    pub(crate) sync_before_result: bool,
}
impl RuntimeConfig {
    /// Validates paths and origin, then reads the credential once. Call from a blocking worker.
    ///
    /// # Errors
    /// Rejects nonregular/oversize/non-ASCII credentials, invalid origins and overlapping mounts.
    pub fn load(inputs: RuntimeInputs) -> Result<Self, RuntimeError> {
        let origin = origin(&inputs.gateway_url)?;
        let proxy = inputs
            .egress_proxy_url
            .as_deref()
            .map(|value| {
                let error = || RuntimeError::Configuration("TQA_EGRESS_PROXY_URL");
                let endpoint = tqa_tools::ProxyEndpoint::new(value).map_err(|_| error())?;
                if origin.host_str().map(|host| host.trim_matches(['[', ']']))
                    != Some(endpoint.host())
                {
                    return Err(error());
                }
                Ok(endpoint)
            })
            .transpose()?;
        let workspace = directory(inputs.workspace, "workspace mount")?;
        let state_dir = directory(inputs.state_dir, "TQA_STATE_DIR")?;
        if state_dir.starts_with(&workspace) || workspace.starts_with(&state_dir) {
            return Err(RuntimeError::Configuration("state and workspace overlap"));
        }
        let token = read_token(&inputs.token_file)?;
        Ok(Self {
            origin,
            proxy,
            token,
            state_dir,
            workspace,
            skills: inputs.skills,
            sync_before_result: inputs.sync_before_result,
        })
    }
    /// Returns the derived WebSocket registration endpoint on the configured origin.
    #[must_use]
    pub fn link_url(&self) -> Url {
        let mut url = self.origin.clone();
        // Both known scheme replacements retain a special URL's authority.
        let _ = url.set_scheme(if self.origin.scheme() == "https" {
            "wss"
        } else {
            "ws"
        });
        url.set_path("/runtime/link");
        url
    }
    /// Returns the screen bridge's WebSocket endpoint on the configured origin (ADR-0039 section 5).
    #[must_use]
    pub fn screen_url(&self) -> Url {
        let mut url = self.link_url();
        url.set_path("/runtime/screen");
        url
    }
    /// Returns the gateway-authoritative history endpoint on the configured origin.
    #[must_use]
    pub fn history_url(&self) -> Url {
        let mut url = self.origin.clone();
        url.set_path("/runtime/history");
        url
    }
}
fn origin(value: &str) -> Result<Url, RuntimeError> {
    let error = || RuntimeError::Configuration("TQA_GATEWAY_URL");
    let raw_path = value
        .split_once("://")
        .and_then(|(_, authority)| authority.split_once('/'))
        .map(|(_, path)| path);
    if value.is_empty()
        || value.len() > 2048
        || value.trim() != value
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b'\\')
        || raw_path.is_some_and(|path| !path.is_empty())
    {
        return Err(error());
    }
    let url = Url::parse(value).map_err(|_| error())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !matches!(url.path(), "" | "/")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(error());
    }
    Ok(url)
}
fn directory(path: PathBuf, field: &'static str) -> Result<PathBuf, RuntimeError> {
    let canonical = fs::canonicalize(path).map_err(|_| RuntimeError::Configuration(field))?;
    if !canonical.is_dir() {
        return Err(RuntimeError::Configuration(field));
    }
    Ok(canonical)
}
fn read_token(path: &std::path::Path) -> Result<SecretString, RuntimeError> {
    let error = || RuntimeError::Configuration("TQA_RUNTIME_TOKEN_FILE");
    let metadata = fs::symlink_metadata(path).map_err(|_| error())?;
    if !metadata.is_file() || metadata.len() > 4096 {
        return Err(error());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| error())?
        .take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| error())?;
    if bytes.len() > 4096 {
        return Err(error());
    }
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    if bytes.is_empty() || !bytes.iter().all(|value| (0x20..=0x7e).contains(value)) {
        return Err(error());
    }
    let text = String::from_utf8(bytes).map_err(|_| error())?;
    Ok(SecretString::from(text))
}

#[cfg(test)]
mod sync_tests {
    use super::{RuntimeError, sync_before_result};

    #[test]
    fn rt_i_024_only_an_absent_value_or_exactly_one_is_accepted() {
        assert!(matches!(sync_before_result(None), Ok(false)));
        assert!(matches!(sync_before_result(Some("1")), Ok(true)));
        for value in ["0", "true", "", " 1", "11"] {
            assert!(
                matches!(
                    sync_before_result(Some(value)),
                    Err(RuntimeError::Configuration("TQA_SYNC_BEFORE_RESULT"))
                ),
                "{value:?}"
            );
        }
    }
}
