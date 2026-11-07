//! LLM-U-047 (GW-I-148's runtime half): a relay answer carrying `X-TQA-Gateway-Error` is the gateway's own refusal,
//! told apart from the provider's: its code and status, never the body's words. It comes before the ordinary 429 and
//! 503 readings, but the relay's own throttle keeps its meaning. A header that isn't a bare code counts as absent, and
//! a body's `error.code` alone never makes a refusal the gateway's.

#[allow(
    dead_code,
    reason = "This suite uses only some of the shared provider fixtures."
)]
#[path = "fixtures/http.rs"]
mod fixtures;

use std::{error::Error, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use tqa_llm::{LlmError, ThrottleScope};
use tqa_testkit::{FakeClock, FakeModelServer, FakeSleeper, Script, Scripted};
use tqa_types::{GATEWAY_ERROR_HEADER, gateway_error_code};

/// One scripted relay answer: status, Retry-After, the gateway's header, the relay's throttle header.
struct Case {
    status: u16,
    retry_after: Option<&'static str>,
    gateway: Option<&'static str>,
    throttle: Option<&'static str>,
}

const BODY: &str = r#"{"error":{"code":"key_use_unrecorded","message":"The key's use could not be recorded; try again."}}"#;

async fn answers(cases: &[Case]) -> Result<Vec<LlmError>, Box<dyn Error>> {
    let scripts = cases
        .iter()
        .map(|case| {
            let mut events = vec![Scripted::Status {
                code: case.status,
                body: BODY.into(),
                retry_after: case.retry_after.map(str::to_owned),
            }];
            if let Some(code) = case.gateway {
                events.push(Scripted::Header(GATEWAY_ERROR_HEADER.into(), code.into()));
            }
            if let Some(scope) = case.throttle {
                events.push(Scripted::Header(
                    tqa_llm::RELAY_THROTTLE_HEADER.into(),
                    scope.into(),
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
    Ok(errors)
}

// TEST: LLM-U-047
#[tokio::test]
async fn llm_u_047_the_gateways_own_refusal_is_told_apart_from_the_providers()
-> Result<(), Box<dyn Error>> {
    let case = |status, retry_after, gateway| Case {
        status,
        retry_after,
        gateway,
        throttle: None,
    };
    let errors = answers(&[
        case(503, None, Some("key_use_unrecorded")),
        case(429, Some("3"), Some("rate_limited")),
        case(503, Some("3"), Some("secret_unavailable")),
        case(403, None, Some("forbidden")),
    ])
    .await?;
    let three = Some(Duration::from_secs(3));
    let expected = [
        ("key_use_unrecorded", 503, None, true),
        ("rate_limited", 429, three, true),
        ("secret_unavailable", 503, three, true),
        ("forbidden", 403, None, false),
    ];
    for (error, (code, status, retry_after, retryable)) in errors.iter().zip(expected) {
        assert!(
            matches!(error, LlmError::GatewayRefused { code: actual, status: s, retry_after: r }
                if actual == code && *s == status && *r == retry_after),
            "{error:?}"
        );
        // Retried by its status, as before; its code says whose refusal it is.
        assert_eq!(error.retryable(), retryable, "{code}");
        assert_eq!(error.code(), "gateway");
        // The body is the gateway's words, not the provider's: none of it is quoted.
        assert_eq!(error.provider_message(), None);
        assert!(
            !error.to_string().contains("could not be recorded"),
            "{error}"
        );
        assert!(error.to_string().contains(code), "{error}");
    }
    Ok(())
}

// TEST: LLM-U-047
#[tokio::test]
async fn llm_u_047_the_relays_throttle_keeps_its_meaning_and_near_misses_are_the_providers()
-> Result<(), Box<dyn Error>> {
    let errors = answers(&[
        // The relay's throttle is the gateway's too, and keeps its wait-it-out handling.
        Case {
            status: 429,
            retry_after: Some("3"),
            gateway: Some("rate_limited"),
            throttle: Some("route"),
        },
        Case {
            status: 503,
            retry_after: None,
            gateway: Some("Not A Code"),
            throttle: None,
        },
        Case {
            status: 503,
            retry_after: None,
            gateway: Some(""),
            throttle: None,
        },
        // The body alone names a gateway code: still the provider's answer.
        Case {
            status: 503,
            retry_after: None,
            gateway: None,
            throttle: None,
        },
    ])
    .await?;
    assert!(matches!(
        errors[0],
        LlmError::Throttled {
            scope: ThrottleScope::Route,
            ..
        }
    ));
    for error in &errors[1..] {
        assert!(
            matches!(error, LlmError::Http { status: 503, .. }),
            "{error:?}"
        );
    }
    Ok(())
}

// TEST: LLM-U-047
#[test]
fn llm_u_047_a_gateway_code_is_a_bare_lowercase_token() {
    assert_eq!(GATEWAY_ERROR_HEADER, "x-tqa-gateway-error");
    for code in [
        "key_use_unrecorded",
        "rate_limited",
        "timeout",
        "a",
        "e2e_code_9",
    ] {
        assert_eq!(gateway_error_code(code), Some(code));
    }
    let long = "a".repeat(65);
    for value in [
        "",
        "Key_use",
        "key use",
        "key-use",
        " key",
        "key ",
        "9lives",
        "_lead",
        "key.use",
        "gateway.key",
        long.as_str(),
    ] {
        assert_eq!(gateway_error_code(value), None, "{value:?}");
    }
    assert_eq!(gateway_error_code(&"a".repeat(64)).map(str::len), Some(64));
}
