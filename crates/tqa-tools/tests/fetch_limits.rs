//! TOOL-U-010: malformed content, redirect limits, cancellation and explicit proxy inputs.
#[path = "support/context.rs"]
mod context;
#[path = "support/proxy.rs"]
mod proxy;
use proxy::{Proxy, Reply, TOKEN, TestResult, body, page};
use serde_json::json;
use std::{sync::Arc, time::Duration};
use tqa_testkit::{FakeClock, FakeSleeper, workspace_fixture};
use tqa_tools::{
    ProxyEndpoint, Tool, ToolError,
    builtin::{FetchUrl, Shell},
};

#[test]
fn tool_u_010_proxy_origins_and_runtime_credentials_are_bounded() -> TestResult {
    for value in [
        "http://gateway:3128",
        "https://gateway:8443/",
        "http://[::1]:3128",
    ] {
        ProxyEndpoint::new(value)?;
    }
    for value in [
        "",
        "socks5://gateway",
        "http://gateway:0",
        "http://user:fixture@gateway",
        "http://@gateway",
        "http://gateway/path",
        "http://gateway/../",
        "http:gateway/path",
        "http:gateway/http://path",
        "http:gateway/../",
        "http:@gateway",
        "http:///@gateway",
        "http://gateway?query",
        "http://gateway#fragment",
        " http://gateway",
        "http://gateway\\",
        "http://gateway\n",
    ] {
        assert!(ProxyEndpoint::new(value).is_err(), "{value}");
    }
    assert!(ProxyEndpoint::new(&format!("http://{}", "x".repeat(2048))).is_err());
    FetchUrl::new(
        ProxyEndpoint::new("http://gateway:3128")?,
        &"x".repeat(4096).into(),
    )?;
    for token in [String::new(), "x".repeat(4097), "bad\nheader".into()] {
        assert!(FetchUrl::new(ProxyEndpoint::new("http://gateway:3128")?, &token.into()).is_err());
    }
    Ok(())
}

