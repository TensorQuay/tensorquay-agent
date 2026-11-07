//! RT-I-013/PROTO-C-001: environment wire and schema boundaries share one contract.
use serde_json::{Value, json};
use tqa_types::{
    Capability, ClientFrame, Environment, ErrorCode, Event, ServerFrame, protocol_schema,
};
type TestResult = Result<(), Box<dyn std::error::Error>>;

fn configuration(environment: &Value) -> Value {
    json!({"type":"session.config","conversation_id":"c","last_stored_seq":0,
        "route":{"name":"local","protocol":"openai_chat","model":"model",
            "relay_path":"/relay/r/local","context_window":32768,"max_output":4096,
            "native_tools":true,"parallel_tools":true,"vision":false,"reasoning":null},
        "network":{"allowed":[],"more":0},"limits":{"max_steps":10,
            "wall_clock_secs":300,"tool_result_bytes":8192},
        "workspace":{"display":"/workspace"},"protocol":1,"environment":environment})
}

fn environment() -> Value {
    json!({"template":"workstation","digest":format!("sha256:{}", "a".repeat(64)),
        "capabilities":["shell","python","node","build","browser","data"]})
}

fn validate_both(value: &Value, valid: bool) -> TestResult {
    let mut schema = protocol_schema();
    schema["$ref"] = json!("#/definitions/ClientFrame");
    let validator = jsonschema::validator_for(&schema)?;
    let value = configuration(value);
    assert_eq!(
        ClientFrame::parse(&serde_json::to_vec(&value)?).is_ok(),
        valid,
        "{value}"
    );
    assert_eq!(validator.is_valid(&value), valid, "schema: {value}");
    Ok(())
}

#[test]
fn proto_c_001_environment_bounds_match_the_schema() -> TestResult {
    for (template, valid) in [
        ("a".into(), true),
        ("a".repeat(64), true),
        ("a".repeat(65), false),
        ("custom_2-small".into(), true),
        (String::new(), false),
        ("Upper".into(), false),
        ("2small".into(), false),
        ("a b".into(), false),
        ("界".into(), false),
        ("workstation\n".into(), false),
    ] {
        let mut value = environment();
        value["template"] = json!(template);
        validate_both(&value, valid)?;
    }
    for (capabilities, valid) in [
        (json!([]), true),
        (json!(["shell"]), true),
        (json!(["browser", "build", "node", "python", "shell"]), true),
        (json!(["data"]), true),
        (
            json!(["data", "browser", "build", "node", "python", "shell"]),
            true,
        ),
        // Revision 14: the Desktop, with all seven.
        (json!(["desktop"]), true),
        (
            json!([
                "shell", "python", "node", "build", "browser", "data", "desktop"
            ]),
            true,
        ),
        (json!(["shell", "shell"]), false),
        (json!(["gpu"]), false),
        (json!(["unknown"]), false),
        (json!(["Shell"]), false),
        (json!(null), false),
        (
            json!([
                "shell", "python", "node", "build", "browser", "data", "desktop", "shell"
            ]),
            false,
        ),
    ] {
        let mut value = environment();
        value["capabilities"] = capabilities;
        validate_both(&value, valid)?;
    }
    Ok(())
}

#[test]
fn proto_c_001_environment_digest_is_bounded_pinned_and_secret_free() -> TestResult {
    let digest = format!("sha256:{}", "a".repeat(64));
    for (pin, valid) in [
        (digest.clone(), true),
        (format!("registry.example:5000/team/image@{digest}"), true),
        (format!("{}@{digest}", "a".repeat(440)), true),
        (format!("{}@{digest}", "a".repeat(441)), false),
        (format!("sha256:{}", "a".repeat(63)), false),
        (format!("sha256:{}", "A".repeat(64)), false),
        (format!("sha256:{}", "g".repeat(64)), false),
        (format!("{digest}\n"), false),
        (format!("@{digest}"), false),
        (format!("https://registry.example/image@{digest}"), false),
        (
            format!("user:password@registry.example/image@{digest}"),
            false,
        ),
        (format!("registry.example/界@{digest}"), false),
        ("image:latest".into(), false),
        (String::new(), false),
    ] {
        let mut value = environment();
        value["digest"] = json!(pin);
        validate_both(&value, valid)?;
    }
    for (capabilities, valid) in [(json!(["shell"]), true), (json!([]), false)] {
        validate_both(
            &json!({"template":"minimal","digest":"","capabilities":capabilities}),
            valid,
        )?;
    }
    let mut unknown = environment();
    unknown["credentials"] = json!("forbidden");
    validate_both(&unknown, false)?;
    Ok(())
}

#[test]
fn proto_c_001_environment_changed_is_canonical_additive_and_validated() -> TestResult {
    let value = json!({"type":"environment.changed","template":"custom",
        "capabilities":["browser","shell","node"],"future_field":true});
    let event: Event = serde_json::from_value(value)?;
    assert_eq!(
        serde_json::to_value(event)?,
        json!({"type":"environment.changed",
        "template":"custom","capabilities":["shell","node","browser"]})
    );
    for (template, capabilities) in [
        ("Invalid", json!([])),
        ("custom", json!(["shell", "shell"])),
    ] {
        let frame = json!({"type":"event","seq":1,"run_id":null,"step":null,
            "ts":"2000-01-01T00:00:00Z","event":{"type":"environment.changed",
                "template":template,"capabilities":capabilities}});
        assert_eq!(
            ServerFrame::parse(&serde_json::to_vec(&frame)?)
                .err()
                .ok_or("invalid environment accepted")?
                .code,
            ErrorCode::InvalidFrame
        );
    }
    Ok(())
}

#[test]
fn proto_c_001_environment_registry_constructor_and_legacy_default_match_parsing() -> TestResult {
    let digest = format!("sha256:{}", "a".repeat(64));
    let canonical = Environment::new(
        "custom".into(),
        digest,
        vec![Capability::Browser, Capability::Shell],
    )?;
    assert_eq!(
        canonical.capabilities,
        [Capability::Shell, Capability::Browser]
    );
    let mut duplicate = canonical.clone();
    duplicate.capabilities.push(Capability::Shell);
    assert!(duplicate.validate().is_err());
    let mut invalid = canonical;
    invalid.template = "Invalid".into();
    assert_eq!(
        invalid.validate().err().ok_or("invalid accepted")?.path,
        "environment.template"
    );
    let legacy = Environment::default();
    assert!(legacy.is_legacy());
    legacy.validate()?;
    let mut old = configuration(&json!(null));
    old.as_object_mut()
        .ok_or("configuration object")?
        .remove("environment");
    let parsed = ClientFrame::parse(&serde_json::to_vec(&old)?)?;
    assert!(
        matches!(&parsed, ClientFrame::SessionConfig { environment, .. } if *environment == legacy)
    );
    assert!(serde_json::to_value(parsed)?.get("environment").is_none());
    Ok(())
}
