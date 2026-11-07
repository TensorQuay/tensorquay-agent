//! GW-U-144: a configuration the TOML parse refuses names the field by its path and says why, as serde's own error
//! does, and never holds a value from the file (ENGINEERING-GUARDRAILS §1.5).
use super::{printable, reason};
use crate::GatewayConfig;

/// A generated test-only value; it must never appear in a refusal.
const SECRET: &str = "s3cret-value-7f21";

fn config(extra: &str) -> String {
    let image = format!("sha256:{}", "a".repeat(64));
    format!(
        "listen='127.0.0.1:8080'\npublic_origin='http://localhost:8080'\nmodels_path='/etc/tqa/models.toml'\n\
         [storage]\nroot='/srv/tqa'\n[runtime]\nbackend='docker'\nimage='{image}'\n\
         [sandbox]\ngateway_url='http://tqa-gateway:8080'\n{extra}"
    )
}

fn server(fields: &str) -> String {
    config(&format!(
        "[[mcp.servers]]\nname='docs'\ncommand='/usr/local/bin/docs-mcp'\n{fields}"
    ))
}

/// The refusal's text, after checking it holds no value from the file.
fn refusal(source: &str) -> String {
    let text = match GatewayConfig::from_toml(source) {
        Ok(_) => String::from("accepted"),
        Err(error) => error.to_string(),
    };
    assert!(
        !text.contains(SECRET),
        "a value reached the refusal: {text}"
    );
    text
}

#[test]
fn gw_u_144_an_unknown_field_is_named_by_its_path() {
    let text = refusal(&config(&format!("surprise='{SECRET}'\n")));
    assert!(
        text.starts_with(
            "invalid gateway configuration: sandbox.surprise: unknown field, expected one of `"
        ),
        "{text}"
    );
    let text = refusal(&server("unknown=1\n"));
    assert!(
        text.starts_with(
            "invalid gateway configuration: mcp.servers[0].unknown: unknown field, expected "
        ),
        "{text}"
    );
}

#[test]
fn gw_u_144_a_wrong_type_says_what_was_found_and_expected_without_the_value() {
    let text = refusal(&server(&format!("call_timeout_seconds='{SECRET}'\n")));
    assert!(
        text.starts_with(
            "invalid gateway configuration: mcp.servers[0].call_timeout_seconds: invalid type: found a string, \
             expected "
        ),
        "{text}"
    );
}

#[test]
fn gw_u_144_a_missing_field_is_named_with_its_table() {
    let text = refusal(&config("[[mcp.servers]]\nname='docs'\n"));
    assert_eq!(
        text,
        "invalid gateway configuration: mcp.servers[0]: missing field `command`"
    );
}

#[test]
fn gw_u_144_a_syntax_error_names_its_line_and_column_not_the_line() {
    let source = config(&format!("[mcp\nkey='{SECRET}'\n"));
    let line = source.lines().count() - 1;
    let text = refusal(&source);
    assert_eq!(
        text,
        format!(
            "invalid gateway configuration: TOML syntax error at line {line}, column 5: unclosed table, expected `]`"
        )
    );
}

#[test]
fn gw_u_144_a_key_that_is_not_a_plain_name_is_not_repeated() {
    // A quoted key of name characters reads as the bare key would, and is named; one with any other is not repeated.
    let text = refusal(&config(&format!("'token: {SECRET}'=1\n")));
    assert!(
        text.starts_with(
            "invalid gateway configuration: a field with an unprintable name: unknown field, expected "
        ),
        "{text}"
    );
}

#[test]
fn gw_u_144_a_credential_pasted_as_a_key_is_masked() {
    let github = format!(
        "ghp_{}",
        "A1b2C3d4E5".repeat(4).get(..36).unwrap_or_default()
    );
    let openai = "sk-live-4f9a";
    for key in [github.as_str(), openai] {
        let text = refusal(&config(&format!("{key}=1\n")));
        assert!(!text.contains(key), "{text}");
        assert!(
            text.starts_with(
                "invalid gateway configuration: sandbox.[redacted]: unknown field, expected "
            ),
            "{text}"
        );
    }
    let nested = refusal(&server(&format!("{openai}=1\n")));
    assert!(
        nested
            .starts_with("invalid gateway configuration: mcp.servers[0].[redacted]: unknown field"),
        "{nested}"
    );
}

