//! PROTO-U-032 (ADR-0037 §3, revision 11): the run's keys reach the runtime on the delivered `user.message`, each with
//! its variable, where it goes and its placeholder. The field is optional, bounded and checked, the Rust parser and
//! the schema agree on it, and a debug print never shows a placeholder. Revision 11 also brings the install CA and a
//! resumed run's keys in `session.config`, and the live `host_key.used` and `host_key.refused` frames.
//! PROTO-U-033: a grant's host may be an exact IP address, in its canonical form.
use serde_json::{Value, json};
use tqa_types::{
    ClientFrame, HostKeyGrant, MAX_RUN_HOST_KEYS, MIN_RUNTIME_REVISION, PROTOCOL_REVISIONS,
    ServerFrame, protocol_schema,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const PLACEHOLDER: &str = "tqa_ph_0123456789abcdefghijklmnopqrstuvwxyzABCDEFG";

fn grant() -> Value {
    json!({"env": "GITHUB_WORK", "host": "github.com", "port": 443, "path_prefix": "/acme", "scheme": "basic",
           "user_name": "x-access-token", "placeholder": PLACEHOLDER})
}

fn message(host_keys: Option<Value>) -> Value {
    let mut frame = json!({"type": "user.message", "text": "hello", "attachments": []});
    if let Some(host_keys) = host_keys {
        frame["host_keys"] = host_keys;
    }
    frame
}

fn with(field: &str, value: Value) -> Value {
    let mut grant = grant();
    grant[field] = value;
    grant
}

// TEST: PROTO-U-032
#[test]
fn proto_u_032_the_run_keys_on_the_delivered_message_are_bounded_and_checked() -> TestResult {
    let mut schema = protocol_schema();
    schema["$ref"] = json!("#/definitions/ClientFrame");
    let validator = jsonschema::validator_for(&schema)?;
    let bearer = json!({"env": "API", "host": "api.example.com", "port": 8443, "path_prefix": null,
                        "scheme": "bearer", "user_name": null, "placeholder": PLACEHOLDER.replace('0', "1")});
    let many: Vec<Value> = (0..=MAX_RUN_HOST_KEYS)
        .map(|index| with("env", json!(format!("KEY_{index}"))))
        .collect();
    for (host_keys, valid) in [
        (None, true),
        (Some(json!([])), true),
        (Some(json!([grant(), bearer])), true),
        (Some(json!(many[..MAX_RUN_HOST_KEYS].to_vec())), true),
        (Some(json!(many)), false),
        (Some(json!([grant(), grant()])), false),
        (Some(json!([with("env", json!("github_work"))])), false),
        (Some(json!([with("env", json!("1KEY"))])), false),
        (Some(json!([with("host", json!(""))])), false),
        (Some(json!([with("host", json!("GitHub.com"))])), false),
        // An exact address: IPv4, and IPv6 in its canonical form without brackets (ADR-0037 Amendment 2).
        (Some(json!([with("host", json!("192.0.2.10"))])), true),
        (Some(json!([with("host", json!("2001:db8::1"))])), true),
        (Some(json!([with("host", json!("[2001:db8::1]"))])), false),
        (Some(json!([with("port", json!(0))])), false),
        (Some(json!([with("path_prefix", json!("acme"))])), false),
        (Some(json!([with("scheme", json!("header"))])), false),
        (
            Some(json!([with("placeholder", json!("tqa_ph_short"))])),
            false,
        ),
        (
            Some(json!([with(
                "placeholder",
                json!(PLACEHOLDER.replace("tqa_ph_", "tqa_xx_"))
            )])),
            false,
        ),
        (
            Some(json!([with(
                "placeholder",
                json!(PLACEHOLDER.replace('0', "+"))
            )])),
            false,
        ),
        (Some(json!([with("extra", json!(1))])), false),
    ] {
        let wire = message(host_keys);
        assert_eq!(
            ClientFrame::parse(&serde_json::to_vec(&wire)?).is_ok(),
            valid,
            "{wire}"
        );
        assert_eq!(validator.is_valid(&wire), valid, "schema: {wire}");
    }
    Ok(())
}

// TEST: PROTO-U-032
#[test]
fn proto_u_032_a_key_reads_back_and_never_prints_its_placeholder() -> TestResult {
    // Read back as sent; a message with no keys is written as before, so the shape without keys is unchanged.
    let ClientFrame::UserMessage { host_keys, .. } =
        ClientFrame::parse(&serde_json::to_vec(&message(Some(json!([grant()]))))?)?
    else {
        return Err("not a user.message".into());
    };
    let [read] = host_keys.as_slice() else {
        return Err("one key".into());
    };
    assert_eq!(
        (
            read.env(),
            read.host(),
            read.port(),
            read.path_prefix(),
            read.placeholder()
        ),
        ("GITHUB_WORK", "github.com", 443, Some("/acme"), PLACEHOLDER)
    );
    assert!(
        !format!("{read:?}").contains(&PLACEHOLDER[7..]),
        "a debug print shows no placeholder"
    );
    let plain = ClientFrame::parse(&serde_json::to_vec(&message(None))?)?;
    assert!(!serde_json::to_string(&plain)?.contains("host_keys"));
    let built = HostKeyGrant::new(
        "GITHUB_WORK".into(),
        ("github.com".into(), 443, Some("/acme".into())),
        ("basic".into(), Some("x-access-token".into())),
        PLACEHOLDER.into(),
    )?;
    assert_eq!(&built, read);
    assert!(
        HostKeyGrant::new(
            "x".into(),
            ("github.com".into(), 443, None),
            ("bearer".into(), None),
            PLACEHOLDER.into()
        )
        .is_err()
    );
    Ok(())
}

// TEST: PROTO-U-032
#[test]
fn proto_u_032_revision_11_adds_them_and_the_floor_stays_10() {
    // The gateway sends them only to a runtime announcing 11, since an older one would refuse the field.
    // Later revisions (12: the lead's brief) keep both: keys came in 11, and the floor stays 10. The current revision
    // is checked against the table in protocol_revision.rs.
    assert_eq!(
        (MIN_RUNTIME_REVISION, tqa_types::HOST_KEYS_REVISION),
        (10, 11)
    );
    assert_eq!(
        tqa_types::ErrorCode::HostKeysUnsupported.as_str(),
        "host_keys_unsupported"
    );
    assert!(!tqa_types::ErrorCode::HostKeysUnsupported.retryable());
    let revision = PROTOCOL_REVISIONS[10];
    assert_eq!(revision.number, 11);
    assert!(revision.schema.contains(&("ClientFrame", "host_keys")));
}

fn session(host_ca: Option<Value>) -> Value {
    let mut frame = json!({
        "type":"session.config","conversation_id":"conversation","last_stored_seq":0,
        "route":{"name":"local","protocol":"openai_chat","model":"m","relay_path":"/relay/r/local",
            "context_window":8192,"max_output":1024,"native_tools":true,"parallel_tools":true,"vision":false,
            "reasoning":null},
        "network":{"allowed":[],"more":0},
        "limits":{"max_steps":10,"wall_clock_secs":300,"tool_result_bytes":65_536,"max_output_tokens":131_072},
        "workspace":{"display":"/workspace"},"protocol":1
    });
    if let Some(host_ca) = host_ca {
        frame["host_ca"] = host_ca;
    }
    frame
}

fn certificate(body: &str) -> String {
    format!("-----BEGIN CERTIFICATE-----\n{body}\n-----END CERTIFICATE-----\n")
}

// TEST: PROTO-U-032
#[test]
fn proto_u_032_session_config_carries_the_install_ca_the_sandbox_trusts() -> TestResult {
    let mut schema = protocol_schema();
    schema["$ref"] = json!("#/definitions/ClientFrame");
    let validator = jsonschema::validator_for(&schema)?;
    let one = certificate("MIIBszCCAVmgAwIBAgIUQ0E=");
    let two = format!("{one}{}", certificate("MIIBtDCCAVqgAwIBAgIUQ0I="));
    let three = format!("{two}{one}");
    let long = certificate(&"QUFB".repeat(4096));
    // A key block in place of a certificate; the kind is spelled apart so no key-shaped text sits in the source.
    let kind = ["PRIVATE", "KEY"].join(" ");
    let key = format!("-----BEGIN {kind}-----\nQUFB\n-----END {kind}-----\n");
    for (host_ca, valid) in [
        (None, true),
        (Some(json!(one)), true),
        (Some(json!(two)), true),
        (Some(json!(three)), false),
        (Some(json!(long)), false),
        (Some(json!("")), false),
        (Some(json!(key)), false),
        (Some(json!(format!("{one}trailing"))), false),
    ] {
        let wire = session(host_ca);
        assert_eq!(
            ClientFrame::parse(&serde_json::to_vec(&wire)?).is_ok(),
            valid,
            "{wire}"
        );
        assert_eq!(validator.is_valid(&wire), valid, "schema: {wire}");
    }
    assert!(
        PROTOCOL_REVISIONS[10]
            .schema
            .contains(&("ClientFrame", "host_ca"))
    );
    Ok(())
}

// TEST: PROTO-U-032
#[test]
fn proto_u_032_session_config_carries_the_keys_of_a_run_in_progress() -> TestResult {
    let mut schema = protocol_schema();
    schema["$ref"] = json!("#/definitions/ClientFrame");
    let validator = jsonschema::validator_for(&schema)?;
    for (run_host_keys, valid) in [
        (None, true),
        (Some(json!([grant()])), true),
        (Some(json!([grant(), grant()])), false),
        (
            Some(json!([with("placeholder", json!("tqa_ph_short"))])),
            false,
        ),
    ] {
        let mut wire = session(None);
        if let Some(keys) = run_host_keys {
            wire["run_host_keys"] = keys;
        }
        assert_eq!(
            ClientFrame::parse(&serde_json::to_vec(&wire)?).is_ok(),
            valid,
            "{wire}"
        );
        assert_eq!(validator.is_valid(&wire), valid, "schema: {wire}");
    }
    let plain = ClientFrame::parse(&serde_json::to_vec(&session(None))?)?;
    assert!(!serde_json::to_string(&plain)?.contains("run_host_keys"));
    assert!(
        PROTOCOL_REVISIONS[10]
            .schema
            .contains(&("ClientFrame", "run_host_keys"))
    );
    Ok(())
}

// TEST: PROTO-U-032
#[test]
fn proto_u_032_the_live_key_frames_carry_run_step_sender_and_a_name_only_when_shown() -> TestResult
{
    let mut schema = protocol_schema();
    schema["$ref"] = json!("#/definitions/ServerFrame");
    let validator = jsonschema::validator_for(&schema)?;
    for (frame, valid) in frame_cases() {
        assert_eq!(
            ServerFrame::parse(&serde_json::to_vec(&frame)?).is_ok(),
            valid,
            "{frame}"
        );
        assert_eq!(validator.is_valid(&frame), valid, "schema: {frame}");
    }
    // Written exactly as the web checks them: step and name always present, no sequence.
    // The use with no step (the table's fourth case).
    let parsed = ServerFrame::parse(&serde_json::to_vec(&frame_cases()[3].0)?)?;
    let written = serde_json::to_value(&parsed)?;
    let mut fields: Vec<&str> = written
        .as_object()
        .ok_or("an object")?
        .keys()
        .map(String::as_str)
        .collect();
    fields.sort_unstable();
    assert_eq!(
        fields,
        [
            "host",
            "name",
            "port",
            "run_id",
            "sender_id",
            "step",
            "type"
        ]
    );
    assert!(
        PROTOCOL_REVISIONS[10]
            .schema
            .contains(&("ServerFrame", "host_key.refused"))
    );
    Ok(())
}

/// Live key frames, and whether each is valid.
fn frame_cases() -> Vec<(Value, bool)> {
    let sender = "01a0f3ee-1b53-7027-adcf-7f974fea1081";
    let used = json!({"type": "host_key.used", "run_id": "run-1", "step": 3, "sender_id": sender,
                      "name": "GITHUB_WORK", "host": "api.github.com", "port": 443});
    let mut refused = used.clone();
    refused["type"] = json!("host_key.refused");
    refused["outcome"] = json!("unavailable");
    let edit = |frame: &Value, field: &str, value: Value| {
        let mut frame = frame.clone();
        frame[field] = value;
        frame
    };
    vec![
        (used.clone(), true),
        (refused.clone(), true),
        (edit(&used, "name", Value::Null), true),
        (edit(&used, "step", Value::Null), true),
        (edit(&refused, "outcome", json!("foreign_run")), true),
        (edit(&refused, "outcome", json!("stolen")), false),
        (edit(&used, "name", json!("github_work")), false),
        (edit(&used, "host", json!("API.github.com")), false),
        (edit(&used, "port", json!(0)), false),
        (edit(&used, "sender_id", json!("someone")), false),
        // A key's address may be an IP literal (Amendment 2), so its notices name one, in the schema as in the parser.
        (edit(&used, "host", json!("2001:db8::34")), true),
        (edit(&refused, "host", json!("2001:db8::34")), true),
        (edit(&used, "host", json!("192.0.2.10")), true),
        (edit(&refused, "host", json!("::ffff:192.0.2.10")), true),
        (edit(&used, "host", json!("2001:DB8::34")), false),
        (edit(&refused, "host", json!("[2001:db8::34]")), false),
    ]
}

// TEST: PROTO-U-033
/// One spelling for one address, as the gateway's proxy compares it. The schema can't say "canonical" (zero compression
/// isn't a regular pattern), so only the parser refuses these; the gateway never sends them.
#[test]
fn proto_u_033_an_address_in_a_grant_is_in_its_canonical_form() -> TestResult {
    for (host, valid) in [
        ("2001:db8::1", true),
        ("::ffff:192.0.2.10", true),
        ("fd00::5", true),
        ("2001:DB8::1", false),
        ("2001:db8:0::1", false),
        ("2001:0db8::1", false),
        ("[2001:db8::1]", false),
    ] {
        let wire = message(Some(json!([with("host", json!(host))])));
        assert_eq!(
            ClientFrame::parse(&serde_json::to_vec(&wire)?).is_ok(),
            valid,
            "{host}"
        );
    }
    Ok(())
}
