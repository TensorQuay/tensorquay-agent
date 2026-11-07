//! TK-U-011: scripted response headers on a status response or a stream (over its content type); an invalid one is
//! a fixture error, never sent.

use std::{error::Error, sync::Arc};
use tqa_testkit::{FakeClock, FakeModelServer, FakeSleeper, Script, Scripted};

fn throttled(header: (&str, &str)) -> Script {
    Script::new(vec![
        Scripted::Status {
            code: 429,
            body: "throttled".into(),
            retry_after: Some("3".into()),
        },
        Scripted::Header(header.0.into(), header.1.into()),
    ])
}

#[tokio::test]
async fn tk_u_011_scripted_headers_are_sent_and_invalid_ones_fail_the_fixture()
-> Result<(), Box<dyn Error>> {
    let scripts = vec![
        throttled(("tqa-relay-throttle", "user")),
        throttled(("bad header", "user")),
        throttled(("tqa-relay-throttle", "line\nbreak")),
    ];
    let server =
        FakeModelServer::start(scripts, Arc::new(FakeSleeper::new(FakeClock::new()))).await?;
    let client = reqwest::Client::new();
    let url = format!("{}/chat/completions", server.base_url());
    let mut responses = Vec::new();
    for _ in 0..3 {
        let response = client
            .post(&url)
            .bearer_auth("fixture")
            .json(&serde_json::json!({"model": "fixture", "messages": [], "stream": true}))
            .send()
            .await?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        responses.push((status, headers, response.text().await?));
    }
    server.shutdown().await?;
    let (status, headers, body) = &responses[0];
    assert_eq!(*status, 429);
    assert_eq!(headers["retry-after"], "3");
    assert_eq!(headers["tqa-relay-throttle"], "user");
    assert_eq!(body, "throttled");
    // An invalid name or value is reported as a broken fixture rather than sent or silently dropped.
    for (status, headers, body) in &responses[1..] {
        assert_eq!(*status, 500);
        assert!(headers.get("tqa-relay-throttle").is_none());
        assert_eq!(body, "invalid scripted header");
    }
    Ok(())
}

#[tokio::test]
async fn tk_u_011_a_scripted_header_relabels_a_stream() -> Result<(), Box<dyn Error>> {
    let script = Script::new(vec![
        Scripted::Text("hi".into()),
        Scripted::Header("content-type".into(), "text/plain; charset=utf-8".into()),
    ]);
    let server =
        FakeModelServer::start(vec![script], Arc::new(FakeSleeper::new(FakeClock::new()))).await?;
    let response = reqwest::Client::new()
        .post(format!("{}/chat/completions", server.base_url()))
        .bearer_auth("fixture")
        .json(&serde_json::json!({"model": "fixture", "messages": [], "stream": true}))
        .send()
        .await?;
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.headers()["content-type"],
        "text/plain; charset=utf-8"
    );
    let body = response.text().await?;
    server.shutdown().await?;
    assert!(
        body.starts_with("data: ") && body.ends_with("data: [DONE]\n\n"),
        "{body}"
    );
    Ok(())
}
