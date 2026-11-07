//! Credential-free, streaming page delivery behind the gateway's public origin.

mod headers;
mod socket;

use crate::{GatewayError, app::State};
use axum::{
    body::Body,
    extract::{Request, State as AppState},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use futures::StreamExt;
use std::{sync::Arc, time::Duration};

pub(crate) const HEAD_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
pub(super) const HMR_PATH: &str = "/_next/hmr";

pub(crate) struct Web {
    client: reqwest::Client,
    sockets: Arc<tokio::sync::Semaphore>,
}

impl Web {
    pub fn new(config: Option<&crate::config::WebConfig>) -> Result<Self, GatewayError> {
        if config.is_some_and(|web| web.dev) {
            tracing::warn!("web development hot reload is enabled");
        }
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .map_err(|_| GatewayError::Configuration("web transport is unavailable"))?;
        Ok(Self {
            client,
            sockets: Arc::new(tokio::sync::Semaphore::new(32)),
        })
    }
}

pub(crate) fn is_page(path: &str) -> bool {
    // Repeated leading slashes name the same namespace (a base URL ending in "/" makes "//mcp/..."): never a page.
    let path = format!("/{}", path.trim_start_matches('/'));
    ![
        "/api", "/healthz", "/readyz", "/metrics", "/runtime", "/relay", "/mcp",
    ]
    .iter()
    .any(|prefix| {
        path == *prefix
            || path
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

pub(crate) async fn page(AppState(state): AppState<Arc<State>>, request: Request) -> Response {
    if !is_page(request.uri().path()) {
        return GatewayError::NotFound.into_response();
    }
    if !matches!(*request.method(), Method::GET | Method::HEAD) {
        return (StatusCode::METHOD_NOT_ALLOWED, [("allow", "GET, HEAD")]).into_response();
    }
    let Some(config) = &state.config.web else {
        return GatewayError::NotFound.into_response();
    };
    if config.dev && request.uri().path() == HMR_PATH && request.headers().contains_key("upgrade") {
        return socket::upgrade(state, request).await;
    }
    let mut url = config.upstream.clone();
    url.set_path(request.uri().path());
    url.set_query(request.uri().query());
    let (parts, body) = request.into_parts();
    let limit = state.config.limits.body_bytes;
    let discarded = tokio::select! {
        () = state.cancel.cancelled() => return GatewayError::Cancelled.into_response(),
        result = tokio::time::timeout(HEAD_TIMEOUT, discard(body, limit)) => result,
    };
    match discarded {
        Ok(Ok(())) => {}
        Ok(Err(error)) => return error.into_response(),
        Err(_) => return GatewayError::BadRequest("request body timed out").into_response(),
    }
    // GET and HEAD content has no defined semantics, so page requests are forwarded without one.
    let upstream = state
        .web
        .client
        .request(parts.method, url)
        .headers(headers::request(parts.headers, &state.config.public_origin));
    let response = tokio::select! {
        () = state.cancel.cancelled() => return GatewayError::Cancelled.into_response(),
        result = tokio::time::timeout(HEAD_TIMEOUT, upstream.send()) => match result {
            Ok(Ok(response)) => response,
            _ => return unavailable(),
        }
    };
    let status = response.status();
    let headers = headers::response(response.headers().clone());
    let stream = futures::stream::unfold(
        (response.bytes_stream(), state.cancel.clone()),
        |(mut stream, cancel)| async move {
            let next = tokio::select! {
                () = cancel.cancelled() => None,
                value = tokio::time::timeout(HEAD_TIMEOUT, stream.next()) => match value {
                    Ok(value) => value.map(|value| value.map_err(|_| std::io::Error::other("web stream failed"))),
                    Err(_) => Some(Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "web stream timed out"))),
                }
            };
            next.map(|chunk| (chunk, (stream, cancel)))
        },
    );
    let mut response = (status, Body::from_stream(stream)).into_response();
    *response.headers_mut() = headers;
    response
}

/// Reads a page request body to its end without keeping it, so the limit still answers `413` and the client
/// connection stays in a defined state.
async fn discard(body: Body, limit: usize) -> Result<(), GatewayError> {
    use http_body_util::BodyExt;
    let mut limited = http_body_util::Limited::new(body, limit);
    while let Some(frame) = limited.frame().await {
        match frame {
            Ok(_) => {}
            Err(error) if error.is::<http_body_util::LengthLimitError>() => {
                return Err(GatewayError::BodyTooLarge);
            }
            Err(_) => return Err(GatewayError::BadRequest("request body could not be read")),
        }
    }
    Ok(())
}

pub(super) fn unavailable() -> Response {
    let mut response = (
        StatusCode::BAD_GATEWAY,
        "The web interface is not available yet.\n",
    )
        .into_response();
    *response.headers_mut() = headers::response(response.headers().clone());
    response
}

#[cfg(test)]
mod tests {
    use super::{Body, GatewayError, discard};

    fn chunks(sizes: &[usize]) -> Body {
        let frames: Vec<Result<Vec<u8>, std::io::Error>> =
            sizes.iter().map(|size| Ok(vec![b' '; *size])).collect();
        Body::from_stream(futures::stream::iter(frames))
    }

    #[tokio::test]
    async fn gw_i_041_a_page_body_is_discarded_up_to_the_limit_and_refused_beyond_it() {
        assert!(discard(chunks(&[512, 512]), 1024).await.is_ok());
        assert!(matches!(
            discard(chunks(&[512, 512, 1]), 1024).await,
            Err(GatewayError::BodyTooLarge)
        ));
        assert!(discard(Body::empty(), 1024).await.is_ok());
    }
}
