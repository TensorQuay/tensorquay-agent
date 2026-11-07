//! LLM-E-008/016 and LLM-C-019: HTTP classification and credential-safe diagnostics.

#[path = "fixtures/http.rs"]
mod fixtures;

use secrecy::SecretString;
use std::{error::Error, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use tqa_llm::{Auth, HttpConfig, LlmError, StreamLimits, ThrottleScope, provider_for};
use tqa_testkit::{FakeClock, FakeModelServer, FakeSleeper, Script, Scripted};

#[tokio::test]
async fn llm_e_008_http_statuses_and_retry_after() -> Result<(), Box<dyn Error>> {
    let cases = [
        (429, Some("7"), Some(Duration::from_secs(7))),
        (
            429,
            Some("Thu, 01 Jan 1970 00:00:00 GMT"),
            Some(Duration::ZERO),
        ),
        (429, Some("not a date"), None),
        (500, None, None),
        (408, None, None),
        (502, None, None),
        (503, None, None),
        (504, None, None),
        // The overload status some servers send; retryable and an availability error (ADR-0025).
        (529, None, None),
        (400, None, None),
        (401, None, None),
        (403, None, None),
        (404, None, None),
    ];
    let scripts = cases
        .iter()
        .map(|(code, header, _)| {
            Script::new(vec![Scripted::Status {
                code: *code,
                body: "unavailable".into(),
                retry_after: header.map(str::to_owned),
            }])
        })
        .collect();
    let clock = FakeClock::new();
    let server = FakeModelServer::start(scripts, Arc::new(FakeSleeper::new(clock.clone()))).await?;
    let provider = fixtures::provider(fixtures::route(server.base_url())?, &clock)?;
    let mut request = fixtures::request();
    request.tools.push(fixtures::tool());
    for (status, _, expected) in cases {
        let error = provider
            .complete(request.clone(), CancellationToken::new())
            .await
            .err()
            .ok_or("accepted HTTP error")?;
        assert_eq!(
            error.retryable(),
            [408, 429, 500, 502, 503, 504, 529].contains(&status)
        );
        if status == 429 {
            assert!(
                matches!(error, LlmError::RateLimited { retry_after } if retry_after == expected)
            );
        } else {
            assert!(
                matches!(error, LlmError::Http { status: actual, ref body_excerpt } if actual == status && body_excerpt == "unavailable")
            );
        }
    }
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn llm_c_019_error_excerpt_masks_resolved_secret_and_ignores_empty_unused_secret()
-> Result<(), Box<dyn Error>> {
    let clock = FakeClock::new();
    let server = FakeModelServer::start(
        vec![
            Script::new(vec![Scripted::Status {
                code: 401,
                body: "refused demo-secret".into(),
                retry_after: None,
            }]);
            2
        ],
        Arc::new(FakeSleeper::new(clock.clone())),
    )
    .await?;
    for secret in ["demo-secret", ""] {
        let mut route = fixtures::route(server.base_url())?;
        if !secret.is_empty() {
            route.auth = Auth::Bearer;
            route.key_ref = Some("secrets/test".into());
        }
        let provider = provider_for(
            route,
            Some(SecretString::from(secret)),
            HttpConfig::new(StreamLimits::RECOMMENDED),
            Arc::new(clock.clone()),
            Arc::new(FakeSleeper::new(clock.clone())),
        )?;
        let error = provider
            .complete(fixtures::request(), CancellationToken::new())
            .await
            .err()
            .ok_or("accepted HTTP error")?;
        let expected = if secret.is_empty() {
            "refused demo-secret"
        } else {
            "refused [redacted]"
        };
        assert!(matches!(error, LlmError::Http { body_excerpt, .. } if body_excerpt == expected));
    }
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn llm_u_032_the_relays_own_throttle_is_told_apart_from_a_provider_429()
-> Result<(), Box<dyn Error>> {
    let cases = [
        (429, Some("3"), Some("user")),
        (429, None, Some("route")),
        (429, Some("3"), Some("unknown")),
        (429, Some("3"), None),
        (503, Some("3"), Some("route")),
    ];
    let scripts = cases
        .iter()
        .map(|(code, retry_after, scope)| {
            let mut events = vec![Scripted::Status {
                code: *code,
                body: "throttled".into(),
                retry_after: retry_after.map(str::to_owned),
            }];
            if let Some(scope) = scope {
                events.push(Scripted::Header(
                    tqa_llm::RELAY_THROTTLE_HEADER.into(),
                    (*scope).into(),
                ));
            }
            Script::new(events)
        })
        .collect();
    let clock = FakeClock::new();
    let server = FakeModelServer::start(scripts, Arc::new(FakeSleeper::new(clock.clone()))).await?;
    let provider = fixtures::provider(fixtures::route(server.base_url())?, &clock)?;
    let mut errors = Vec::new();
    for _ in cases {
        errors.push(
            provider
                .complete(fixtures::request(), CancellationToken::new())
                .await
                .err()
                .ok_or("accepted HTTP error")?,
        );
    }
    server.shutdown().await?;
    let three = Duration::from_secs(3);
    assert!(matches!(
        errors[0],
        LlmError::Throttled { retry_after, scope: ThrottleScope::User } if retry_after == three
    ));
    // The relay always asks for a wait; without one the runtime still waits a second rather than spin.
    assert!(matches!(
        errors[1],
        LlmError::Throttled { retry_after, scope: ThrottleScope::Route } if retry_after == Duration::from_secs(1)
    ));
    for throttle in &errors[..2] {
        assert!(throttle.retryable());
        assert_eq!(throttle.code(), "rate_limited");
    }
    // Only the relay's own 429 with a known scope is a throttle; anything else keeps its ordinary meaning.
    assert!(
        matches!(errors[2], LlmError::RateLimited { retry_after: Some(delay) } if delay == three)
    );
    assert!(
        matches!(errors[3], LlmError::RateLimited { retry_after: Some(delay) } if delay == three)
    );
    assert!(
        matches!(errors[4], LlmError::Unavailable { retry_after: Some(delay) } if delay == three)
    );
    Ok(())
}

#[test]
fn llm_u_032_throttle_scopes_round_trip_and_near_misses_are_not_throttles() {
    for scope in [ThrottleScope::User, ThrottleScope::Route] {
        assert_eq!(ThrottleScope::parse(scope.as_str()), Some(scope));
    }
    assert_eq!(ThrottleScope::User.as_str(), "user");
    assert_eq!(ThrottleScope::Route.as_str(), "route");
    for value in [
        "",
        "User",
        "ROUTE",
        " user",
        "route ",
        "users",
        "user,route",
    ] {
        assert_eq!(ThrottleScope::parse(value), None, "{value:?}");
    }
}
