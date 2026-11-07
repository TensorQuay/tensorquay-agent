//! PROTO-U-005: UTF-8 chunk boundaries, ordering and bounded assembly.

use std::error::Error;

use tqa_types::{
    ChunkError, Event, TOOL_OUTPUT_CHUNK_BYTES, ToolCallId, ToolOutputAssembler, chunk_tool_output,
};

fn chunk(id: &str, index: u32, last: bool, data: &str) -> Result<Event, Box<dyn Error>> {
    Ok(Event::ToolOutput {
        call_id: id.parse()?,
        chunk: index,
        last,
        data: data.to_owned(),
    })
}

#[test]
fn proto_u_005_multibyte_output_reassembles_exactly() -> Result<(), Box<dyn Error>> {
    let data = "aa".to_owned() + &"界".repeat((200 * 1024 - 2) / 3);
    assert_eq!(data.len(), 200 * 1024);
    let call_id: ToolCallId = "call".parse()?;
    let chunks = chunk_tool_output(call_id.clone(), &data);
    assert_eq!(chunks.len(), 4);
    let mut assembler = ToolOutputAssembler::new(1, data.len());
    let mut result = None;
    for (index, event) in chunks.iter().enumerate() {
        let Event::ToolOutput {
            call_id: actual,
            chunk,
            last,
            data: part,
        } = event
        else {
            return Err("chunker emitted wrong event".into());
        };
        assert_eq!(actual, &call_id);
        assert_eq!(*chunk, u32::try_from(index)?);
        assert_eq!(*last, index + 1 == chunks.len());
        assert!(part.len() <= TOOL_OUTPUT_CHUNK_BYTES);
        assert!(std::str::from_utf8(part.as_bytes()).is_ok());
        result = assembler.push(event)?;
        assert_eq!(result.is_some(), *last);
    }
    assert_eq!(result, Some(data));
    Ok(())
}

#[test]
fn proto_u_005_empty_and_exact_boundary_output() -> Result<(), Box<dyn Error>> {
    for data in [String::new(), "x".repeat(TOOL_OUTPUT_CHUNK_BYTES)] {
        let chunks = chunk_tool_output("call".parse()?, &data);
        assert_eq!(chunks, vec![chunk("call", 0, true, &data)?]);
        let mut assembler = ToolOutputAssembler::new(1, data.len());
        assert_eq!(assembler.push(&chunks[0])?, Some(data));
    }
    Ok(())
}

#[test]
fn proto_u_005_out_of_order_does_not_change_pending_output() -> Result<(), Box<dyn Error>> {
    let mut assembler = ToolOutputAssembler::new(2, 16);
    assert_eq!(
        assembler.push(&chunk("a", 1, true, "b")?),
        Err(ChunkError::UnknownCall)
    );
    assert_eq!(assembler.push(&chunk("a", 0, false, "a")?)?, None);
    assert_eq!(
        assembler.push(&chunk("a", 2, true, "bad")?),
        Err(ChunkError::OutOfOrder {
            expected: 1,
            got: 2
        })
    );
    assert_eq!(
        assembler.push(&chunk("b", 0, true, "other")?)?,
        Some("other".to_owned())
    );
    assert_eq!(
        assembler.push(&chunk("a", 1, true, "b")?)?,
        Some("ab".to_owned())
    );
    Ok(())
}

#[test]
fn proto_u_005_capacity_errors_leave_state_unchanged() -> Result<(), Box<dyn Error>> {
    let mut assembler = ToolOutputAssembler::new(1, 5);
    assembler.push(&chunk("a", 0, false, "1234")?)?;
    assert_eq!(
        assembler.push(&chunk("b", 0, false, "x")?),
        Err(ChunkError::TooManyCalls { max: 1 })
    );
    assert_eq!(
        assembler.push(&chunk("a", 1, true, "56")?),
        Err(ChunkError::OutputTooLarge { bytes: 6, max: 5 })
    );
    assert_eq!(
        assembler.push(&chunk("a", 1, true, "5")?)?,
        Some("12345".to_owned())
    );
    assert_eq!(
        assembler.push(&chunk("b", 0, true, "x")?)?,
        Some("x".to_owned())
    );
    Ok(())
}

#[test]
fn proto_u_005_oversized_chunks_and_wrong_events_are_rejected() -> Result<(), Box<dyn Error>> {
    let mut assembler = ToolOutputAssembler::new(1, 3 * TOOL_OUTPUT_CHUNK_BYTES);
    assembler.push(&chunk("a", 0, false, "start")?)?;
    assert_eq!(
        assembler.push(&Event::TextDelta {
            text: "wrong".to_owned()
        }),
        Err(ChunkError::NotToolOutput)
    );
    let bytes = TOOL_OUTPUT_CHUNK_BYTES + 1;
    assert_eq!(
        assembler.push(&chunk("a", 1, true, &"x".repeat(bytes))?),
        Err(ChunkError::ChunkTooLarge {
            bytes,
            max: TOOL_OUTPUT_CHUNK_BYTES
        })
    );
    assert_eq!(
        assembler.push(&chunk("a", 1, true, "end")?)?,
        Some("startend".to_owned())
    );
    Ok(())
}

#[test]
fn proto_u_005_new_call_byte_limit_and_zero_capacity() -> Result<(), Box<dyn Error>> {
    let mut assembler = ToolOutputAssembler::new(1, 1);
    assert_eq!(
        assembler.push(&chunk("a", 0, false, "ab")?),
        Err(ChunkError::OutputTooLarge { bytes: 2, max: 1 })
    );
    assert_eq!(
        assembler.push(&chunk("b", 0, true, "x")?)?,
        Some("x".to_owned())
    );
    assert_eq!(
        ToolOutputAssembler::new(0, 1).push(&chunk("a", 0, true, "x")?),
        Err(ChunkError::TooManyCalls { max: 0 })
    );
    Ok(())
}
