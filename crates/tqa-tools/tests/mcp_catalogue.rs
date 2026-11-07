//! TOOL-U-029 and SEC-011: MCP tools enter the registry only through `register_mcp`, and the model sees each tool under a
//! provider-safe name (ADR-0011 decision 4): builtins unchanged, MCP tools `mcp__<server>__<tool>` folded to `[a-z0-9_]`, at
//! most 64 bytes, with a stable suffix on collisions. The table is a bijection for a run's catalogue.
use serde_json::json;
use std::{error::Error, sync::Arc};
use tqa_tools::{
    MAX_MODEL_TOOL_NAME_BYTES, ModelNames, RegistryError, Tool, ToolContext, ToolError, ToolName,
    ToolOutput, ToolRegistry, ToolSpec,
};

type TestResult = Result<(), Box<dyn Error>>;

struct Named(ToolSpec);
#[async_trait::async_trait]
impl Tool for Named {
    fn spec(&self) -> &ToolSpec {
        &self.0
    }
    async fn call(&self, _: serde_json::Value, _: &ToolContext) -> Result<ToolOutput, ToolError> {
        Err(ToolError::Cancelled)
    }
}
fn tool(name: ToolName) -> Arc<dyn Tool> {
    Arc::new(Named(ToolSpec {
        name,
        description: "A tool".into(),
        parameters: json!({"type":"object"}),
        parallel_safe: false,
        changes_workspace: false,
    }))
}
fn mcp(server: &str, name: &str) -> Result<ToolName, ToolError> {
    ToolName::mcp(server, name)
}

#[test]
fn tool_u_029_mcp_tools_register_only_through_register_mcp() -> TestResult {
    let mut registry = ToolRegistry::default();
    assert!(matches!(
        registry.register(tool(mcp("docs", "search")?)),
        Err(RegistryError::ReservedName)
    ));
    registry.register_mcp(tool(mcp("docs", "search")?))?;
    assert!(registry.get("mcp.docs.search").is_some());
    assert!(matches!(
        registry.register_mcp(tool(mcp("docs", "search")?)),
        Err(RegistryError::Duplicate)
    ));
    // A builtin name can never enter through the MCP path.
    assert!(matches!(
        registry.register_mcp(tool(ToolName::new("shell")?)),
        Err(RegistryError::ReservedName)
    ));
    Ok(())
}

#[test]
fn sec_011_an_mcp_tool_named_shell_is_namespaced_and_the_builtin_is_unaffected() -> TestResult {
    let mut registry = ToolRegistry::default();
    registry.register(tool(ToolName::new("shell")?))?;
    registry.register_mcp(tool(mcp("srv", "shell")?))?;
    let names = ModelNames::new(registry.specs().map(|spec| &spec.name));
    assert_eq!(names.model(&ToolName::new("shell")?), Some("shell"));
    assert_eq!(names.model(&mcp("srv", "shell")?), Some("mcp__srv__shell"));
    assert_eq!(names.internal("shell"), Some(&ToolName::new("shell")?));
    assert_eq!(
        names.internal("mcp__srv__shell"),
        Some(&mcp("srv", "shell")?)
    );
    // No builtin may take the model-facing MCP prefix, so none can collide with or shadow an MCP tool.
    assert!(matches!(
        registry.register(tool(ToolName::new("mcp__srv__shell")?)),
        Err(RegistryError::ReservedName)
    ));
    Ok(())
}

#[test]
fn tool_u_029_names_fold_to_provider_safe_text() -> TestResult {
    for (server, tool_name, expected) in [
        ("docs", "search", "mcp__docs__search"),
        ("docs", "Get.Item-v2", "mcp__docs__get_item_v2"),
        ("a_b", "x", "mcp__a_b__x"),
        ("docs", "UPPER", "mcp__docs__upper"),
    ] {
        let name = mcp(server, tool_name)?;
        let names = ModelNames::new([&name]);
        assert_eq!(names.model(&name), Some(expected), "{tool_name}");
    }
    // The longest server and tool: truncated to the provider limit, still a valid provider name.
    let long = mcp("abcdefghijklmnop", &"T".repeat(128))?;
    let names = ModelNames::new([&long]);
    let model = names.model(&long).ok_or("named")?;
    assert_eq!(model.len(), MAX_MODEL_TOOL_NAME_BYTES);
    assert!(model.starts_with("mcp__abcdefghijklmnop__ttt"));
    assert!(
        model
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    );
    Ok(())
}

