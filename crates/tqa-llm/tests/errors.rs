//! LLM-E-008/016 and LLM-C-019: public errors have stable codes and secret-safe formatting.

use std::error::Error;
use tqa_llm::{Completion, ConnectionFailure, LlmError, Registry, StreamLimitKind, TimeoutKind};

#[test]
fn llm_c_019_all_error_kinds_format_safely() -> Result<(), Box<dyn Error>> {
    let diagnostic = "x-api-key: example-value".to_owned();
    let registry = Registry::from_toml("bad=1")
        .err()
        .ok_or("accepted bad registry")?;
    assert_eq!(registry.code(), "registry");
    let errors = [
        (
            LlmError::Http {
                status: 401,
                body_excerpt: diagnostic.clone(),
            },
            "http",
            false,
        ),
        (
            LlmError::RateLimited { retry_after: None },
            "rate_limited",
            true,
        ),
        (LlmError::Timeout(TimeoutKind::Connect), "timeout", true),
        (
            LlmError::StreamCut {
                partial: Box::new(Completion {
                    text: diagnostic.clone(),
                    ..Completion::default()
                }),
            },
            "stream_cut",
            true,
        ),
        (LlmError::Protocol(diagnostic.clone()), "protocol", false),
        (LlmError::Malformed(diagnostic.clone()), "malformed", false),
        (
            LlmError::ToolArgsInvalid {
                index: 7,
                detail: diagnostic.clone(),
            },
            "tool_args_invalid",
            false,
        ),
        (
            LlmError::ToolArgsCut {
                index: 7,
                output_tokens: Some(64),
            },
            "tool_args_cut",
            false,
        ),
        (LlmError::BodyTooLarge { max: 1 }, "body_too_large", false),
        (
            LlmError::StreamLimit {
                which: StreamLimitKind::SseEvent,
                max: 1,
            },
            "stream_limit",
            false,
        ),
        (LlmError::UnsupportedProtocol, "unsupported_protocol", false),
        (LlmError::Registry(registry), "registry", false),
        (LlmError::Cancelled, "cancelled", false),
        (LlmError::Io(diagnostic), "io", false),
    ];
    for (error, code, retryable) in errors {
        assert_eq!(error.code(), code);
        assert_eq!(error.retryable(), retryable);
        let formatted = format!("{error} {error:?}");
        assert!(!formatted.contains("example-value"));
        assert!(!formatted.is_empty());
    }
    Ok(())
}

#[test]
fn llm_e_029_every_connection_failure_is_retryable() {
    // A connection failure before any response is retried, whatever its class (LLM-E-008, LLM-E-029).
    for failure in [
        ConnectionFailure::Refused,
        ConnectionFailure::Reset,
        ConnectionFailure::Closed,
        ConnectionFailure::Unresolved,
        ConnectionFailure::Unreachable,
    ] {
        let error = LlmError::Connection(failure);
        assert_eq!(
            (error.code(), error.retryable()),
            ("io", true),
            "{failure:?}"
        );
        assert!(!format!("{error}").is_empty());
    }
}
