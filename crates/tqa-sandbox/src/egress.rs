use url::Url;

use crate::SandboxError;

/// Explicit routing modes; neither mode is an implicit default.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EgressMode {
    /// A bridge with no external route; the gateway must join the same network.
    Internal,
    /// The same internal bridge with explicit policy-proxy environment variables.
    Proxy,
}

/// An exact hostname admitted by the gateway's policy proxy; wildcards are not implied.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostRule(String);

impl HostRule {
    /// Validates an exact ASCII hostname or address.
    ///
    /// # Errors
    /// Returns `InvalidSpec` for empty, malformed, wildcard or unbounded hosts.
    pub fn new(host: impl Into<String>) -> Result<Self, SandboxError> {
        Self::parse(host.into())
    }

    fn parse(mut host: String) -> Result<Self, SandboxError> {
        if host.is_empty()
            || host.len() > 253
            || !host.is_ascii()
            || host.contains(['*', '/', '@', '?', '#'])
            || url::Host::parse(&host).is_err()
        {
            return Err(SandboxError::invalid(
                "egress",
                "expected an exact host rule",
            ));
        }
        host.make_ascii_lowercase();
        Ok(Self(host))
    }

    /// Returns the validated host; the proxy remains responsible for enforcement.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.0
    }
}

/// A bounded policy declaration; only the gateway proxy interprets the allow list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EgressPolicy {
    /// The explicitly selected network mode.
    pub mode: EgressMode,
    /// Exact hosts to admit through the policy proxy, at most 256.
    pub allow: Vec<HostRule>,
    /// Required for Proxy and forbidden for Internal; never inferred from the runtime URL.
    pub proxy_url: Option<Url>,
}

impl EgressPolicy {
    /// Checks mode consistency and a credential-free HTTP proxy endpoint.
    ///
    /// # Errors
    /// Returns `InvalidSpec` for inconsistent or unbounded policy input.
    pub fn validate(&self) -> Result<(), SandboxError> {
        if self.allow.len() > 256 {
            return Err(SandboxError::invalid("egress", "too many host rules"));
        }
        match (self.mode, &self.proxy_url) {
            (EgressMode::Internal, None) => Ok(()),
            (EgressMode::Proxy, Some(url)) if valid_url(url, &["http", "https"]) => Ok(()),
            _ => Err(SandboxError::invalid(
                "egress",
                "proxy endpoint does not match the selected mode",
            )),
        }
    }
}

pub(crate) fn valid_url(url: &Url, schemes: &[&str]) -> bool {
    schemes.contains(&url.scheme())
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && url.as_str().len() <= 2048
}
