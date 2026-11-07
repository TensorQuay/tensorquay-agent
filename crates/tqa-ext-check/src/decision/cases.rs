//! The canned consultations of `ext-check decision` (TK-U-030): deterministic, built in, and free of user data. Each is
//! the run state and, after a response, the model's response that the runtime's own relay client turns into a
//! `decision/v1` request at each exposure level, as it would in a run.
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use tqa_core::{BudgetUse, CallSummary, ModelResponse, RunState, TranscriptLine};
use tqa_llm::ToolCall;
use tqa_tools::{
    Tool, ToolContext, ToolError, ToolName, ToolOutput, ToolRegistry, ToolSpec, builtin,
};
use tqa_types::{DecisionPoint, InvalidId, RunId, ToolStatus, decision_v1};

/// One consultation.
#[derive(Clone, Debug)]
pub struct Case {
    /// Its name in the report.
    pub name: &'static str,
    /// Where the loop asks.
    pub point: DecisionPoint,
    /// The step's state, before the exposure's filter.
    pub state: RunState,
    /// For `after_response`: the model's response, before the exposure's filter.
    pub response: Option<ModelResponse>,
}

/// The MCP tool in the step's tools, so an engine sees a server's tool name as a run shows it.
const MCP_SERVER: &str = "docs";
const MCP_TOOL: &str = "search";

/// A listed MCP tool: its specification only, never called.
struct McpTool(ToolSpec);

#[async_trait]
impl Tool for McpTool {
    fn spec(&self) -> &ToolSpec {
        &self.0
    }
    async fn call(&self, _: Value, _: &ToolContext) -> Result<ToolOutput, ToolError> {
        Err(ToolError::mcp(
            "not_callable",
            "ext-check never runs a tool",
        ))
    }
}

/// The step's tools: the product's own file, search and shell tools, with their real schemas, and one MCP tool.
#[must_use]
pub fn registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    let builtins: [Arc<dyn Tool>; 6] = [
        Arc::new(builtin::ReadFile::new()),
        Arc::new(builtin::WriteFile::new()),
        Arc::new(builtin::EditFile::new()),
        Arc::new(builtin::Glob::new()),
        Arc::new(builtin::Grep::new()),
        Arc::new(builtin::Shell::new()),
    ];
    for tool in builtins {
        let _ = registry.register(tool);
    }
    if let Ok(name) = ToolName::mcp(MCP_SERVER, MCP_TOOL) {
        let _ = registry.register_mcp(Arc::new(McpTool(ToolSpec {
            name,
            description: "Search the project's documentation".into(),
            parameters: json!({"type": "object", "required": ["query"], "additionalProperties": false,
                "properties": {"query": {"type": "string"}}}),
            parallel_safe: true,
            changes_workspace: false,
        })));
    }
    registry
}

/// The routes' facts, as a gateway's route table declares them.
#[must_use]
pub fn routes() -> Vec<decision_v1::Route> {
    [
        ("local", Some(32_768), Some(4_096), true),
        ("cheap", Some(8_192), Some(2_048), true),
        ("long", Some(131_072), Some(8_192), false),
    ]
    .into_iter()
    .map(
        |(name, context_window, max_output, native_tools)| decision_v1::Route {
            name: name.into(),
            context_window,
            max_output,
            native_tools,
        },
    )
    .collect()
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|&name| name.to_owned()).collect()
}

fn state(run_id: &RunId, step: u32, routes: &[&str], tools: Vec<String>) -> RunState {
    RunState {
        run_id: run_id.clone(),
        step,
        attempt: 1,
        route: routes
            .first()
            .map_or_else(String::new, |&route| route.to_owned()),
        routes: names(routes),
        tools,
        budget: BudgetUse {
            steps_used: step - 1,
            max_steps: 30,
            output_tokens_used: u64::from(step - 1) * 350,
            max_output_tokens: 20_000,
        },
        previous_calls: Vec::new(),
        transcript: Some(vec![
            TranscriptLine {
                role: "user".into(),
                text: "Make the failing test in tests/parse.rs pass.".into(),
            },
            TranscriptLine {
                role: "assistant".into(),
                text: "I'll read the test first.".into(),
            },
        ]),
    }
}

fn call(id: &str, name: &str, arguments: &Value) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: name.into(),
        arguments: arguments.to_string(),
    }
}

fn response(text: &str, calls: Vec<ToolCall>) -> ModelResponse {
    ModelResponse {
        text: text.into(),
        tool_calls: calls,
        ..ModelResponse::default()
    }
}

/// Every canned consultation, in the order the suite asks them at each exposure level.
///
/// # Errors
/// Never in practice: the run's id is a fixed, valid identifier.
pub fn cases() -> Result<Vec<Case>, InvalidId> {
    let run: RunId = "ext-check-run".parse()?;
    let tools: Vec<String> = registry()
        .specs()
        .map(|spec| spec.name.as_str().to_owned())
        .collect();
    let mcp = format!("mcp.{MCP_SERVER}.{MCP_TOOL}");
    let mut later = state(&run, 5, &["local", "cheap", "long"], tools.clone());
    later.previous_calls = vec![
        CallSummary {
            name: "read_file".into(),
            status: Some(ToolStatus::Ok),
            arguments: Some(json!({"path": "tests/parse.rs", "offset": null, "limit": null})),
        },
        CallSummary {
            name: "shell".into(),
            status: Some(ToolStatus::Error),
            arguments: Some(json!({"command": "cargo test --test parse"})),
        },
    ];
    let reading = || {
        call(
            "call_1",
            "read_file",
            &json!({"path": "src/parse.rs", "offset": null, "limit": null}),
        )
    };
    Ok(vec![
        Case {
            name: "before_step_first",
            point: DecisionPoint::BeforeStep,
            state: state(&run, 1, &["local", "cheap"], tools.clone()),
            response: None,
        },
        Case {
            name: "before_step_later",
            point: DecisionPoint::BeforeStep,
            state: later.clone(),
            response: None,
        },
        Case {
            name: "before_step_one_route_one_tool",
            point: DecisionPoint::BeforeStep,
            state: state(&run, 2, &["local"], names(&["read_file"])),
            response: None,
        },
        Case {
            name: "after_response_one_call",
            point: DecisionPoint::AfterResponse,
            state: state(&run, 1, &["local", "cheap"], tools.clone()),
            response: Some(response("I'll read the parser.", vec![reading()])),
        },
        Case {
            name: "after_response_three_calls",
            point: DecisionPoint::AfterResponse,
            state: later,
            response: Some(response(
                "Reading the parser, searching for the error and checking the docs.",
                vec![
                    reading(),
                    call(
                        "call_2",
                        "grep",
                        &json!({"pattern": "ParseError", "path": null, "glob": null,
                        "max_results": null, "cursor": null}),
                    ),
                    call("call_3", &mcp, &json!({"query": "number formats"})),
                ],
            )),
        },
        Case {
            name: "after_response_text_only",
            point: DecisionPoint::AfterResponse,
            state: state(&run, 3, &["local", "cheap"], tools),
            response: Some(response("The test passes now.", Vec::new())),
        },
    ])
}
