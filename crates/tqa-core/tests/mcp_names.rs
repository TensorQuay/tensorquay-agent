//! CORE-U-050: the model sees MCP tools as `mcp__<server>__<tool>` and the journal keeps `mcp.<server>.<tool>` (ADR-0011
//! decision 4). Outgoing tool lists and replayed calls use the model-facing name; a returned call is mapped back before it is
//! journalled or dispatched, on native and text-protocol routes. The table depends only on the run's catalogue, so a resume
//! rebuilds the same names.
mod support;
use std::{error::Error, sync::Arc};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_llm::{CompletionRequest, FinishReason, StreamChunk};
use tqa_testkit::{FakeTool, ProviderReply};
use tqa_tools::{ToolName, ToolRegistry, ToolSpec};
use tqa_types::{Event, ServerFrame, ToolStatus};

const MODEL_NAME: &str = "mcp__docs__search";
const INTERNAL_NAME: &str = "mcp.docs.search";

fn mcp_tool() -> Result<Arc<FakeTool>, Box<dyn Error>> {
    Ok(Arc::new(FakeTool::new(
        ToolSpec {
            name: ToolName::mcp("docs", "search")?,
            description: "Search the docs".into(),
            parameters: serde_json::json!({"type":"object","properties":{"q":{"type":"string"}}}),
            parallel_safe: true,
            changes_workspace: false,
        },
        vec![output("found it", 0)],
    )))
}
/// A registry with a builtin and the MCP tool, registered in either order.
fn catalogue(mcp: &Arc<FakeTool>, mcp_first: bool) -> Result<ToolRegistry, Box<dyn Error>> {
    let builtin = fake("read_file", true, vec![output("text", 0)])?;
    let mut registry = ToolRegistry::new();
    if mcp_first {
        registry.register_mcp(mcp.clone())?;
        registry.register(builtin)?;
    } else {
        registry.register(builtin)?;
        registry.register_mcp(mcp.clone())?;
    }
    Ok(registry)
}
/// A text-protocol reply streamed in small pieces.
fn text_reply(content: &str) -> ProviderReply {
    let mut chunks: Vec<StreamChunk> = content
        .as_bytes()
        .chunks(7)
        .map(|piece| StreamChunk::TextDelta(String::from_utf8_lossy(piece).into_owned()))
        .collect();
    chunks.extend([usage(), StreamChunk::Finish(FinishReason::Stop)]);
    reply(chunks)
}
fn tool_names(request: &CompletionRequest) -> Vec<String> {
    request.tools.iter().map(|tool| tool.name.clone()).collect()
}
fn replayed_calls(request: &CompletionRequest) -> Vec<String> {
    request
        .messages
        .iter()
        .flat_map(|message| message.tool_calls.iter().map(|call| call.name.clone()))
        .collect()
}
fn journalled_calls(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::ToolCall { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect()
}
async fn run(h: Harness, history: &[ServerFrame]) -> Result<RunOutcome, Box<dyn Error>> {
    Ok(Runner::new(h.deps)?
        .run(
            history,
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?)
}

#[tokio::test]
async fn core_u_050_native_calls_use_model_names_on_the_wire_and_internal_names_in_the_journal()
-> TestResult {
    let tool = mcp_tool()?;
    let h = Harness::new(
        vec![calls(&[("a", MODEL_NAME, r#"{"q":"x"}"#)]), answer()],
        catalogue(&tool, false)?,
    )?;
    let (provider, sink) = (h.provider.clone(), h.sink.clone());
    assert_eq!(run(h, &[]).await?, RunOutcome::Completed);
    let requests = provider.requests();
    let names = tool_names(&requests[0]);
    assert!(names.contains(&MODEL_NAME.to_owned()), "{names:?}");
    assert!(!names.iter().any(|name| name.contains('.')), "{names:?}");
    assert!(
        names.contains(&"read_file".to_owned()),
        "builtins keep their names"
    );
    assert_eq!(journalled_calls(&sink.events()), [INTERNAL_NAME]);
    assert_eq!(tool.calls().len(), 1, "the call reached the MCP tool");
    assert_eq!(
        replayed_calls(&requests[1]),
        [MODEL_NAME],
        "history replays the model name"
    );
    Ok(())
}

#[tokio::test]
async fn core_u_050_text_protocol_calls_are_mapped_the_same_way() -> TestResult {
    let tool = mcp_tool()?;
    let block = tqa_prompt::render_call(MODEL_NAME, &serde_json::json!({"q":"x"}));
    let mut h = Harness::new(
        vec![text_reply(&format!("Searching.\n{block}\n")), answer()],
        catalogue(&tool, false)?,
    )?;
    h.deps.route.native_tools = false;
    h.deps.route.parallel_tools = false;
    let (provider, sink) = (h.provider.clone(), h.sink.clone());
    assert_eq!(run(h, &[]).await?, RunOutcome::Completed);
    assert_eq!(journalled_calls(&sink.events()), [INTERNAL_NAME]);
    assert_eq!(tool.calls().len(), 1);
    let rendered: String = provider.requests()[1]
        .messages
        .iter()
        .flat_map(|message| &message.content)
        .filter_map(|part| match part {
            tqa_llm::ContentPart::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        rendered.contains(MODEL_NAME),
        "the instructions and replay name the model name"
    );
    assert!(
        !rendered.contains(INTERNAL_NAME),
        "the internal name never reaches the model"
    );
    Ok(())
}

#[tokio::test]
async fn core_u_050_an_unknown_model_name_is_journalled_as_given_and_refused() -> TestResult {
    let tool = mcp_tool()?;
    let h = Harness::new(
        vec![calls(&[("a", "mcp__nobody__search", "{}")]), answer()],
        catalogue(&tool, false)?,
    )?;
    let sink = h.sink.clone();
    run(h, &[]).await?;
    assert_eq!(journalled_calls(&sink.events()), ["mcp__nobody__search"]);
    let results = tool_results(&sink.events());
    assert_eq!(results[0].1, ToolStatus::Error, "{results:?}");
    assert!(tool.calls().is_empty());
    Ok(())
}

#[tokio::test]
async fn core_u_050_a_resume_rebuilds_the_same_names_from_the_catalogue_alone() -> TestResult {
    // The first run, with the MCP tool registered after the builtin.
    let tool = mcp_tool()?;
    let first = Harness::new(
        vec![calls(&[("a", MODEL_NAME, r#"{"q":"x"}"#)]), answer()],
        catalogue(&tool, false)?,
    )?;
    let first_provider = first.provider.clone();
    let history_sink = first.sink.clone();
    run(first, &[]).await?;
    let history = history_sink.frames();
    // The resumed run registers the same catalogue in the other order; its replay must name the call identically.
    let resumed_tool = mcp_tool()?;
    let mut resumed = Harness::new(vec![answer()], catalogue(&resumed_tool, true)?)?;
    resumed.sink_with(MemorySink::new(&history));
    let resumed_provider = resumed.provider.clone();
    run(resumed, &history).await?;
    let replayed = replayed_calls(&resumed_provider.requests()[0]);
    assert_eq!(replayed, [MODEL_NAME]);
    assert_eq!(
        tool_names(&resumed_provider.requests()[0]),
        tool_names(&first_provider.requests()[0])
    );
    Ok(())
}