#[tokio::test]
async fn tool_u_010_redirect_count_and_invalid_redirect_schemes_are_bounded() -> TestResult {
    for (location, expected) in [
        ("http://allowed.invalid/again", "io"),
        ("file:///etc/passwd", "invalid_args"),
    ] {
        let reply = Reply::Page(format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").into_bytes());
        let proxy = Proxy::start(vec![reply; 7]).await?;
        let fixture = workspace_fixture()?;
        let tool = FetchUrl::new(ProxyEndpoint::new(&proxy.url())?, &TOKEN.into())?;
        let result = tool
            .call(
                json!({"url":"http://allowed.invalid/"}),
                &context::context(&fixture),
            )
            .await;
        let seen = proxy.seen.clone();
        proxy.finish().await?;
        assert_eq!(
            result.err().ok_or("redirect loop succeeded")?.code(),
            expected
        );
        assert!(seen.lock().map_err(|_| "fixture lock")?.connect.len() <= 6);
    }
    Ok(())
}

#[tokio::test]
async fn tool_u_010_compressed_binary_and_oversized_headers_are_refused() -> TestResult {
    let compressed = Reply::Page(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Encoding: gzip\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc".to_vec());
    let duplicate_encoding = Reply::Page(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Encoding: identity\r\nContent-Encoding: gzip\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc".to_vec());
    let large = Reply::Page(
        format!(
            "HTTP/1.1 200 OK\r\nX-Filler: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "x".repeat(16 * 1024)
        )
        .into_bytes(),
    );
    for reply in [
        compressed,
        duplicate_encoding,
        page("application/octet-stream", [0, 1, 2]),
        page("text/plain", [0xff, 0xfe]),
        large,
    ] {
        let proxy = Proxy::start(vec![reply]).await?;
        let fixture = workspace_fixture()?;
        let tool = FetchUrl::new(ProxyEndpoint::new(&proxy.url())?, &TOKEN.into())?;
        let result = tool
            .call(
                json!({"url":"http://allowed.invalid/"}),
                &context::context(&fixture),
            )
            .await;
        proxy.finish().await?;
        assert!(result.is_err());
    }
    Ok(())
}

#[tokio::test]
async fn tool_u_010_url_bytes_and_canonical_expansion_are_bounded_before_connecting() -> TestResult
{
    let prefix = "http://allowed.invalid/";
    for (url, permitted) in [
        (format!("{prefix}{}", "x".repeat(4096 - prefix.len())), true),
        (
            format!("{prefix}{}", "x".repeat(4097 - prefix.len())),
            false,
        ),
        (format!("{prefix}{}", "ü".repeat(700)), false),
    ] {
        let proxy = Proxy::start(vec![page("text/plain", "bounded URL")]).await?;
        let fixture = workspace_fixture()?;
        let tool = FetchUrl::new(ProxyEndpoint::new(&proxy.url())?, &TOKEN.into())?;
        let result = tool
            .call(json!({"url":url}), &context::context(&fixture))
            .await;
        let seen = proxy.seen.clone();
        proxy.finish().await?;
        if permitted {
            assert_eq!(body(&result?.text), "bounded URL");
        } else {
            assert_eq!(
                result.err().ok_or("oversized URL accepted")?.code(),
                "invalid_args"
            );
            assert!(seen.lock().map_err(|_| "fixture lock")?.connect.is_empty());
        }
    }
    Ok(())
}

#[tokio::test]
async fn tool_u_010_exact_response_header_boundary_succeeds() -> TestResult {
    let head = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nX-Padding: ";
    let tail = "\r\nConnection: close\r\n\r\n";
    let reply = format!(
        "{head}{}{tail}",
        "x".repeat(16 * 1024 - head.len() - tail.len())
    );
    assert_eq!(reply.len(), 16 * 1024);
    let proxy = Proxy::start(vec![Reply::Page(reply.into_bytes())]).await?;
    let fixture = workspace_fixture()?;
    let tool = FetchUrl::new(ProxyEndpoint::new(&proxy.url())?, &TOKEN.into())?;
    let result = tool
        .call(
            json!({"url":"http://allowed.invalid/"}),
            &context::context(&fixture),
        )
        .await;
    proxy.finish().await?;
    assert_eq!(body(&result?.text), "");
    Ok(())
}

#[tokio::test]
async fn tool_u_010_html_complexity_cannot_expand_unbounded_rendering() -> TestResult {
    for body in [
        format!("{}text{}", "<div>".repeat(129), "</div>".repeat(129)),
        "<br>".repeat(65_536),
        "<table><tr><td colspan=18446744073709551615>text</td></tr></table>".into(),
    ] {
        let proxy = Proxy::start(vec![page("text/html", body)]).await?;
        let fixture = workspace_fixture()?;
        let tool = FetchUrl::new(ProxyEndpoint::new(&proxy.url())?, &TOKEN.into())?;
        let result = tool
            .call(
                json!({"url":"http://allowed.invalid/"}),
                &context::context(&fixture),
            )
            .await;
        proxy.finish().await?;
        assert_eq!(
            result.err().ok_or("complex document accepted")?.code(),
            "unsupported"
        );
    }
    Ok(())
}

#[tokio::test]
async fn tool_u_010_html_at_the_complexity_boundaries_still_renders() -> TestResult {
    for body in [
        format!("{}text{}", "<div>".repeat(125), "</div>".repeat(125)),
        "<!--x-->".repeat(65_532),
        "<table><tr><td colspan=1000 rowspan=1000>text</td></tr></table>".into(),
    ] {
        let proxy = Proxy::start(vec![page("text/html", body)]).await?;
        let fixture = workspace_fixture()?;
        let tool = FetchUrl::new(ProxyEndpoint::new(&proxy.url())?, &TOKEN.into())?;
        let result = tool
            .call(
                json!({"url":"http://allowed.invalid/"}),
                &context::context(&fixture),
            )
            .await;
        proxy.finish().await?;
        result?;
    }
    Ok(())
}

#[tokio::test]
async fn tool_u_010_shell_receives_only_explicit_proxy_values_and_no_bearer() -> TestResult {
    let fixture = workspace_fixture()?;
    let shell = Shell::new().with_proxy(ProxyEndpoint::new("http://gateway:3128")?);
    let result = shell.call(json!({"command":"printf '%s\\n' \"$HTTP_PROXY\" \"$HTTPS_PROXY\" \"$http_proxy\" \"$https_proxy\" \"$NO_PROXY\" \"${TQA_RUNTIME_TOKEN_FILE-unset}\" \"${NODE_USE_ENV_PROXY-unset}\""}), &context::context(&fixture)).await?;
    // Node's fetch and https read the proxy from the environment only when told to (G9c, SBX-C-019).
    assert_eq!(
        result.text,
        "http://gateway:3128/\nhttp://gateway:3128/\nhttp://gateway:3128/\nhttp://gateway:3128/\ngateway,localhost,127.0.0.1,::1\nunset\n1\n"
    );
    Ok(())
}

#[tokio::test]
async fn tool_u_010_stalled_proxy_observes_the_total_deadline_and_cancellation() -> TestResult {
    stalled(false).await?;
    stalled(true).await
}

async fn stalled(timeout: bool) -> TestResult {
    let proxy = Proxy::start(vec![Reply::Stall]).await?;
    let fixture = workspace_fixture()?;
    let mut cx = context::context(&fixture);
    let clock = FakeClock::new();
    let sleeper = Arc::new(FakeSleeper::new(clock.clone()));
    cx.clock = Arc::new(clock.clone());
    cx.sleeper = sleeper.clone();
    let tool = FetchUrl::new(ProxyEndpoint::new(&proxy.url())?, &TOKEN.into())?;
    let request = tool.call(json!({"url":"http://allowed.invalid/"}), &cx);
    let control = async {
        tokio::time::timeout(Duration::from_secs(5), async {
            while proxy
                .seen
                .lock()
                .map_err(|_| "fixture lock")?
                .connect
                .is_empty()
                || !sleeper
                    .pending_durations()
                    .contains(&Duration::from_secs(30))
            {
                tokio::task::yield_now().await;
            }
            Ok::<_, Box<dyn std::error::Error>>(())
        })
        .await??;
        if timeout {
            clock.advance(Duration::from_secs(30))?;
        } else {
            cx.cancel.cancel();
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let (result, control) = tokio::join!(request, control);
    proxy.finish().await?;
    control?;
    assert_eq!(
        result,
        Err(if timeout {
            ToolError::Timeout
        } else {
            ToolError::Cancelled
        })
    );
    Ok(())
}

// TEST: TOOL-U-060
#[tokio::test]
async fn tool_u_060_a_shell_reaches_its_own_loopback_server_past_the_proxy() -> TestResult {
    // Real-world use before 17.0 (17 Oct 2026): a team's page served on 127.0.0.1 inside the sandbox; curl and Python
    // sent it to the egress proxy (NO_PROXY named only the gateway) and got
    // "Blocked network access to 127.0.0.1:46765". A task must reach its own local servers: loopback never goes through
    // the proxy, everything else still does.
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    // Never joined: a request the proxy swallowed leaves it waiting, and the test's own process ends it.
    let _server = std::thread::spawn(move || answer_local(&listener));
    let fixture = workspace_fixture()?;
    // A proxy whose name never resolves: a request sent through it fails at once.
    let shell = Shell::new().with_proxy(ProxyEndpoint::new("http://proxy.invalid:9")?);
    let result = shell
        .call(
            json!({"command": format!(
                "printf '%s|%s\\n' \"$NO_PROXY\" \"$no_proxy\"; \
                 curl -sS --max-time 5 http://127.0.0.1:{port}/; echo; \
                 python3 -c \"import urllib.request; print(urllib.request.urlopen('http://localhost:{port}/', timeout=5).read().decode())\""
            )}),
            &context::context(&fixture),
        )
        .await?;
    assert_eq!(
        result.text,
        "proxy.invalid,localhost,127.0.0.1,::1|proxy.invalid,localhost,127.0.0.1,::1\nlocal\nlocal\n",
        "loopback is listed after the proxy's host, and both clients reach the local server"
    );
    Ok(())
}

/// Answers "local" to the first two requests on `listener`, then stops.
fn answer_local(listener: &std::net::TcpListener) {
    use std::io::{Read, Write};
    for mut stream in listener.incoming().take(2).flatten() {
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let _ = stream.write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 5\r\n\r\nlocal");
    }
}
