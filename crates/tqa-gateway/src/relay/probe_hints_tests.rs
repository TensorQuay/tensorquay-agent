//! GW-U-059: a probe refused with 400 suggests the route setting its wording points at; anything else suggests
//! nothing. Wordings are the ones providers answer with (paraphrased where long), never a provider's name.
use super::probe::{Failure, fail, resolves};
use super::probe_hints::{protocol_endpoint, suggestion};

fn suggested(status: u16, excerpt: &str) -> Option<(&'static str, String)> {
    suggestion(status, excerpt).map(|hint| (hint.field, hint.value.into_owned()))
}

#[test]
fn gw_u_059_a_schema_refused_under_strict_mode_suggests_strict_off() {
    for excerpt in [
        r#"{"error":{"message":"Invalid schema for function 'read_file': In context=(), 'additionalProperties' is required to be supplied and to be false."}}"#,
        r#"{"error":{"message":"Invalid schema for function 'grep': 'required' is required to be supplied and to be an array including every key in properties. Missing 'path'."}}"#,
        "strict mode: the schema of tool glob is not supported",
    ] {
        assert_eq!(
            suggested(400, excerpt),
            Some(("strict", "off".to_owned())),
            "{excerpt}"
        );
    }
}

#[test]
fn gw_u_059_an_unsupported_keyword_suggests_basic_schemas_first() {
    for excerpt in [
        r#"{"error":{"message":"Invalid JSON payload: Unknown name \"pattern\" at 'tools[0].function_declarations[0].parameters.properties[0].value'"}}"#,
        "schema keyword 'maxLength' is not supported",
        r#"[{"error":{"code":400,"message":"Invalid JSON payload received. Unknown name \"additionalProperties\" at 'tools[0].function_declarations[0].parameters': Cannot find field.","status":"INVALID_ARGUMENT"}}]"#,
        r#"{"error":{"message":"Invalid schema for function 'x': '$ref' is not supported in strict mode"}}"#,
    ] {
        assert_eq!(
            suggested(400, excerpt),
            Some(("schema_keywords", "basic".to_owned())),
            "{excerpt}"
        );
    }
}

#[test]
fn gw_u_059_nothing_is_suggested_without_a_400_and_a_known_wording() {
    let schema = r"Invalid schema for function 'read_file': 'additionalProperties' is required";
    for (status, excerpt) in [
        (500, schema),
        (401, schema),
        (429, schema),
        (400, "max_tokens is too large"),
        (400, ""),
        // A role refusal waits for the instructions_role setting.
        (400, "system role is not supported for this model"),
    ] {
        assert_eq!(suggested(status, excerpt), None, "{status} {excerpt}");
    }
}

#[test]
fn gw_u_059_a_suggestion_explains_itself() {
    let hint = suggestion(
        400,
        "Invalid schema for function 'a': 'additionalProperties' is required",
    )
    .ok_or("hint");
    assert!(hint.is_ok_and(|hint| hint.reason.contains("strict")));
}

/// The check's own request refused with `status` and this body.
fn refused(code: &'static str, status: u16, excerpt: &str) -> Failure {
    let mut failure = fail(code);
    failure.status = Some(status);
    failure.excerpt = Some(excerpt.to_owned());
    failure
}

// TEST: GW-U-154
#[test]
fn gw_u_154_a_404_or_405_at_the_protocols_path_is_a_missing_endpoint_unless_the_model_is_unknown() {
    // No endpoint for this protocol at this address: another protocol, or a missing /v1.
    for (status, excerpt) in [
        (404, "404 page not found"),
        (
            404,
            r#"{"error":{"message":"Invalid URL (POST /v1/messages)","type":"invalid_request_error"}}"#,
        ),
        (
            404,
            r#"{"type":"error","error":{"type":"not_found_error","message":"Not Found"}}"#,
        ),
        (404, ""),
        (405, "Method Not Allowed"),
        (405, r#"{"error":"model not found"}"#),
    ] {
        let code = if status == 404 {
            "not_found"
        } else {
            "upstream_error"
        };
        assert_eq!(
            protocol_endpoint(refused(code, status, excerpt)).code,
            "endpoint_missing",
            "{status} {excerpt}"
        );
    }
    // The endpoint is there; the model isn't.
    for excerpt in [
        r#"{"error":{"message":"model \"qwen3:4b\" not found, try pulling it first","type":"api_error"}}"#,
        r#"{"object":"error","message":"The model `gpt-x` does not exist.","type":"NotFoundError","code":404}"#,
        r#"{"error":{"code":"model_not_found","message":"The model `o9` does not exist or you do not have access to it."}}"#,
        r#"{"type":"error","error":{"type":"not_found_error","message":"model: claude-x"}}"#,
    ] {
        let failure = protocol_endpoint(refused("not_found", 404, excerpt));
        assert_eq!(failure.code, "not_found", "{excerpt}");
    }
    // Every other failure is unchanged.
    for (code, status) in [
        ("auth_rejected", 401),
        ("rate_limited", 429),
        ("upstream_error", 500),
        ("upstream_error", 400),
    ] {
        assert_eq!(protocol_endpoint(refused(code, status, "x")).code, code);
    }
    assert_eq!(protocol_endpoint(fail("unreachable")).code, "unreachable");
}

// TEST: GW-U-154
#[tokio::test]
async fn gw_u_154_a_name_resolves_or_doesnt_within_its_bound() {
    assert!(resolves("localhost").await);
    // .invalid never resolves (RFC 6761), so a gateway without the container host's name answers false.
    assert!(!resolves("container-host.invalid").await);
}
