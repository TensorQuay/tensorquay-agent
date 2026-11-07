//! LLM-U-046: the Chat decoder counts the chunks in which the server held the reply back: a choice with no text, no
//! reasoning, no tool call, no role and no finish reason. A server whose own tool-call parser takes a call the model
//! wrote in its native format streams one such chunk per withheld token; on a text route the call then never
//! arrives, and this count is what shows it (Gate 5, 29 Sep). The fixtures are the 80 replies captured that day, cut
//! to each chunk's choices and usage (`call-*` delivered a call block, `prose-*` and `empty-*` did not), plus a
//! synthesized genuine final answer.
use super::wire::{CallIndex, decode_in};
use crate::{StreamLimits, collect::StreamAssembler};
use std::path::Path;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// A fixture's completion, decoded and assembled as the adapter does.
fn completion(path: &Path) -> Result<crate::Completion, Box<dyn std::error::Error>> {
    let mut assembler = StreamAssembler::new(StreamLimits::RECOMMENDED);
    let mut calls = CallIndex::default();
    for line in std::fs::read_to_string(path)?.lines() {
        let Some(data) = line.strip_prefix("data: ") else {
            continue;
        };
        if data == "[DONE]" {
            break;
        }
        for chunk in decode_in(data, &mut calls)? {
            assembler.push(&chunk)?;
        }
    }
    Ok(assembler.finish()?)
}

// TEST: LLM-U-046
#[test]
fn llm_u_046_withheld_chunks_are_counted_and_every_good_reply_has_none() -> TestResult {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/withheld");
    let mut seen = std::collections::BTreeMap::<String, usize>::new();
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("fixture name")?
            .to_owned();
        let kind = name.split('-').next().ok_or("fixture kind")?.to_owned();
        let completion = completion(&path)?;
        match kind.as_str() {
            // A delivered call block, or a genuine final answer: nothing was held back.
            "call" | "final" => assert_eq!(completion.withheld_chunks, 0, "{name}"),
            // Prose with no call, or nothing at all: the server held tokens back.
            "prose" | "empty" => assert!(completion.withheld_chunks > 0, "{name}"),
            other => return Err(format!("unknown fixture kind {other}").into()),
        }
        if kind == "call" {
            assert!(completion.text.contains("```tqa_call"), "{name}");
        }
        *seen.entry(kind).or_default() += 1;
    }
    assert_eq!(
        seen,
        [("call", 54), ("empty", 6), ("final", 1), ("prose", 20)]
            .into_iter()
            .map(|(kind, count)| (kind.to_owned(), count))
            .collect(),
        "every captured reply is a fixture"
    );
    Ok(())
}

// TEST: LLM-U-046
#[test]
fn llm_u_046_a_role_only_chunk_and_an_empty_text_delta_are_not_withheld() -> TestResult {
    let mut calls = CallIndex::default();
    for data in [
        r#"{"choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}"#,
        r#"{"choices":[{"index":0,"delta":{"content":""},"finish_reason":null}]}"#,
        r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
        r#"{"choices":[],"usage":{"prompt_tokens":1,"completion_tokens":1}}"#,
    ] {
        let chunks = decode_in(data, &mut calls)?;
        assert!(!chunks.contains(&crate::StreamChunk::Withheld), "{data}");
    }
    let withheld = decode_in(
        r#"{"choices":[{"index":0,"delta":{},"finish_reason":null}]}"#,
        &mut calls,
    )?;
    assert_eq!(withheld, vec![crate::StreamChunk::Withheld]);
    Ok(())
}
