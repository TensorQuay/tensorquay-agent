//! Additive run budgets preserve older configuration while rejecting invalid explicit values.
use serde_json::json;
use tqa_types::{Event, RunLimits, protocol_schema};
type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn proto_u_003_output_budget_default_bounds_and_schema_agree() -> TestResult {
    let old = json!({"max_steps":10,"wall_clock_secs":300,"tool_result_bytes":8192});
    let limits: RunLimits = serde_json::from_value(old.clone())?;
    assert_eq!(serde_json::to_value(limits)?["max_output_tokens"], 131_072);
    let mut schema = protocol_schema();
    schema["$ref"] = json!("#/definitions/RunLimits");
    let validator = jsonschema::validator_for(&schema)?;
    assert!(validator.is_valid(&old));
    for tokens in [0, 1, 16_777_216, 16_777_217, u64::MAX] {
        let programmatic = RunLimits {
            max_output_tokens: tokens,
            ..limits
        };
        assert_eq!(
            programmatic.validate().is_ok(),
            (1..=16_777_216).contains(&tokens)
        );
    }
    for tokens in [
        json!(0),
        json!(1),
        json!(131_072),
        json!(16_777_216),
        json!(16_777_217),
        json!(u64::MAX),
        json!(-1),
        json!(1.5),
        json!(null),
    ] {
        let valid = tokens
            .as_u64()
            .is_some_and(|n| (1..=16_777_216).contains(&n));
        let mut value = old.clone();
        value["max_output_tokens"] = tokens.clone();
        assert_eq!(
            serde_json::from_value::<RunLimits>(value.clone()).is_ok(),
            valid,
            "{tokens}"
        );
        assert_eq!(validator.is_valid(&value), valid, "schema {tokens}");
    }
    Ok(())
}

#[test]
fn proto_c_001_new_budget_warnings_and_estimated_usage_round_trip() -> TestResult {
    let mut schema = protocol_schema();
    schema["$ref"] = json!("#/definitions/Event");
    let validator = jsonschema::validator_for(&schema)?;
    for value in [
        json!({"type":"budget.warning","budget":"output_tokens","used":8,"limit":10}),
        json!({"type":"budget.warning","budget":"no_progress","used":6,"limit":12}),
        json!({"type":"usage","prompt_tokens":0,"completion_tokens":5,
            "cached_tokens":null,"reasoning_tokens":null,"estimated":true}),
        json!({"type":"usage","prompt_tokens":2,"completion_tokens":5,
            "cached_tokens":null,"reasoning_tokens":null}),
    ] {
        let event: Event = serde_json::from_value(value.clone())?;
        assert_eq!(serde_json::to_value(event)?, value);
        assert!(validator.is_valid(&value));
    }
    Ok(())
}
