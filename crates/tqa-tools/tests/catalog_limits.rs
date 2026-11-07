//! SEC-012: registration rejects excessive metadata and schemas before compilation.
use serde_json::json;
use std::{error::Error, sync::Arc};
use tqa_testkit::FakeTool;
use tqa_tools::{RegistryError, ToolName, ToolRegistry, ToolSpec};
use tqa_types::ToolCatalogLimit;

type TestResult = Result<(), Box<dyn Error>>;

fn tool(index: usize) -> Result<ToolSpec, Box<dyn Error>> {
    Ok(ToolSpec {
        name: ToolName::new(format!("tool_{index}"))?,
        description: "fixture".into(),
        parameters: json!({"type":"object"}),
        parallel_safe: true,
        changes_workspace: false,
    })
}

fn register(registry: &mut ToolRegistry, spec: ToolSpec) -> Result<(), RegistryError> {
    registry.register(Arc::new(FakeTool::new(spec, vec![])))
}

fn limited(result: Result<(), RegistryError>, expected: ToolCatalogLimit) {
    assert!(matches!(result.err(), Some(RegistryError::CatalogLimit(limit)) if limit == expected));
}

#[test]
fn sec_012_registry_refuses_a_hundred_kib_schema() -> TestResult {
    let mut spec = tool(0)?;
    spec.parameters["description"] = json!("x".repeat(100 * 1024));
    let mut registry = ToolRegistry::new();
    let result = register(&mut registry, spec);
    assert!(result.is_err(), "oversize schema was registered");
    limited(result, ToolCatalogLimit::SchemaBytes);
    assert_eq!(registry.specs().count(), 0);
    Ok(())
}

#[test]
fn sec_012_registry_schema_boundary_precedes_compilation_and_leaks_no_values() -> TestResult {
    let mut registry = ToolRegistry::new();
    let mut spec = tool(0)?;
    spec.parameters["description"] = json!("");
    let overhead = serde_json::to_vec(&spec.parameters)?.len();
    spec.parameters["description"] = json!("x".repeat(65536 - overhead));
    register(&mut registry, spec)?;
    let mut next = tool(1)?;
    next.parameters["description"] = json!("x".repeat(65536 - overhead + 1));
    limited(register(&mut registry, next), ToolCatalogLimit::SchemaBytes);
    let mut invalid = tool(1)?;
    invalid.parameters = json!({"type":"invalid", "private-schema":"x".repeat(65536)});
    let error = register(&mut registry, invalid)
        .err()
        .ok_or("oversize input accepted")?;
    assert_eq!(
        error.to_string(),
        "tool parameter schema exceeds 65536 bytes"
    );
    assert!(!format!("{error:?}").contains("private-schema"));
    limited(Err(error), ToolCatalogLimit::SchemaBytes);
    assert_eq!(registry.specs().count(), 1);
    register(&mut registry, tool(1)?)?;
    Ok(())
}

#[test]
fn sec_012_registry_tool_count_accepts_256_and_refuses_257() -> TestResult {
    let mut registry = ToolRegistry::new();
    for index in 0..256 {
        register(&mut registry, tool(index)?)?;
    }
    limited(register(&mut registry, tool(256)?), ToolCatalogLimit::Count);
    assert_eq!(registry.specs().count(), 256);
    assert!(registry.get("tool_256").is_none());
    assert!(registry.prepare("tool_255", &json!({})).is_ok());
    Ok(())
}

#[test]
fn sec_012_registry_catalog_exact_limit_and_rejections_preserve_capacity() -> TestResult {
    let mut spec = tool(0)?;
    spec.description.clear();
    let wire = json!([{"type":"function","function":{"name":"tool_0","description":"",
        "parameters":spec.parameters,"strict":true}}]);
    let padding = 1_048_576 - serde_json::to_vec(&wire)?.len();
    spec.description = "x".repeat(padding + 1);
    let mut registry = ToolRegistry::new();
    limited(
        register(&mut registry, spec.clone()),
        ToolCatalogLimit::CatalogBytes,
    );
    assert_eq!(registry.specs().count(), 0);
    let mut invalid = tool(1)?;
    invalid.parameters = json!({"type":"invalid"});
    assert!(matches!(
        register(&mut registry, invalid),
        Err(RegistryError::InvalidSchema)
    ));
    spec.description.pop();
    register(&mut registry, spec)?;
    limited(
        register(&mut registry, tool(1)?),
        ToolCatalogLimit::CatalogBytes,
    );
    assert_eq!(registry.specs().count(), 1);
    assert!(registry.prepare("tool_0", &json!({})).is_ok());
    Ok(())
}
