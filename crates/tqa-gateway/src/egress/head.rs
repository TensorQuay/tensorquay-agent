//! Bounded parsing of a proxy request head: `CONNECT host:port` or an absolute-form `http://` request.
use crate::GatewayError;

/// The largest accepted request head, including the blank line.
pub(super) const MAX_HEAD_BYTES: usize = 8 * 1024;
const MAX_HEADERS: usize = 100;
// Hop-by-hop and proxy headers never reach the destination.
const DROPPED: [&str; 5] = [
    "proxy-authorization",
    "proxy-connection",
    "connection",
    "keep-alive",
    "upgrade",
];

/// Where the request goes.
#[derive(Debug, Eq, PartialEq)]
pub(super) enum Target {
    /// A tunnel to `host:port` (usually TLS the proxy cannot read).
    Tunnel { host: String, port: u16 },
    /// A plain HTTP request, already rewritten to origin form with `Connection: close`.
    Forward {
        host: String,
        port: u16,
        head: Vec<u8>,
    },
}

/// A parsed request head.
#[derive(Debug)]
pub(super) struct Head {
    pub target: Target,
    /// The runtime's own `Proxy-Authorization: Bearer` token, when present.
    pub bearer: Option<String>,
}

/// Parses one head that ends with an empty line.
///
/// # Errors
/// `BadRequest` for anything malformed, unbounded or unsupported (HTTPS in absolute form, userinfo, HTTP/2).
pub(super) fn parse(bytes: &[u8]) -> Result<Head, GatewayError> {
    let text = std::str::from_utf8(bytes).map_err(|_| bad("proxy request head must be UTF-8"))?;
    let text = text
        .strip_suffix("\r\n\r\n")
        .ok_or_else(|| bad("proxy request head is incomplete"))?;
    let mut lines = text.split("\r\n");
    let request = lines.next().unwrap_or_default();
    let mut parts = request.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(bad("malformed proxy request line"));
    };
    if !matches!(version, "HTTP/1.1" | "HTTP/1.0") {
        return Err(bad("only HTTP/1.x proxy requests are supported"));
    }
    let mut kept = Vec::new();
    let mut bearer = None;
    for line in lines {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| bad("malformed proxy request header"))?;
        let lower = name.trim().to_ascii_lowercase();
        if lower == "proxy-authorization" {
            bearer = value.trim().strip_prefix("Bearer ").map(str::to_owned);
        }
        if !DROPPED.contains(&lower.as_str()) {
            kept.push(line);
        }
        if kept.len() > MAX_HEADERS {
            return Err(bad("too many proxy request headers"));
        }
    }
    let target = if method == "CONNECT" {
        let (host, port) = authority(target)?;
        Target::Tunnel { host, port }
    } else {
        let rest = target
            .strip_prefix("http://")
            .ok_or_else(|| bad("proxy requests must use CONNECT or an absolute http:// target"))?;
        let (authority_text, path) = rest
            .find(['/', '?'])
            .map_or((rest, "/"), |at| rest.split_at(at));
        let (host, port) = if authority_text.contains(':') && !authority_text.ends_with(']') {
            authority(authority_text)?
        } else {
            (host_part(authority_text)?, 80)
        };
        let path = if path.starts_with('?') {
            format!("/{path}")
        } else {
            path.to_owned()
        };
        let mut head = format!("{method} {path} {version}\r\n");
        for line in &kept {
            head.push_str(line);
            head.push_str("\r\n");
        }
        head.push_str("Connection: close\r\n\r\n");
        Target::Forward {
            host,
            port,
            head: head.into_bytes(),
        }
    };
    Ok(Head { target, bearer })
}

fn authority(text: &str) -> Result<(String, u16), GatewayError> {
    let (host, port) = text
        .rsplit_once(':')
        .ok_or_else(|| bad("a proxy target needs a port"))?;
    let port = port
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(|| bad("a proxy target port must be 1 through 65535"))?;
    Ok((host_part(host)?, port))
}

fn host_part(host: &str) -> Result<String, GatewayError> {
    let host = host
        .strip_prefix('[')
        .and_then(|inner| inner.strip_suffix(']'))
        .unwrap_or(host);
    if host.is_empty() || host.len() > 253 || host.contains(['@', '/', ' ', '[', ']']) {
        return Err(bad("invalid proxy target host"));
    }
    Ok(host.to_owned())
}

const fn bad(message: &'static str) -> GatewayError {
    GatewayError::BadRequest(message)
}

#[cfg(test)]
#[path = "head_tests.rs"]
mod tests;
