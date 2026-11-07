//! SEC-012: one encoded catalog budget is shared by registration and model requests.
use serde_json::{Value, json};
use tqa_types::{
    MAX_TOOL_CATALOG_BYTES, MAX_TOOL_SCHEMA_BYTES, MAX_TOOLS, ToolCatalogBudget, ToolCatalogEntry,
    ToolCatalogLimit,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn entry<'a>(description: &'a str, parameters: &'a Value) -> ToolCatalogEntry<'a> {
    ToolCatalogEntry {
        name: "fixture",
        description,
        parameters,
        strict: true,
    }
}

#[test]
fn sec_012_catalog_counts_serialized_utf8_escapes_metadata_and_array_punctuation() -> TestResult {
    let parameters = json!({"description":"é\n\"\\", "type":"object"});
    let description = "é\n\"\\";
    let wire = json!({"type":"function", "function":{"name":"fixture",
        "description":description,"parameters":parameters,"strict":true}});
    let prefix = serde_json::to_vec(&json!([wire.clone(), wire.clone()]))?.len();
    let remaining = MAX_TOOL_CATALOG_BYTES - prefix;
    let padded = format!("{description}{}", "x".repeat(remaining));
    let first = ToolCatalogBudget::default().include(entry(description, &parameters))?;
    assert!(first.include(entry(&padded, &parameters)).is_ok());
    assert_eq!(
        first.include(entry(&(padded + "x"), &parameters)).err(),
        Some(ToolCatalogLimit::CatalogBytes)
    );
    assert!(
        first.include(entry(description, &parameters)).is_ok(),
        "rejection consumed budget"
    );
    Ok(())
}

#[test]
fn sec_012_catalog_schema_and_count_limits_are_inclusive() -> TestResult {
    assert_eq!(
        (MAX_TOOLS, MAX_TOOL_SCHEMA_BYTES, MAX_TOOL_CATALOG_BYTES),
        (256, 65_536, 1_048_576)
    );
    let mut schema = json!({"description":""});
    let overhead = serde_json::to_vec(&schema)?.len();
    schema["description"] = json!("x".repeat(MAX_TOOL_SCHEMA_BYTES - overhead));
    assert!(
        ToolCatalogBudget::default()
            .include(entry("", &schema))
            .is_ok()
    );
    schema["description"] = json!("x".repeat(MAX_TOOL_SCHEMA_BYTES - overhead + 1));
    assert_eq!(
        ToolCatalogBudget::default()
            .include(entry("", &schema))
            .err(),
        Some(ToolCatalogLimit::SchemaBytes)
    );
    let small = json!({});
    let mut budget = ToolCatalogBudget::default();
    for _ in 0..MAX_TOOLS {
        budget = budget.include(entry("", &small))?;
    }
    assert_eq!(
        budget.include(entry("", &schema)).err(),
        Some(ToolCatalogLimit::Count)
    );
    Ok(())
}
