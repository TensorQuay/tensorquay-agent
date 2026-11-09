//! LLM-U-036 and LLM-U-040: the provider's own words from an HTTP error body, sanitised for `run.failed` and `provider.retry` (owner
//! bug 2): the message field when the body is JSON, else the text; no control characters; whitespace collapsed;
//! credentials redacted, including a short token after Bearer or Basic; at most 300 characters on a character boundary.
use tqa_llm::LlmError;
use tqa_types::valid_provider_message;

fn http(body: &str) -> LlmError {
    LlmError::Http {
        status: 400,
        body_excerpt: body.into(),
    }
}
fn message(body: &str) -> Option<String> {
    http(body).provider_message()
}

#[test]
fn llm_u_036_the_providers_message_field_is_preferred() {
    let cases = [
        (
            r#"{"error":{"message":"The model `x` does not exist","type":"invalid_request_error"}}"#,
            "The model `x` does not exist",
        ),
        (
            r#"{"type":"error","error":{"type":"invalid_request_error","message":"max_tokens is too large"}}"#,
            "max_tokens is too large",
        ),
        (r#"{"message":"quota exceeded"}"#, "quota exceeded"),
        (r#"{"error":"route not found"}"#, "route not found"),
        (
            "upstream connect error or disconnect",
            "upstream connect error or disconnect",
        ),
        (
            "<html><body>502 Bad Gateway</body></html>",
            "<html><body>502 Bad Gateway</body></html>",
        ),
        (
            r#"{"detail":{"reason":"nested"}}"#,
            r#"{"detail":{"reason":"nested"}}"#,
        ),
    ];
    for (body, expected) in cases {
        assert_eq!(message(body).as_deref(), Some(expected), "{body}");
    }
}

#[test]
fn llm_u_036_control_characters_go_and_whitespace_collapses() {
    assert_eq!(
        message("line one\nline two\r\n\tindented\u{0}nul\u{7f}del\u{85}c1").as_deref(),
        Some("line one line two indented nul del c1")
    );
    assert_eq!(message("  padded   text  ").as_deref(), Some("padded text"));
    for body in ["", "   ", "\n\t\u{0}"] {
        assert_eq!(message(body), None, "{body:?}");
    }
}

#[test]
fn llm_u_036_every_credential_form_is_redacted() {
    for (body, secret) in [
        (
            "rejected Authorization: Bearer abc123 for this route",
            "abc123",
        ),
        ("token Bearer shorttok was revoked", "shorttok"),
        ("bearer lower-case-tok is invalid", "lower-case-tok"),
        ("Basic dXNlcjpwYXNz is not accepted", "dXNlcjpwYXNz"),
        ("header x-api-key: k-12345 invalid", "k-12345"),
        ("query api-key=k-67890 invalid", "k-67890"),
        ("Incorrect API key provided: sk-proj-abc", "sk-proj-abc"),
        (
            "token 0123456789abcdef0123456789abcdef0123 expired",
            "0123456789abcdef0123456789abcdef0123",
        ),
        (
            r#"{"error":{"message":"Invalid key Bearer tok9 given"}}"#,
            "tok9",
        ),
    ] {
        let text = message(body).unwrap_or_default();
        assert!(!text.contains(secret), "{secret} survived in {text}");
        assert!(text.contains("[redacted]"), "{text}");
    }
}

#[test]
fn llm_u_036_the_message_is_bounded_on_a_character_boundary() {
    // Word-shaped text: one unbroken run of 32 characters or more is key-shaped and redacted as a whole.
    let long = message(&"word ".repeat(2048)).unwrap_or_default();
    assert_eq!(long.chars().count(), 300);
    assert!(long.ends_with('…'));
    // Multi-byte text at the cut: never split, and still at most 300 characters.
    let wide = message(&"ééé ".repeat(100)).unwrap_or_default();
    assert_eq!(wide.chars().count(), 300);
    assert!(valid_provider_message(&long) && valid_provider_message(&wide));
    let exact = format!("{}abc", "ab ".repeat(99));
    assert_eq!(exact.chars().count(), 300);
    assert_eq!(
        message(&exact).as_deref(),
        Some(exact.as_str()),
        "300 characters are kept whole"
    );
}

#[test]
fn llm_u_036_only_http_errors_carry_the_providers_words() {
    for error in [
        LlmError::RateLimited { retry_after: None },
        LlmError::Unavailable { retry_after: None },
        LlmError::Timeout(tqa_llm::TimeoutKind::Idle),
    ] {
        assert_eq!(error.provider_message(), None, "{error:?}");
    }
    for body in ["x", &"é".repeat(1000), "a\nb", "Bearer t"] {
        let text = message(body).unwrap_or_default();
        assert!(
            valid_provider_message(&text),
            "{text:?} fits the protocol's rule"
        );
    }
}

/// LLM-U-040: each must-pass provider's error body shape, as observed or documented.
#[test]
fn llm_u_040_each_providers_error_shape_yields_its_message() {
    for (shape, body, expected) in [
        (
            "flat detail (observed on 27 Sep 2026)",
            r#"{"detail":"System messages are not allowed"}"#,
            "System messages are not allowed",
        ),
        (
            "flat detail, an unsupported parameter",
            r#"{"detail":"Unsupported parameter: metadata"}"#,
            "Unsupported parameter: metadata",
        ),
        (
            "the documented error object",
            r#"{"error":{"message":"Invalid schema for function 'edit_file'","type":"invalid_request_error","param":"tools[0].parameters","code":"invalid_function_parameters"}}"#,
            "Invalid schema for function 'edit_file'",
        ),
        (
            "the Messages error shape",
            r#"{"type":"error","error":{"type":"invalid_request_error","message":"max_tokens: Field required"}}"#,
            "max_tokens: Field required",
        ),
        (
            "an error object with code and status",
            r#"{"error":{"code":400,"message":"Invalid JSON payload received.","status":"INVALID_ARGUMENT"}}"#,
            "Invalid JSON payload received.",
        ),
        (
            "an error object with code and status, list-wrapped",
            r#"[{"error":{"code":400,"message":"Invalid JSON payload received.","status":"INVALID_ARGUMENT"}}]"#,
            "Invalid JSON payload received.",
        ),
    ] {
        assert_eq!(message(body).as_deref(), Some(expected), "{shape}");
    }
    // A detail that isn't text (a validation list) falls back to the sanitised body text.
    let list = r#"{"detail":[{"loc":["body","input"],"msg":"field required"}]}"#;
    assert_eq!(message(list).as_deref(), Some(list));
}