#[test]
fn gw_u_144_every_value_serde_quotes_is_left_out() {
    for (message, expected) in [
        (
            "invalid type: string \"s3cret-value-7f21\", expected u64",
            "invalid type: found a string, expected u64",
        ),
        (
            "invalid type: floating point `1.5`, expected u16",
            "invalid type: found a floating point number, expected u16",
        ),
        (
            "invalid type: boolean `true`, expected a string",
            "invalid type: found a boolean, expected a string",
        ),
        (
            "invalid type: sequence, expected a string",
            "invalid type: found an array, expected a string",
        ),
        (
            "invalid type: map, expected u64",
            "invalid type: found a table, expected u64",
        ),
        (
            "invalid value: integer `99999999`, expected u16",
            "invalid value: found an integer, expected u16",
        ),
        (
            "unknown variant `s3cret-value-7f21`, expected `docker` or `firecracker`",
            "unknown variant, expected `docker` or `firecracker`",
        ),
        (
            "unknown field `quotta`, expected `root` or `quota`",
            "unknown field, expected `root` or `quota`",
        ),
        ("missing field `root`", "missing field `root`"),
        ("missing field `max_live`", "missing field `max_live`"),
        // Not a name the schema could have: serde quoted something else.
        ("missing field `s3cret-value`", "invalid value"),
        ("duplicate field `root`", "duplicate field `root`"),
        (
            "invalid length 3, expected a pair",
            "invalid length 3, expected a pair",
        ),
        (
            "[sandbox.ceiling] cpus is out of range",
            "[sandbox.ceiling] cpus is out of range",
        ),
        // Anything else may hold a value (a custom error), so it is only called invalid.
        (
            "bad address s3cret-value-7f21 for the pool",
            "invalid value",
        ),
        ("invalid type: s3cret-value-7f21", "invalid value"),
        // A kind this version doesn't name, and a name without serde's separator after it.
        (
            "invalid type: newtype struct s3cret-value-7f21, expected u64",
            "invalid value",
        ),
        (
            "unknown field `s3cret-value-7f21` in this table",
            "invalid value",
        ),
        (
            "unknown field `s3cret-value-7f21`, there are no fields",
            "unknown field, there are no fields",
        ),
        (
            "unknown variant `s3cret-value-7f21`, there are no variants",
            "unknown variant, there are no variants",
        ),
        (
            "unknown variant `a`, expected s3cret-value-7f21`, expected `docker` or `firecracker`",
            "unknown variant, expected `docker` or `firecracker`",
        ),
    ] {
        assert_eq!(reason(message), expected, "{message}");
    }
}

#[test]
fn gw_u_144_a_long_reason_is_cut_at_a_bound() {
    let expected = (0..200)
        .map(|n| format!("`f{n}`"))
        .collect::<Vec<_>>()
        .join(", ");
    let cut = reason(&format!("unknown field `x`, expected one of {expected}"));
    assert!(cut.len() <= 512, "{}", cut.len());
    assert!(cut.ends_with('…'), "{cut}");
    // The bound falls inside a two-byte character: the cut moves back to its start, never splitting it.
    let wide = reason(&format!("unknown field `x`, expected {}", "é".repeat(300)));
    assert_eq!(wide.len(), 508 + '…'.len_utf8(), "{}", wide.len());
    assert!(wide.ends_with("é…"), "{wide}");
}

#[test]
fn gw_u_144_the_root_and_an_unknown_segment_read_as_what_they_are() {
    assert_eq!(printable("."), "the top level");
    for unknown in ["?", "mcp.?", "servers.?[0].name"] {
        assert_eq!(
            printable(unknown),
            "a field whose path is unknown",
            "{unknown}"
        );
    }
    assert_eq!(printable("mcp.servers[0].name"), "mcp.servers[0].name");
    assert_eq!(printable("a b"), "a field with an unprintable name");
}

#[test]
fn gw_u_144_a_missing_top_level_field_names_the_top_level() {
    let text = refusal("public_origin='http://localhost:8080'\n");
    assert!(
        text.starts_with("invalid gateway configuration: the top level: missing field `"),
        "{text}"
    );
}
