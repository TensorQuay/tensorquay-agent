//! GW-U-037: the connector record (ADR-0031 decision 7) and its validator, shared by the `[[connectors]]` seed and the
//! admin API; every refusal names the field and a documented reason.
use super::{ConnectorDocument, Origin, Refusal, validate};
use crate::InvalidReason;
use serde_json::{Value, json};

fn document(value: Value) -> Result<ConnectorDocument, serde_json::Error> {
    serde_json::from_value(value)
}

fn minimal() -> Value {
    json!({"name":"tracker","url":"https://mcp.tracker.example/mcp"})
}

fn with(field: &str, value: Value) -> Value {
    let mut base = minimal();
    base[field] = value;
    base
}

fn refused(value: Value, origin: Origin) -> Option<Refusal> {
    let document = document(value).ok()?;
    validate(&document, origin, &[]).err()
}

#[test]
fn gw_u_037_a_minimal_connector_gets_the_documented_defaults()
-> Result<(), Box<dyn std::error::Error>> {
    let parsed = document(minimal())?;
    validate(&parsed, Origin::Admin, &[]).map_err(|refusal| format!("{refusal:?}"))?;
    assert_eq!(parsed.call_timeout_seconds, 60);
    assert!(parsed.enabled && !parsed.enabled_by_default && !parsed.insecure_http);
    assert_eq!(parsed.auth.scheme, super::Scheme::None);
    assert_eq!(
        serde_json::to_value(&parsed)?,
        json!({"name":"tracker","url":"https://mcp.tracker.example/mcp","auth":{"scheme":"none","required":false},
               "call_timeout_seconds":60,"enabled":true,"enabled_by_default":false}),
        "the stored form omits unset and file-only fields"
    );
    assert!(
        document(with("unknown", json!(1))).is_err(),
        "unknown fields are refused"
    );
    Ok(())
}

#[test]
fn gw_u_037_names_follow_the_shared_rule_and_never_take_a_stdio_servers_name() {
    for name in [
        "",
        "Tracker",
        "track__er",
        "tracker_",
        "t2345678901234567",
        "1tracker",
    ] {
        assert_eq!(
            refused(with("name", json!(name)), Origin::Admin),
            Some(Refusal::Invalid("name", InvalidReason::NameInvalid)),
            "{name:?}"
        );
    }
    let parsed = document(minimal()).ok();
    let taken = parsed.map(|parsed| validate(&parsed, Origin::Admin, &["tracker".into()]));
    assert_eq!(taken, Some(Err(Refusal::NameTaken)));
}

#[test]
fn gw_u_037_the_url_is_https_without_credentials_or_a_fragment() {
    for url in [
        "https://mcp.example/mcp",
        "https://mcp.example:8443/v1/mcp?tenant=acme",
        "https://10.1.2.3/mcp",
    ] {
        assert_eq!(
            refused(with("url", json!(url)), Origin::Admin),
            None,
            "{url}"
        );
    }
    // Exactly at the 2048-byte bound is accepted; one byte more is refused.
    let at_bound = format!(
        "https://mcp.example/{}",
        "a".repeat(2048 - "https://mcp.example/".len())
    );
    assert_eq!(at_bound.len(), 2048);
    assert_eq!(refused(with("url", json!(at_bound)), Origin::Admin), None);
    let long = format!("{at_bound}a");
    for url in [
        "http://mcp.example/mcp",
        "ftp://mcp.example/mcp",
        "https://user:pass@mcp.example/mcp",
        "https://user@mcp.example/mcp",
        "https://:pass@mcp.example/mcp",
        "https://mcp.example/mcp#top",
        "https://",
        "not a url",
        long.as_str(),
    ] {
        assert_eq!(
            refused(with("url", json!(url)), Origin::Admin),
            Some(Refusal::Invalid("url", InvalidReason::BaseUrlInvalid)),
            "{url}"
        );
    }
    for url in [
        "https://mcp.example/mcp?api_key=1",
        "https://mcp.example/mcp?Token=1",
        "https://mcp.example/mcp?x=1&signature=2",
    ] {
        assert_eq!(
            refused(with("url", json!(url)), Origin::Admin),
            Some(Refusal::Invalid("url", InvalidReason::AuthQueryInvalid)),
            "{url}"
        );
    }
}

