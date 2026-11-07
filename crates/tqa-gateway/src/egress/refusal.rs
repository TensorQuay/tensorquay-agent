//! What a refused connection says (ADR-0035 §3): the status line's reason phrase and a one-line `text/plain` body
//! name the host, port and reason, and `Proxy-Status` (RFC 9209) carries the error type, so the agent can tell a
//! blocked host from a broken server whatever its client prints.

/// The destination isn't on the allow-list.
pub(crate) const NOT_LISTED: &str = "not in the allow-list";
/// The destination resolves to an address the guard refuses.
pub(crate) const PRIVATE: &str = "resolves to a private or reserved address";
/// `[egress] disabled`.
pub(crate) const DISABLED: &str = "egress is disabled";
/// An internal rule's name resolves to the gateway's own ground: its engine's networks, the sandbox pool, its own or the
/// host's addresses, or `internal_never` (ADR-0035 amendment).
pub(crate) const INTERNAL_FORBIDDEN: &str =
    "an internal server resolves to an address that is never reached";
/// Internal rules are off until `[egress] internal_never` names the host's addresses (the gateway can't see them).
pub(crate) const INTERNAL_UNCONFIGURED: &str = "internal servers need internal_never";
/// The engine's networks are unknown or stale, so an internal rule can't be checked.
pub(crate) const INTERNAL_UNVERIFIED: &str = "internal servers can't be checked now";

/// The whole HTTP/1.1 response for a denied destination. The host is echoed only in the allow-list's canonical form
/// (a name or an address), so nothing a client sent can shape a header.
pub(crate) fn denied(host: &str, port: u16, reason: &str) -> String {
    let target = match (
        super::canonical_host(host),
        host.parse::<std::net::Ipv6Addr>(),
    ) {
        (_, Ok(address)) => format!("[{address}]:{port}"),
        (Ok(name), Err(_)) if !name.starts_with("*.") => format!("{name}:{port}"),
        _ => format!("this host:{port}"),
    };
    let (phrase, sentence, error) = match reason {
        PRIVATE => (
            format!("Blocked: {target} resolves to a private or reserved address"),
            format!(
                "Blocked: {target} resolves to a private or reserved address. Only an exact address an administrator \
                 allows can be reached."
            ),
            "destination_ip_prohibited",
        ),
        INTERNAL_FORBIDDEN => (
            format!("Blocked: {target} resolves to an address internal servers never use here"),
            format!(
                "Blocked: {target} resolves to an address internal servers never use here (the gateway, its sandboxes, \
                 the container engine or this machine)."
            ),
            "destination_ip_prohibited",
        ),
        INTERNAL_UNCONFIGURED => (
            format!("Blocked: {target} is an internal server, and internal servers are off here"),
            format!(
                "Blocked: {target} is an internal server, and internal servers are off until an administrator names \
                 this machine's own addresses ([egress] internal_never)."
            ),
            "http_request_denied",
        ),
        INTERNAL_UNVERIFIED => (
            format!("Blocked: {target} is an internal server that can't be checked right now"),
            format!(
                "Blocked: {target} is an internal server, and the gateway can't check its addresses right now. Try \
                 again in a minute."
            ),
            "http_request_denied",
        ),
        DISABLED => (
            "Blocked: network access is turned off".to_owned(),
            format!(
                "Blocked: network access outside the sandbox is turned off here, so {target} can't be reached."
            ),
            "http_request_denied",
        ),
        _ => (
            format!("Blocked by the network allow-list: {target}"),
            format!(
                "Blocked by the network allow-list: {target} is not allowed. Ask an administrator to allow it, or use \
                 an allowed host."
            ),
            "http_request_denied",
        ),
    };
    let body = format!("{sentence}\n");
    format!(
        "HTTP/1.1 403 {phrase}\r\nProxy-Status: tqa-gateway; error={error}; details=\"{phrase}\"\r\n\
         Content-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

#[cfg(test)]
#[path = "refusal_tests.rs"]
mod tests;
