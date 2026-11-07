use crate::GatewayError;
use reqwest::{
    Client, Method, RequestBuilder,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use secrecy::{ExposeSecret, SecretString};
use tqa_llm::{Auth, Protocol, Route};

/// One upstream operation; each completion operation belongs to one protocol.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Operation {
    Chat,
    Responses,
    Messages,
    Models,
}
impl Operation {
    fn path(self) -> &'static str {
        match self {
            Self::Chat => "chat/completions",
            Self::Responses => "responses",
            Self::Messages => "messages",
            Self::Models => "models",
        }
    }
    fn method(self) -> Method {
        match self {
            Self::Models => Method::GET,
            _ => Method::POST,
        }
    }
    /// The protocol a route must speak for this completion operation.
    pub(super) fn protocol(self) -> Option<Protocol> {
        match self {
            Self::Chat => Some(Protocol::OpenAiChat),
            Self::Responses => Some(Protocol::OpenAiResponses),
            Self::Messages => Some(Protocol::AnthropicMessages),
            Self::Models => None,
        }
    }
}
pub(super) fn build(
    client: &Client,
    route: &Route,
    operation: Operation,
    secret: Option<&SecretString>,
) -> Result<RequestBuilder, GatewayError> {
    let mut url = url::Url::parse(&route.base_url)
        .map_err(|_| GatewayError::Configuration("invalid relay route URL"))?;
    url.set_path(&format!(
        "{}/{}",
        url.path().trim_end_matches('/'),
        operation.path()
    ));
    let mut headers = HeaderMap::new();
    for (name, value) in &route.extra_headers {
        insert(&mut headers, name, value, false)?;
    }
    match &route.auth {
        Auth::None => (),
        Auth::Bearer => {
            let secret = required(secret)?;
            insert(
                &mut headers,
                "authorization",
                &format!("Bearer {secret}"),
                true,
            )?;
        }
        Auth::Header { name } => insert(&mut headers, name, required(secret)?, true)?,
        Auth::Query { name } => {
            url.query_pairs_mut().append_pair(name, required(secret)?);
        }
        _ => {
            return Err(GatewayError::Configuration(
                "unsupported relay authentication",
            ));
        }
    }
    headers.insert(
        reqwest::header::ACCEPT,
        HeaderValue::from_static("text/event-stream, application/json"),
    );
    // The protocol-required version is pinned here, for every call to a Messages endpoint (its model list too); the
    // runtime's own headers are never forwarded.
    if operation == Operation::Messages
        || (operation == Operation::Models && route.protocol == Protocol::AnthropicMessages)
    {
        headers.insert(
            HeaderName::from_static(tqa_llm::MESSAGES_VERSION_HEADER),
            HeaderValue::from_static(tqa_llm::MESSAGES_VERSION),
        );
    }
    Ok(client.request(operation.method(), url).headers(headers))
}
fn required(secret: Option<&SecretString>) -> Result<&str, GatewayError> {
    secret
        .map(ExposeSecret::expose_secret)
        .filter(|value| !value.is_empty())
        .ok_or(GatewayError::SecretUnavailable)
}
fn insert(
    headers: &mut HeaderMap,
    name: &str,
    value: &str,
    sensitive: bool,
) -> Result<(), GatewayError> {
    let name = HeaderName::from_bytes(name.as_bytes())
        .map_err(|_| GatewayError::Configuration("invalid relay header name"))?;
    let mut value = HeaderValue::from_str(value).map_err(|_| GatewayError::SecretUnavailable)?;
    value.set_sensitive(sensitive);
    headers.insert(name, value);
    Ok(())
}
