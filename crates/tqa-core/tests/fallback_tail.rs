//! CORE-U-022 edge: on a text-protocol route, prose after the call block is held back while streaming and still
//! reaches the transcript once the reply is parsed; the call runs as usual.
mod support;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_llm::{FinishReason, StreamChunk};
use tqa_types::Event;

const REPLY: &str = "Reading it first.\n```tool_call\n{\"name\": \"read_file\", \"arguments\": {\"path\": \"a.rs\"}}\n```\nThen I will summarise it.";

#[tokio::test]
async fn core_u_022_prose_after_the_call_block_reaches_the_transcript() -> TestResult {
    let tool = fake("read_file", true, vec![output("fn main() {}", 0)])?;
    let first = reply(vec![
        StreamChunk::TextDelta(REPLY.into()),
        usage(),
        StreamChunk::Finish(FinishReason::Stop),
    ]);
    let mut h = Harness::new(
        vec![first, answer()],
        registry(std::slice::from_ref(&tool))?,
    )?;
    h.deps.route.native_tools = false;
    h.deps.route.parallel_tools = false;
    let sink = h.sink.clone();
    let outcome = Runner::new(h.deps)?
        .run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert_eq!(outcome, RunOutcome::Completed);
    assert_eq!(tool.calls().len(), 1, "the call in the block runs");
    let streamed: String = sink
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(streamed.contains("Reading it first."), "{streamed}");
    assert!(
        streamed.contains("Then I will summarise it."),
        "the trailing prose is emitted: {streamed}"
    );
    assert!(
        !streamed.contains("tool_call"),
        "the call block itself is not prose: {streamed}"
    );
    Ok(())
}