#[test]
fn tool_u_029_collisions_get_stable_suffixes_in_the_order_of_the_original_names() -> TestResult {
    // All three fold to mcp__docs__get_item; the original names order the suffixes.
    let a = mcp("docs", "Get.Item")?;
    let b = mcp("docs", "get-item")?;
    let c = mcp("docs", "get_item")?;
    let forward = ModelNames::new([&a, &b, &c]);
    let reverse = ModelNames::new([&c, &b, &a]);
    for names in [&forward, &reverse] {
        assert_eq!(names.model(&a), Some("mcp__docs__get_item"));
        assert_eq!(names.model(&b), Some("mcp__docs__get_item_2"));
        assert_eq!(names.model(&c), Some("mcp__docs__get_item_3"));
    }
    // A suffix on a name already at the limit still fits it.
    let long_a = mcp("docs", &format!("{}.A", "x".repeat(70)))?;
    let long_b = mcp("docs", &format!("{}-a", "x".repeat(70)))?;
    let names = ModelNames::new([&long_a, &long_b]);
    let first = names.model(&long_a).ok_or("a")?;
    let second = names.model(&long_b).ok_or("b")?;
    assert_ne!(first, second);
    assert!(first.len() <= MAX_MODEL_TOOL_NAME_BYTES && second.len() <= MAX_MODEL_TOOL_NAME_BYTES);
    // "…x-a" sorts before "…x.A" ('-' is 0x2d, '.' 0x2e), so it keeps the base and the other takes the suffix.
    assert!(first.ends_with("_2"), "{first}");
    assert!(!second.ends_with("_2"), "{second}");
    Ok(())
}

#[test]
fn tool_u_029_the_table_round_trips_every_name_and_refuses_unknown_ones() -> TestResult {
    let catalogue = [
        ToolName::new("shell")?,
        ToolName::new("read_file")?,
        mcp("docs", "search")?,
        mcp("docs", "Search")?,
        mcp("tracker", "issues.create")?,
        mcp("tracker", "issues_create")?,
    ];
    let names = ModelNames::new(catalogue.iter());
    let mut seen = std::collections::BTreeSet::new();
    for name in &catalogue {
        let model = names.model(name).ok_or("named")?;
        assert!(seen.insert(model.to_owned()), "{model} is unique");
        assert_eq!(names.internal(model), Some(name), "{model}");
    }
    // The string forms used on the wire: known names map, anything else passes through unchanged.
    for name in &catalogue {
        let model = names.to_model(name.as_str());
        assert_eq!(names.to_internal(model), name.as_str());
    }
    assert_eq!(names.to_model("unknown_tool"), "unknown_tool");
    assert_eq!(
        names.to_internal("mcp__nobody__search"),
        "mcp__nobody__search"
    );
    assert_eq!(names.internal("mcp__nobody__search"), None);
    assert_eq!(
        names.internal("mcp.docs.search"),
        None,
        "internal names are not model names"
    );
    assert_eq!(names.model(&mcp("other", "x")?), None);
    Ok(())
}

#[tokio::test]
async fn tool_u_029_tool_command_is_public_for_the_runtime() -> TestResult {
    let status = tqa_tools::tool_command("/bin/true", None)?.status().await?;
    assert!(status.success());
    Ok(())
}

/// A name folding to `a_b_c_d`: `case` picks each letter's case, `gaps` each gap from '.', '-' and '_'.
fn alike(case: u32, gaps: u32) -> String {
    let letter = |index: u32, lower: char| {
        if case & (1 << index) == 0 {
            lower
        } else {
            lower.to_ascii_uppercase()
        }
    };
    let gap = |index: u32| ['.', '-', '_'][((gaps / 3_u32.pow(index)) % 3) as usize];
    format!(
        "{}{}{}{}{}{}{}",
        letter(0, 'a'),
        gap(0),
        letter(1, 'b'),
        gap(1),
        letter(2, 'c'),
        gap(2),
        letter(3, 'd')
    )
}

#[test]
fn tool_u_029_a_full_catalogue_of_names_folding_alike_still_gets_distinct_names() -> TestResult {
    // 256 (MAX_TOOLS) names that all fold to "a_b_c_d": every letter in either case, every gap one of '.', '-' or '_'.
    let tools = (0..16_u32)
        .flat_map(|case| (0..27_u32).map(move |gaps| alike(case, gaps)))
        .take(tqa_types::MAX_TOOLS)
        .map(|name| mcp("docs", &name))
        .collect::<Result<Vec<_>, _>>()?;
    let names = ModelNames::new(tools.iter());
    let models: std::collections::BTreeSet<_> =
        tools.iter().filter_map(|tool| names.model(tool)).collect();
    assert_eq!(models.len(), tqa_types::MAX_TOOLS, "every name is distinct");
    assert!(models.contains("mcp__docs__a_b_c_d") && models.contains("mcp__docs__a_b_c_d_256"));
    assert!(models.iter().all(|model| !model.is_empty()));
    Ok(())
}
