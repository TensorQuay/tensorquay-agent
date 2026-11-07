//! CORE-U-051: each tool goes out with `strict: true` only when its schema meets the strict rules (`strict_compatible`,
//! LLM-U-034), otherwise with `strict: false`, so a strict endpoint never refuses a request over one tool's schema.
mod support;
use std::{error::Error, sync::Arc};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_testkit::FakeTool;
use tqa_tools::{ToolName, ToolRegistry, ToolSpec};

/// An MCP tool whose schema, as third-party schemas often are, has an optional property and no closed object.
fn open_mcp_tool() -> Result<Arc<FakeTool>, Box<dyn Error>> {
    Ok(Arc::new(FakeTool::new(
        ToolSpec {
            name: ToolName::mcp("docs", "search")?,
            description: "Search the docs".into(),
            parameters: serde_json::json!({"type":"object","properties":{"q":{"type":"string"},"limit":{"type":"integer"}},"required":["q"]}),
            parallel_safe: true,
            changes_workspace: false,
        },
        vec![],
    )))
}

#[tokio::test]
async fn core_u_051_only_compatible_schemas_go_out_strict() -> TestResult {
    // The support tool's schema is closed with its one property required: compatible.
    let mut registry = ToolRegistry::new();
    registry.register(fake("read_file", true, vec![])?)?;
    registry.register_mcp(open_mcp_tool()?)?;
    let h = Harness::new(vec![answer()], registry)?;
    let provider = h.provider.clone();
    let outcome = Runner::new(h.deps)?
        .run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert_eq!(outcome, RunOutcome::Completed);
    let requests = provider.requests();
    let mut flags: Vec<(String, bool)> = requests[0]
        .tools
        .iter()
        .map(|tool| (tool.name.clone(), tool.strict))
        .collect();
    flags.sort();
    assert_eq!(
        flags,
        vec![
            ("mcp__docs__search".to_owned(), false),
            ("read_file".to_owned(), true),
        ]
    );
    Ok(())
}

/// A builtin-style tool: every property required, the optional one nullable.
fn nullable_tool() -> Result<Arc<FakeTool>, Box<dyn Error>> {
    Ok(Arc::new(FakeTool::new(
        ToolSpec {
            name: ToolName::new("read_file")?,
            description: "Read".into(),
            parameters: serde_json::json!({"type":"object","required":["path","limit"],"additionalProperties":false,
                "properties":{"path":{"type":"string"},"limit":{"type":["integer","null"]}}}),
            parallel_safe: true,
            changes_workspace: false,
        },
        vec![output("text", 0)],
    )))
}

#[tokio::test]
async fn tool_u_032_a_call_that_omits_a_nullable_argument_is_recorded_as_sent() -> TestResult {
    // A model on a route that doesn't enforce the schema leaves `limit` out: the run records the call with it null,
    // and the tool gets exactly what was recorded (the same arguments the approval hash would bind).
    let tool = nullable_tool()?;
    let mut registry = ToolRegistry::new();
    registry.register(tool.clone())?;
    let h = Harness::new(
        vec![
            calls(&[("call_1", "read_file", r#"{"path":"a.txt"}"#)]),
            answer(),
        ],
        registry,
    )?;
    let (provider, sink) = (h.provider.clone(), h.sink.clone());
    let outcome = Runner::new(h.deps)?
        .run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert_eq!(outcome, RunOutcome::Completed);
    // Recorded and dispatched as the model meant it: the omitted optional argument stays absent (CORE-U-056).
    let sent = serde_json::json!({"path": "a.txt"});
    let recorded: Vec<serde_json::Value> = sink
        .events()
        .into_iter()
        .filter_map(|event| match event {
            tqa_types::Event::ToolCall { arguments, .. } => Some(arguments),
            _ => None,
        })
        .collect();
    assert_eq!(recorded, vec![sent.clone()]);
    assert_eq!(tool.calls(), vec![sent]);
    assert!(
        provider.requests()[0].tools[0].strict,
        "a strict-valid schema goes out strict"
    );
    Ok(())
}
