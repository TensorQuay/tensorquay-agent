use axum::http::{HeaderMap, HeaderValue, header};

const HOP_HEADERS: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "proxy-connection",
];

fn remove_hop(headers: &mut HeaderMap) {
    let nominated: Vec<_> = headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| {
            value
                .split(',')
                .map(|name| name.trim().to_ascii_lowercase())
        })
        .collect();
    for name in nominated {
        headers.remove(&name);
    }
    for name in HOP_HEADERS {
        headers.remove(*name);
    }
}

pub(super) fn request(mut headers: HeaderMap, origin: &url::Url) -> HeaderMap {
    remove_hop(&mut headers);
    // Page requests are forwarded without content; a client's length would describe bytes never sent.
    headers.remove(header::CONTENT_LENGTH);
    let sensitive: Vec<_> = headers
        .keys()
        .filter(|name| {
            matches!(name.as_str(), "cookie" | "authorization" | "host")
                || name.as_str().starts_with("proxy-")
                || name.as_str().starts_with("x-forwarded-")
        })
        .cloned()
        .collect();
    for name in sensitive {
        headers.remove(name);
    }
    if let Ok(value) = HeaderValue::from_str(origin.scheme()) {
        headers.insert("x-forwarded-proto", value);
    }
    if let Ok(value) =
        HeaderValue::from_str(&origin[url::Position::BeforeHost..url::Position::AfterPort])
    {
        headers.insert("x-forwarded-host", value);
    }
    headers
}

pub(super) fn response(mut headers: HeaderMap) -> HeaderMap {
    remove_hop(&mut headers);
    headers.remove(header::SET_COOKIE);
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers
}
