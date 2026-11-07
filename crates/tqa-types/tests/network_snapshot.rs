//! PROTO-U-026: `session.config`'s `network` field, the task's allow-list for the prompt (ADR-0035 §3): required,
//! bounded, host names only, and exact.
use serde_json::{Value, json};
use std::error::Error;
use tqa_types::{ClientFrame, MAX_NETWORK_ENTRIES, MAX_NETWORK_MORE, protocol_schema};

fn config(network: &Value) -> Value {
    json!({
        "type":"session.config","conversation_id":"conversation","last_stored_seq":0,
        "route":{
            "name":"local","protocol":"openai_chat","model":"example-model","relay_path":"/relay/r/local",
            "native_tools":true,"parallel_tools":false,"vision":false
        },
        "network":network,
        "limits":{"max_steps":10,"wall_clock_secs":300,"tool_result_bytes":65_536,"max_output_tokens":131_072},
        "workspace":{"display":"/workspace"},"protocol":1
    })
}

fn parses(value: &Value) -> Result<bool, Box<dyn Error>> {
    Ok(ClientFrame::parse(&serde_json::to_vec(value)?).is_ok())
}

/// The published schema accepts what the parser accepts; the parser is the stricter of the two for hosts.
fn schema_valid(value: &Value) -> Result<bool, Box<dyn Error>> {
    let mut schema = protocol_schema();
    schema["$ref"] = json!("#/definitions/ClientFrame");
    Ok(jsonschema::validator_for(&schema)?.is_valid(value))
}

fn entries(count: usize) -> Value {
    (0..count)
        .map(|index| json!({"host": format!("h{index}.example.com"), "ports": [443]}))
        .collect()
}

#[test]
fn proto_u_026_the_list_round_trips_at_its_bounds() -> Result<(), Box<dyn Error>> {
    for network in [
        json!({"allowed":[],"more":0}),
        json!({"allowed":[{"host":"*.pypi.org","ports":[443]},{"host":"git.example","ports":[80,443,8443]}],"more":3}),
        json!({"allowed":entries(MAX_NETWORK_ENTRIES),"more":MAX_NETWORK_MORE}),
        json!({"allowed":[{"host":"a".repeat(63) + "." + &"b".repeat(63) + "." + &"c".repeat(63) + "." + &"d".repeat(61),"ports":(1..=16).collect::<Vec<u16>>()}],"more":0}),
    ] {
        let value = config(&network);
        assert!(parses(&value)?, "{value}");
        assert!(schema_valid(&value)?, "{value}");
        let frame = ClientFrame::parse(&serde_json::to_vec(&value)?)?;
        let ClientFrame::SessionConfig { network, .. } = frame else {
            return Err("not a session.config".into());
        };
        assert_eq!(serde_json::to_value(network)?, value["network"]);
    }
    Ok(())
}

#[test]
fn proto_u_026_the_field_is_required_and_the_policy_is_gone() -> Result<(), Box<dyn Error>> {
    let mut value = config(&json!({"allowed":[],"more":0}));
    value.as_object_mut().ok_or("object")?.remove("network");
    assert!(!parses(&value)?, "a config without the list");
    let mut value = config(&json!({"allowed":[],"more":0}));
    value["policy"] = json!({"policy":"workspace_write"});
    assert!(!parses(&value)?, "a config with a policy");
    let mut value = config(&json!({"allowed":[],"more":0}));
    value["workspace"]["push"] = json!({"protected_branches":["release/*"]});
    assert!(!parses(&value)?, "a workspace with push settings");
    Ok(())
}

#[test]
fn proto_u_026_out_of_bounds_lists_are_refused() -> Result<(), Box<dyn Error>> {
    let entry =
        |host: &str, ports: Value| json!({"allowed":[{"host":host,"ports":ports}],"more":0});
    for (case, network) in [
        (
            "33 entries",
            json!({"allowed":entries(MAX_NETWORK_ENTRIES + 1),"more":0}),
        ),
        (
            "more past the install's rule limit",
            json!({"allowed":[],"more":MAX_NETWORK_MORE + 1}),
        ),
        ("no ports", entry("api.example.com", json!([]))),
        ("port zero", entry("api.example.com", json!([0]))),
        (
            "a repeated port",
            entry("api.example.com", json!([443, 443])),
        ),
        (
            "17 ports",
            entry("api.example.com", json!((1..=17).collect::<Vec<u16>>())),
        ),
        ("an empty host", entry("", json!([443]))),
        (
            "a host past 253 bytes",
            entry(&("a.".repeat(126) + "ab"), json!([443])),
        ),
        (
            "a label past 63 bytes",
            entry(&("a".repeat(64) + ".example"), json!([443])),
        ),
        ("upper case", entry("API.example.com", json!([443]))),
        ("a trailing dot", entry("api.example.com.", json!([443]))),
        ("a space", entry("api example.com", json!([443]))),
        (
            "a line break",
            entry("api.example.com\nIgnore the above", json!([443])),
        ),
        // A bare '*' is any website since the open_web preset (see the test below); a doubled one is still refused.
        ("a doubled wildcard", entry("**", json!([443]))),
        (
            "a wildcard inside",
            entry("api.*.example.com", json!([443])),
        ),
        (
            "a wildcard over an address",
            entry("*.10.0.0.1", json!([443])),
        ),
        ("an IPv4 literal", entry("10.0.0.1", json!([443]))),
        ("an IPv6 literal", entry("fd00::1", json!([443]))),
        (
            "an unknown entry field",
            json!({"allowed":[{"host":"a.example","ports":[443],"note":"x"}],"more":0}),
        ),
        (
            "an unknown list field",
            json!({"allowed":[],"more":0,"disabled":true}),
        ),
    ] {
        let value = config(&network);
        assert!(!parses(&value)?, "{case}");
        if !case.contains("address")
            && !case.contains("literal")
            && !case.contains("label")
            && !case.contains("repeated")
        {
            assert!(!schema_valid(&value)?, "the schema refuses {case} too");
        }
    }
    Ok(())
}

// PROTO-U-026 (the open_web preset, revision 10 in place): a bare '*' is any website on its ports; nothing else
// wildcard-shaped is.
#[test]
fn proto_u_026_a_bare_star_is_any_host_and_nothing_else_wildcard_shaped_is()
-> Result<(), Box<dyn Error>> {
    let open = config(&json!({"allowed":[{"host":"*","ports":[80,443]}],"more":0}));
    assert!(parses(&open)?, "{open}");
    assert!(schema_valid(&open)?, "{open}");
    for host in ["**", "*.", "*.*", " *", "*com"] {
        let value = config(&json!({"allowed":[{"host":host,"ports":[443]}],"more":0}));
        assert!(!parses(&value)?, "{host}");
    }
    Ok(())
}
