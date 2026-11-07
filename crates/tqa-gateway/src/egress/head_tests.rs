use super::{Head, Target, parse};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn gw_i_012_connect_targets_hosts_ports_and_bracketed_addresses() -> TestResult {
    let head = parse(b"CONNECT pypi.org:443 HTTP/1.1\r\nHost: pypi.org:443\r\nProxy-Authorization: Bearer token-1\r\n\r\n")?;
    assert_eq!(
        head.target,
        Target::Tunnel {
            host: "pypi.org".into(),
            port: 443
        }
    );
    assert_eq!(head.bearer.as_deref(), Some("token-1"));
    let head = parse(b"CONNECT [2001:db8::1]:8443 HTTP/1.1\r\n\r\n")?;
    assert_eq!(
        head.target,
        Target::Tunnel {
            host: "2001:db8::1".into(),
            port: 8443
        }
    );
    Ok(())
}

#[test]
fn gw_i_012_absolute_requests_are_forwarded_in_origin_form_without_proxy_headers() -> TestResult {
    let head = parse(
        b"GET http://example.org:8080/a/b?c=1 HTTP/1.1\r\nHost: example.org:8080\r\nProxy-Authorization: Bearer secret-token\r\nProxy-Connection: keep-alive\r\nConnection: keep-alive\r\nAccept: */*\r\n\r\n",
    )?;
    let Head {
        target:
            Target::Forward {
                host,
                port,
                head: forwarded,
            },
        bearer,
    } = head
    else {
        return Err("expected a forward".into());
    };
    assert_eq!(
        (host.as_str(), port, bearer.as_deref()),
        ("example.org", 8080, Some("secret-token"))
    );
    let text = String::from_utf8(forwarded)?;
    assert_eq!(
        text,
        "GET /a/b?c=1 HTTP/1.1\r\nHost: example.org:8080\r\nAccept: */*\r\nConnection: close\r\n\r\n"
    );
    let default_port = parse(b"POST http://example.org HTTP/1.1\r\nHost: example.org\r\n\r\n")?;
    assert!(matches!(
        default_port.target,
        Target::Forward { port: 80, .. }
    ));
    Ok(())
}

#[test]
fn gw_i_012_malformed_or_unsupported_heads_are_refused() {
    let oversized = format!(
        "CONNECT a.example:443 HTTP/1.1\r\n{}\r\n",
        "X: y\r\n".repeat(101)
    );
    for head in [
        b"".as_slice(),
        b"CONNECT pypi.org HTTP/1.1\r\n\r\n",
        b"CONNECT pypi.org:0 HTTP/1.1\r\n\r\n",
        b"CONNECT pypi.org:99999 HTTP/1.1\r\n\r\n",
        b"CONNECT user@pypi.org:443 HTTP/1.1\r\n\r\n",
        b"GET /relative HTTP/1.1\r\n\r\n",
        b"GET https://example.org/ HTTP/1.1\r\n\r\n",
        b"GET http://user:pass@example.org/ HTTP/1.1\r\n\r\n",
        b"GET http://example.org/ HTTP/2\r\n\r\n",
        b"GET http://example.org/ HTTP/1.1\r\nBad header\r\n\r\n",
        oversized.as_bytes(),
    ] {
        assert!(parse(head).is_err(), "{}", String::from_utf8_lossy(head));
    }
}
