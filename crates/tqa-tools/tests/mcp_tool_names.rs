//! TOOL-U-028: MCP tool names. `ToolName::new` stays builtin-only; `ToolName::mcp` builds `mcp.<server>.<tool>`, and
//! `ToolName::parse` reads a stored name of either form; the builtin registry still refuses the namespace.
use serde_json::json;
use std::error::Error;
use tqa_tools::{ToolName, ToolSpec};

type TestResult = Result<(), Box<dyn Error>>;

fn spec(name: ToolName) -> ToolSpec {
    ToolSpec {
        name,
        description: "An MCP tool".into(),
        parameters: json!({"type":"object"}),
        parallel_safe: false,
        changes_workspace: false,
    }
}

#[test]
fn tool_u_028_new_is_builtin_only_and_mcp_builds_the_namespaced_name() -> TestResult {
    assert!(ToolName::new("mcp.docs.search").is_err());
    let name = ToolName::mcp("tracker", "issues.create")?;
    assert_eq!(name.as_str(), "mcp.tracker.issues.create");
    assert_eq!(name.mcp_parts(), Some(("tracker", "issues.create")));
    assert_eq!(ToolName::new("shell")?.mcp_parts(), None);
    for (server, tool) in [
        ("Docs", "search"),
        ("a__b", "search"),
        ("a_", "search"),
        ("abcdefghijklmnopq", "search"),
        ("docs", ""),
        ("docs", "a b"),
        ("docs", "é"),
        ("docs.x", "search"),
    ] {
        assert!(ToolName::mcp(server, tool).is_err(), "{server:?} {tool:?}");
    }
    assert!(ToolName::mcp("docs", &"t".repeat(128)).is_ok());
    assert!(ToolName::mcp("docs", &"t".repeat(129)).is_err());
    Ok(())
}

#[test]
fn tool_u_028_parse_reads_either_form_and_refuses_everything_else() -> TestResult {
    for name in [
        "shell",
        "read_file",
        "mcp.docs.search",
        "mcp.tracker.issues.create",
    ] {
        assert_eq!(ToolName::parse(name)?.as_str(), name);
    }
    assert_eq!(
        ToolName::parse("mcp.docs.search")?,
        ToolName::mcp("docs", "search")?
    );
    for name in ["", "Shell", "mcp.docs", "mcp.Docs.x", "mcp.docs.a b", "a-b"] {
        assert!(ToolName::parse(name).is_err(), "{name:?}");
    }
    Ok(())
}

#[test]
fn tool_u_028_the_builtin_registry_still_refuses_the_mcp_namespace() -> TestResult {
    use std::sync::Arc;
    struct Named(ToolSpec);
    #[async_trait::async_trait]
    impl tqa_tools::Tool for Named {
        fn spec(&self) -> &ToolSpec {
            &self.0
        }
        async fn call(
            &self,
            _: serde_json::Value,
            _: &tqa_tools::ToolContext,
        ) -> Result<tqa_tools::ToolOutput, tqa_tools::ToolError> {
            Err(tqa_tools::ToolError::Cancelled)
        }
    }
    let mut registry = tqa_tools::ToolRegistry::default();
    let refused = registry.register(Arc::new(Named(spec(ToolName::mcp("docs", "search")?))));
    assert!(matches!(
        refused,
        Err(tqa_tools::RegistryError::ReservedName)
    ));
    Ok(())
}