#[test]
fn gw_u_037_plain_http_and_a_private_ca_are_file_only() {
    let insecure =
        json!({"name":"local","url":"http://mcp.internal:8080/mcp","insecure_http":true});
    assert_eq!(refused(insecure.clone(), Origin::File), None);
    assert_eq!(
        refused(insecure, Origin::Admin),
        Some(Refusal::Invalid("insecure_http", InvalidReason::FileOnly))
    );
    let ca = with("ca_file", json!("/etc/tqa/ca/tracker.pem"));
    assert_eq!(refused(ca.clone(), Origin::File), None);
    assert_eq!(
        refused(ca, Origin::Admin),
        Some(Refusal::Invalid("ca_file", InvalidReason::FileOnly))
    );
    assert_eq!(
        refused(with("ca_file", json!("relative.pem")), Origin::File),
        Some(Refusal::Invalid("ca_file", InvalidReason::ParameterInvalid))
    );
}

#[test]
fn gw_u_037_auth_schemes_are_data_and_a_credential_needs_one() {
    for auth in [
        json!({"scheme":"bearer"}),
        json!({"scheme":"bearer","required":true}),
        json!({"scheme":"header","header":"X-Api-Key"}),
        json!({"scheme":"header","header":"Authorization"}),
        json!({"scheme":"header","header":"X-Tracker-Token"}),
    ] {
        assert_eq!(
            refused(with("auth", auth.clone()), Origin::Admin),
            None,
            "{auth}"
        );
    }
    for (auth, reason) in [
        (json!({"scheme":"header"}), InvalidReason::AuthHeaderInvalid),
        (
            json!({"scheme":"header","header":"Cookie"}),
            InvalidReason::AuthHeaderInvalid,
        ),
        (
            json!({"scheme":"header","header":"X-"}),
            InvalidReason::AuthHeaderInvalid,
        ),
        (
            json!({"scheme":"header","header":format!("X-{}", "a".repeat(63))}),
            InvalidReason::AuthHeaderInvalid,
        ),
        (
            json!({"scheme":"header","header":"X-Bad Name"}),
            InvalidReason::AuthHeaderInvalid,
        ),
        (
            json!({"scheme":"bearer","header":"X-Api-Key"}),
            InvalidReason::AuthInvalid,
        ),
        (
            json!({"scheme":"none","required":true}),
            InvalidReason::AuthInvalid,
        ),
    ] {
        assert_eq!(
            refused(with("auth", auth.clone()), Origin::Admin),
            Some(Refusal::Invalid("auth", reason)),
            "{auth}"
        );
    }
    let mut shared = with("auth", json!({"scheme":"bearer"}));
    shared["secret_ref"] = json!("secrets/tracker");
    assert_eq!(refused(shared.clone(), Origin::Admin), None);
    shared["secret_ref"] = json!("../escape");
    assert_eq!(
        refused(shared, Origin::Admin),
        Some(Refusal::Invalid(
            "secret_ref",
            InvalidReason::SecretRefInvalid
        ))
    );
    assert_eq!(
        refused(with("secret_ref", json!("secrets/tracker")), Origin::Admin),
        Some(Refusal::Invalid("secret_ref", InvalidReason::AuthInvalid)),
        "a credential with no scheme to send it"
    );
}

#[test]
fn gw_u_037_tool_filters_and_timeouts_are_bounded() {
    let names = |count: usize| {
        json!(
            (0..count)
                .map(|index| format!("t{index}"))
                .collect::<Vec<_>>()
        )
    };
    assert_eq!(
        refused(with("tools", json!({"allow": names(256)})), Origin::Admin),
        None
    );
    assert_eq!(
        refused(
            with("tools", json!({"deny": ["delete_issue"]})),
            Origin::Admin
        ),
        None
    );
    for tools in [
        json!({"allow": names(257)}),
        json!({"allow": []}),
        json!({"allow": ["a"], "deny": ["b"]}),
        json!({}),
        json!({"allow": ["a", "a"]}),
        json!({"allow": [""]}),
        json!({"allow": ["x".repeat(129)]}),
        json!({"allow": ["bad\nname"]}),
    ] {
        assert_eq!(
            refused(with("tools", tools.clone()), Origin::Admin),
            Some(Refusal::Invalid("tools", InvalidReason::ToolFilterInvalid)),
            "{tools}"
        );
    }
    for timeout in [1, 600] {
        assert_eq!(
            refused(with("call_timeout_seconds", json!(timeout)), Origin::Admin),
            None
        );
    }
    for timeout in [0, 601] {
        assert_eq!(
            refused(with("call_timeout_seconds", json!(timeout)), Origin::Admin),
            Some(Refusal::Invalid(
                "call_timeout_seconds",
                InvalidReason::LimitOutOfRange
            ))
        );
    }
}

