//! An explicit, credential-free gateway proxy endpoint.
use crate::ToolError;
use url::Url;

/// A bounded HTTP(S) origin. Runtime admission additionally binds its host to the gateway.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProxyEndpoint {
    pub(crate) url: Url,
    host: String,
}
impl ProxyEndpoint {
    /// Parses an origin without credentials, paths, query parameters or fragments.
    ///
    /// # Errors
    /// Returns `invalid_args` for malformed endpoints, zero ports or more than 2048 bytes.
    pub fn new(value: &str) -> Result<Self, ToolError> {
        let error = || ToolError::invalid("/proxy", "expected a credential-free HTTP(S) origin");
        if value.is_empty()
            || value.len() > 2048
            || value
                .split_once(':')
                .is_none_or(|(_, suffix)| !suffix.starts_with("//"))
            || value.trim() != value
            || value
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte == b'\\')
            || value.split_once("://").is_some_and(|(_, tail)| {
                tail.split('/')
                    .next()
                    .is_some_and(|authority| authority.is_empty() || authority.contains('@'))
            })
            || value
                .split_once("://")
                .and_then(|(_, authority)| authority.split_once('/'))
                .is_some_and(|(_, path)| !path.is_empty())
        {
            return Err(error());
        }
        let url = Url::parse(value).map_err(|_| error())?;
        if url.as_str().len() > 2048
            || !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.port_or_known_default().is_none_or(|port| port == 0)
        {
            return Err(error());
        }
        let host = url
            .host_str()
            .ok_or_else(error)?
            .trim_matches(['[', ']'])
            .to_owned();
        Ok(Self { url, host })
    }
    /// Returns the credential-free canonical origin, including its trailing slash.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.url.as_str()
    }
    /// Returns the canonical host without IPv6 brackets, for gateway binding and proxy bypass of the gateway itself.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }
}
