//! PROTO-C-001 / PROTO-U-003: bounded network targets on additive error frames.
use serde_json::{Value, json};
use std::error::Error;
use tqa_types::{ErrorCode, ServerFrame, protocol_schema};

type TestResult = Result<(), Box<dyn Error>>;

fn frame(target: Value) -> Value {
    let mut wire = json!({"type":"error", "seq":null, "code":"io", "message":"Request failed",
        "retryable":false});
    wire["target"] = target;
    wire
}

fn invalid_targets() -> Vec<Value> {
    let mut targets = vec![json!({}), json!(false), json!([]), json!("fixture.example")];
    for host in [
        String::new(),
        format!(
            "{}.{}.{}.{}",
            "a".repeat(63),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(62)
        ),
        format!("{}.example", "a".repeat(64)),
        "bad host.example".into(),
        "bad_host.example".into(),
        "-bad.example".into(),
        "bad-.example".into(),
        "bad..example".into(),
        "https://fixture.example/path".into(),
        "user@fixture.example".into(),
        "fixture.example:443".into(),
        "fixture.example\n".into(),
        "bücher.example".into(),
        "[2001:db8::1]".into(),
        "fe80::1%zone".into(),
        "2001:db8::xyz".into(),
    ] {
        targets.push(json!({"host":host,"port":443}));
    }
    for port in [
        json!(0),
        json!(65536),
        json!(-1),
        json!(1.5),
        json!("443"),
        Value::Null,
    ] {
        targets.push(json!({"host":"fixture.example","port":port}));
    }
    targets.extend([
        json!({"host":"fixture.example"}),
        json!({"port":443}),
        json!({"host":42,"port":443}),
    ]);
    targets
}

#[test]
fn proto_c_001_egress_denied_has_a_stable_nonretryable_code() -> TestResult {
    let code: ErrorCode = serde_json::from_value(json!("egress_denied"))?;
    assert_eq!(code.as_str(), "egress_denied");
    assert_eq!(code.to_string(), "egress_denied");
    assert!(!code.retryable());
    assert_eq!(serde_json::to_value(code)?, "egress_denied");
    let mut wire = frame(json!({"host":"fixture.example","port":443}));
    wire["code"] = json!("egress_denied");
    assert_eq!(
        serde_json::to_value(ServerFrame::parse(&serde_json::to_vec(&wire)?)?)?,
        wire
    );
    Ok(())
}

#[test]
fn proto_c_001_error_target_round_trips_host_and_port_boundaries() -> TestResult {
    let longest = format!(
        "{}.{}.{}.{}",
        "a".repeat(63),
        "b".repeat(63),
        "c".repeat(63),
        "d".repeat(61)
    );
    assert_eq!(longest.len(), 253);
    // The generated schema the web validates with takes every host the parser does, an address too.
    let mut schema = protocol_schema();
    schema["$ref"] = json!("#/definitions/ServerFrame");
    let validator = jsonschema::validator_for(&schema)?;
    for host in [
        "fixture.example",
        "Fixture.Example.",
        "a",
        "xn--bcher-kva.example",
        "192.0.2.1",
        "2001:db8::1",
        "::",
        &longest,
    ] {
        for port in [1, 443, 65535] {
            let wire = frame(json!({"host":host,"port":port}));
            let parsed = ServerFrame::parse(&serde_json::to_vec(&wire)?)?;
            assert_eq!(serde_json::to_value(parsed)?, wire);
            assert!(validator.is_valid(&wire), "schema: {wire}");
        }
    }
    Ok(())
}

#[test]
fn proto_u_003_error_target_constructor_preserves_validated_values() -> TestResult {
    let target = tqa_types::ErrorTarget::new("Fixture.Example.", 65535)?;
    assert_eq!(target.host(), "Fixture.Example.");
    assert_eq!(target.port(), 65535);
    assert_eq!(
        serde_json::to_value(&target)?,
        json!({"host":"Fixture.Example.","port":65535})
    );
    let host_error = tqa_types::ErrorTarget::new("bad host", 443)
        .err()
        .ok_or("accepted invalid host")?;
    assert_eq!(host_error.code, ErrorCode::InvalidFrame);
    assert_eq!(host_error.path, "target.host");
    let port_error = tqa_types::ErrorTarget::new("fixture.example", 0)
        .err()
        .ok_or("accepted zero port")?;
    assert_eq!(port_error.code, ErrorCode::InvalidFrame);
    assert_eq!(port_error.path, "target.port");
    Ok(())
}

#[test]
fn proto_u_003_error_targets_reject_invalid_hosts_ports_and_shapes() -> TestResult {
    for target in invalid_targets() {
        let wire = frame(target);
        assert!(
            ServerFrame::parse(&serde_json::to_vec(&wire)?).is_err(),
            "accepted {wire}"
        );
        assert!(
            serde_json::from_value::<ServerFrame>(wire.clone()).is_err(),
            "accepted {wire}"
        );
    }
    Ok(())
}

#[test]
fn proto_c_001_legacy_errors_omit_absent_target_and_ignore_future_fields() -> TestResult {
    let mut old = frame(Value::Null);
    old.as_object_mut().ok_or("not an object")?.remove("target");
    assert_eq!(
        serde_json::to_value(ServerFrame::parse(&serde_json::to_vec(&old)?)?)?,
        old
    );
    let explicit_null = frame(Value::Null);
    assert_eq!(
        serde_json::to_value(ServerFrame::parse(&serde_json::to_vec(&explicit_null)?)?)?,
        old
    );
    let expected = frame(json!({"host":"fixture.example","port":443}));
    let mut future = expected.clone();
    future["future_frame_field"] = json!(true);
    future["target"]["future_target_field"] = json!(true);
    assert_eq!(
        serde_json::to_value(ServerFrame::parse(&serde_json::to_vec(&future)?)?)?,
        expected
    );
    Ok(())
}

#[test]
fn proto_u_003_error_target_schema_enforces_the_wire_bounds() -> TestResult {
    let mut schema = tqa_types::protocol_schema();
    schema["$ref"] = json!("#/definitions/ServerFrame");
    let validator = jsonschema::options()
        .should_validate_formats(true)
        .build(&schema)?;
    assert!(validator.is_valid(&frame(json!({"host":"fixture.example","port":443}))));
    for target in invalid_targets() {
        let wire = frame(target);
        assert!(!validator.is_valid(&wire), "schema accepted {wire}");
    }
    Ok(())
}
