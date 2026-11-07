//! GET-only fetch: every destination enters the authenticated policy-proxy tunnel.
mod text;
#[cfg(test)]
mod tls_tests;
mod transport;
use super::{arguments, file_io, specification};
use crate::{ProxyEndpoint, Tool, ToolContext, ToolError, ToolOutput, ToolSpec};
use async_trait::async_trait;
use hyper::header::HeaderValue;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use url::Url;

const BODY_BYTES: usize = 2 * 1024 * 1024;
const TEXT_BYTES: usize = 1024 * 1024;
const HEAD_BYTES: usize = 16 * 1024;
const TIMEOUT: Duration = Duration::from_secs(30);
/// Sent to every origin: sites such as Wikipedia and api.github.com refuse a request without one (403). The name and
/// version only (TOOL-U-054).
const USER_AGENT: &str = concat!(
    "TensorQuay-Agent/",
    env!("CARGO_PKG_VERSION"),
    " (fetch_url)"
);

/// Fetches bounded text using an explicit gateway proxy and the mounted runtime credential.
pub struct FetchUrl {
    spec: ToolSpec,
    proxy: ProxyEndpoint,
    authorization: HeaderValue,
    tls: Arc<ClientConfig>,
}
impl FetchUrl {
    /// Constructs a fetch tool without consulting process proxy or credential environment variables.
    ///
    /// # Errors
    /// Returns `invalid_args` for an empty, nonprintable or oversized runtime credential.
    pub fn new(proxy: ProxyEndpoint, token: &SecretString) -> Result<Self, ToolError> {
        let token = token.expose_secret();
        if token.is_empty()
            || token.len() > 4096
            || !token.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
        {
            return Err(ToolError::invalid("/proxy", "invalid runtime credential"));
        }
        let mut authorization = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| ToolError::invalid("/proxy", "invalid runtime credential"))?;
        authorization.set_sensitive(true);
        let tls = ClientConfig::builder_with_provider(Arc::new(
            tokio_rustls::rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|_| io_error("TLS configuration failed"))?
        .with_root_certificates(
            webpki_roots::TLS_SERVER_ROOTS
                .iter()
                .cloned()
                .collect::<RootCertStore>(),
        )
        .with_no_client_auth();
        Ok(Self {
            spec: specification(
                "fetch_url",
                "Fetch an HTTP or HTTPS page through the permitted network proxy and return text",
                json!({"type":"object","required":["url"],"additionalProperties":false,"properties":{"url":{"type":"string","minLength":1,"maxLength":4096}}}),
                false,
            ),
            proxy,
            authorization,
            tls: Arc::new(tls),
        })
    }
    async fn download(&self, mut url: Url) -> Result<transport::Page, ToolError> {
        for redirects in 0..=5 {
            match transport::fetch(self, &url).await? {
                transport::Response::Page(page) => return Ok(page),
                transport::Response::Redirect(location) if redirects < 5 => {
                    url = destination(
                        url.join(&location)
                            .map_err(|_| ToolError::invalid("/url", "invalid redirect"))?
                            .as_str(),
                    )?;
                }
                transport::Response::Redirect(_) => {
                    return Err(io_error("redirect limit exceeded"));
                }
            }
        }
        Err(io_error("redirect limit exceeded"))
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    url: String,
}
#[async_trait]
impl Tool for FetchUrl {
    // Its schema is strict-valid: optional properties are nullable, and null means absent (TOOL-U-034).
    fn null_means_absent(&self) -> bool {
        true
    }
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    async fn call(&self, args: Value, cx: &ToolContext) -> Result<ToolOutput, ToolError> {
        cx.check()?;
        let args: Args = arguments(args)?;
        let url = destination(&args.url)?;
        let address = url.to_string();
        let started = (cx.clock.now(), Instant::now());
        let mut bounded = cx.clone();
        bounded.limits.file_timeout = cx.limits.file_timeout.min(TIMEOUT);
        let page = bounded.io(self.download(url)).await?;
        let elapsed = cx
            .clock
            .now()
            .saturating_duration_since(started.0)
            .max(started.1.elapsed());
        bounded.limits.file_timeout = bounded
            .limits
            .file_timeout
            .checked_sub(elapsed)
            .filter(|remaining| !remaining.is_zero())
            .ok_or(ToolError::Timeout)?;
        let output = file_io::run(&bounded, move |cancel| text::render(&page, cancel)).await?;
        // Web content is data, never instructions: one untrusted-content block the page can't close (SEC-034).
        Ok(ToolOutput::text(crate::untrusted::wrap(
            "web",
            &[("url", &address)],
            &output.text,
        )?))
    }
}
fn destination(value: &str) -> Result<Url, ToolError> {
    let invalid = || {
        ToolError::invalid(
            "/url",
            "expected an HTTP(S) URL without credentials, at most 4096 bytes",
        )
    };
    if value.is_empty()
        || value.len() > 4096
        || value
            .split_once(':')
            .is_none_or(|(_, suffix)| !suffix.starts_with("//"))
        || value.trim() != value
        || value.split_once("://").is_some_and(|(_, tail)| {
            tail.split(['/', '?', '#'])
                .next()
                .is_some_and(|authority| authority.is_empty() || authority.contains('@'))
        })
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b'\\')
    {
        return Err(invalid());
    }
    let mut url = Url::parse(value).map_err(|_| invalid())?;
    if url.as_str().len() > 4096
        || !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(invalid());
    }
    target(&url)?;
    url.set_fragment(None);
    Ok(url)
}
fn target(url: &Url) -> Result<tqa_types::ErrorTarget, ToolError> {
    let host = url
        .host_str()
        .ok_or_else(|| ToolError::invalid("/url", "missing host"))?
        .trim_matches(['[', ']']);
    tqa_types::ErrorTarget::new(host, url.port_or_known_default().unwrap_or(0))
        .map_err(|_| ToolError::invalid("/url", "invalid destination host or port"))
}
fn io_error(detail: &str) -> ToolError {
    ToolError::Io(detail.into())
}
