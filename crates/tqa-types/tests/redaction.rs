//! LLM-C-019 and LLM-E-016: diagnostics never expose credentials.

use tqa_types::redact;

#[test]
fn llm_c_019_quoted_credentials_consume_escaped_quotes() {
    let input = r#"{"x-api-key":"demo\"continued","next":"safe"}"#;
    assert_eq!(redact(input), r#"{"x-api-key":"[redacted]","next":"safe"}"#);
    assert_eq!(redact("x-api-key: short\n"), "x-api-key: [redacted]\n");
}

#[test]
fn llm_c_019_masks_headers_query_values_and_key_shaped_tokens() {
    for input in [
        "Authorization: Bearer example-value\nnext: safe",
        "x-api-key: example-value\nnext: safe",
        "API-KEY=example-value&next=safe",
        r#"{"x-api-key":"example-value","next":"safe"}"#,
    ] {
        let output = redact(input);
        assert!(!output.contains("example-value"), "{output}");
        assert!(output.contains("[redacted]"));
        assert!(output.contains("safe"));
    }
    for token in [
        format!("sk-{}", "x".repeat(40)),
        "a1".repeat(20),
        "Ab9+/".repeat(10),
    ] {
        let output = redact(&format!("failure: {token}"));
        assert!(!output.contains(&token));
        assert!(output.contains("[redacted]"));
    }
    assert_eq!(
        redact("plain diagnostic with 界"),
        "plain diagnostic with 界"
    );
}
