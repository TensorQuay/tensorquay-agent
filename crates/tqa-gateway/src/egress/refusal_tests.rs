//! GW-U-081: the proxy's refusal for each reason names the host and port in the reason phrase, a `Proxy-Status`
//! header and a one-line body, and never echoes what isn't a canonical host (ADR-0035 §3).
use super::{DISABLED, NOT_LISTED, PRIVATE, denied};

/// The status line, the headers by name, and the body.
fn parts(response: &str) -> (String, Vec<(String, String)>, String) {
    let (head, body) = response.split_once("\r\n\r\n").unwrap_or((response, ""));
    let mut lines = head.split("\r\n");
    let status = lines.next().unwrap_or_default().to_owned();
    let headers = lines
        .filter_map(|line| line.split_once(": "))
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect();
    (status, headers, body.to_owned())
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

#[test]
fn gw_u_081_each_reason_names_the_host_and_port() {
    for (reason, status, error, sentence) in [
        (
            NOT_LISTED,
            "HTTP/1.1 403 Blocked by the network allow-list: api.example.com:443",
            "http_request_denied",
            "Blocked by the network allow-list: api.example.com:443 is not allowed. Ask an administrator to allow it, \
             or use an allowed host.\n",
        ),
        (
            PRIVATE,
            "HTTP/1.1 403 Blocked: api.example.com:443 resolves to a private or reserved address",
            "destination_ip_prohibited",
            "Blocked: api.example.com:443 resolves to a private or reserved address. Only an exact address an \
             administrator allows can be reached.\n",
        ),
        (
            DISABLED,
            "HTTP/1.1 403 Blocked: network access is turned off",
            "http_request_denied",
            "Blocked: network access outside the sandbox is turned off here, so api.example.com:443 can't be \
             reached.\n",
        ),
    ] {
        let (line, headers, body) = parts(&denied("API.Example.com.", 443, reason));
        assert_eq!(line, status, "{reason}");
        let phrase = status.trim_start_matches("HTTP/1.1 403 ");
        assert_eq!(
            header(&headers, "Proxy-Status"),
            Some(format!("tqa-gateway; error={error}; details=\"{phrase}\"").as_str())
        );
        assert_eq!(
            header(&headers, "Content-Type"),
            Some("text/plain; charset=utf-8")
        );
        assert_eq!(
            header(&headers, "Content-Length"),
            Some(body.len().to_string().as_str())
        );
        assert_eq!(header(&headers, "Connection"), Some("close"));
        assert_eq!(body, sentence);
        assert_eq!(body.lines().count(), 1, "one line");
    }
}

#[test]
fn gw_u_081_addresses_are_shown_as_addresses() {
    let (line, _, _) = parts(&denied("10.0.0.5", 8080, PRIVATE));
    assert_eq!(
        line,
        "HTTP/1.1 403 Blocked: 10.0.0.5:8080 resolves to a private or reserved address"
    );
    let (line, _, _) = parts(&denied("fd00::1", 443, NOT_LISTED));
    assert_eq!(
        line,
        "HTTP/1.1 403 Blocked by the network allow-list: [fd00::1]:443"
    );
}

#[test]
fn gw_u_081_a_host_that_isnt_canonical_is_never_echoed() {
    for host in [
        "bad\"host",
        "evil.example\r\nX-Injected: 1",
        "a;b",
        "*.example.com",
        "ex ample",
        "é.example",
    ] {
        let response = denied(host, 443, NOT_LISTED);
        let (line, headers, body) = parts(&response);
        assert_eq!(
            line, "HTTP/1.1 403 Blocked by the network allow-list: this host:443",
            "{host:?}"
        );
        assert!(header(&headers, "X-Injected").is_none());
        assert!(!body.contains(host), "{host:?}");
        assert_eq!(response.matches("\r\n\r\n").count(), 1, "{host:?}");
    }
}
