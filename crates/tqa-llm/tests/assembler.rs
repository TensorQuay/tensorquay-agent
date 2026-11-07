//! LLM-U-011/LLM-C-002: shared bounded assembly preserves malformed raw arguments for the loop.
use std::error::Error;
use tqa_llm::{FinishReason, StreamAssembler, StreamChunk, StreamLimits};

#[test]
fn llm_u_011_assembler_preserves_raw_arguments_in_index_order() -> Result<(), Box<dyn Error>> {
    let mut assembler = StreamAssembler::new(StreamLimits::RECOMMENDED);
    for (index, id, arguments) in [(9, "last", "{"), (2, "first", "[]")] {
        assembler.push(&StreamChunk::ToolCallDelta {
            index,
            id: Some(id.into()),
            name: Some("read_file".into()),
            arguments_delta: arguments.into(),
        })?;
    }
    assert_eq!(assembler.snapshot().tool_calls[0].id, "first");
    assembler.push(&StreamChunk::Finish(FinishReason::ToolCalls))?;
    let completion = assembler.finish()?;
    assert_eq!(completion.tool_calls[0].arguments, "[]");
    assert_eq!(completion.tool_calls[1].arguments, "{");
    Ok(())
}

#[test]
fn llm_c_002_assembler_still_requires_terminal_reason_and_call_identity()
-> Result<(), Box<dyn Error>> {
    let mut assembler = StreamAssembler::new(StreamLimits::RECOMMENDED);
    assembler.push(&StreamChunk::TextDelta("partial".into()))?;
    assert!(assembler.finish().is_err());
    let mut assembler = StreamAssembler::new(StreamLimits::RECOMMENDED);
    assembler.push(&StreamChunk::ToolCallDelta {
        index: 0,
        id: None,
        name: None,
        arguments_delta: "{}".into(),
    })?;
    assembler.push(&StreamChunk::Finish(FinishReason::ToolCalls))?;
    assert!(assembler.finish().is_err());
    Ok(())
}