fn config(extra: &str) -> String {
    let image = format!("sha256:{}", "a".repeat(64));
    format!(
        "listen='127.0.0.1:8080'\npublic_origin='http://localhost:8080'\nmodels_path='/etc/tqa/models.toml'\n\
         [storage]\nroot='/srv/tqa'\n[runtime]\nbackend='docker'\nimage='{image}'\n\
         [sandbox]\ngateway_url='http://tqa-gateway:8080'\n{extra}"
    )
}

fn seed_refused(extra: &str) -> Option<&'static str> {
    match crate::GatewayConfig::from_toml(&config(extra)) {
        Err(crate::GatewayError::Configuration(reason)) => Some(reason),
        _ => None,
    }
}

/// The TOML parse's refusal of the seed (GW-U-144): the field's path and why.
fn seed_field_refused(extra: &str) -> Option<String> {
    match crate::GatewayConfig::from_toml(&config(extra)) {
        Err(crate::GatewayError::ConfigurationField(reason)) => Some(reason),
        _ => None,
    }
}

#[test]
fn gw_u_037_the_file_seed_is_validated_at_start() -> Result<(), Box<dyn std::error::Error>> {
    let one = "[[connectors]]\nname='tracker'\nurl='https://mcp.tracker.example/mcp'\n";
    let parsed = crate::GatewayConfig::from_toml(&config(one))?;
    assert_eq!(parsed.connectors.len(), 1);
    assert_eq!(
        seed_refused(
            "[[connectors]]\nname='local'\nurl='http://mcp.internal/mcp'\ninsecure_http=true\n"
        ),
        None,
        "the file may use plain http"
    );
    assert_eq!(
        seed_refused(&format!("{one}{one}")),
        Some("duplicate [[connectors]] name")
    );
    assert_eq!(
        seed_refused(&format!(
            "[[mcp.servers]]\nname='tracker'\ncommand='/bin/t'\n{one}"
        )),
        Some("a [[connectors]] name is taken by [[mcp.servers]]")
    );
    assert_eq!(
        seed_refused("[[connectors]]\nname='tracker'\nurl='https://mcp.example/mcp?token=1'\n"),
        Some("invalid [[connectors]] entry (see the log)")
    );
    let extra = seed_field_refused(
        "[[connectors]]\nname='tracker'\nurl='https://mcp.example/mcp'\nextra=1\n",
    )
    .unwrap_or_default();
    assert!(
        extra.starts_with("connectors[0].extra: unknown field, expected "),
        "{extra}"
    );
    let many: String = (0..65)
        .map(|index| format!("[[connectors]]\nname='c{index}'\nurl='https://mcp.example/mcp'\n"))
        .collect::<Vec<_>>()
        .concat();
    assert_eq!(
        seed_refused(&many),
        Some("too many [[connectors]] (at most 64)")
    );
    let at_most: String = (0..64)
        .map(|index| format!("[[connectors]]\nname='c{index}'\nurl='https://mcp.example/mcp'\n"))
        .collect::<Vec<_>>()
        .concat();
    assert_eq!(seed_refused(&at_most), None);
    Ok(())
}

#[test]
fn gw_u_037_a_file_connectors_plain_http_survives_the_stored_form()
-> Result<(), Box<dyn std::error::Error>> {
    // The seed stores the document and the snapshot reads it back: `insecure_http` must not be dropped on the way.
    let parsed = document(
        json!({"name":"tracker","url":"http://tracker.internal/mcp","insecure_http":true}),
    )?;
    let stored = serde_json::to_value(&parsed)?;
    assert_eq!(stored["insecure_http"], json!(true));
    assert!(document(stored)?.insecure_http);
    Ok(())
}
