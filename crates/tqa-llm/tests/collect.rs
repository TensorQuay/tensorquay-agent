//! LLM-C-002, LLM-E-007/009, LLM-U-045: bounded assembly preserves stream order and partial output.
//! LLM-U-050: an inconsistent or incomplete tool call is a `Malformed` answer, which the loop asks again once.

use futures::stream;
use proptest::prelude::*;
use std::error::Error;
use tqa_llm::{
    CompletionStream, FinishReason, LlmError, StreamChunk, StreamLimitKind, StreamLimits, Usage,
    collect,
};

fn chunks(values: Vec<StreamChunk>) -> CompletionStream {
    Box::pin(stream::iter(values.into_iter().map(Ok)))
}

#[tokio::test]
async fn llm_c_002_collect_interleaved_calls_and_usage() -> Result<(), Box<dyn Error>> {
    let mut values = vec![
        StreamChunk::TextDelta("hello".into()),
        StreamChunk::ReasoningDelta("plan".into()),
    ];
    for (index, part) in ["{", "\"n\"", ":", "1", "}"].iter().enumerate() {
        for call in [1, 0] {
            values.push(StreamChunk::ToolCallDelta {
                index: call,
                id: (index == 0).then(|| format!("call-{call}")),
                name: (index == 0).then(|| "read".into()),
                arguments_delta: (*part).into(),
            });
        }
    }
    let usage = Usage {
        prompt_tokens: 10,
        completion_tokens: 3,
        cached_tokens: None,
        reasoning_tokens: Some(2),
    };
    values.extend([
        StreamChunk::Finish(FinishReason::ToolCalls),
        StreamChunk::Usage(usage.clone()),
    ]);
    let result = collect(chunks(values), StreamLimits::RECOMMENDED).await?;
    assert_eq!(result.text, "hello");
    assert_eq!(result.reasoning, "plan");
    assert_eq!(
        result
            .tool_calls
            .iter()
            .map(|call| call.id.as_str())
            .collect::<Vec<_>>(),
        ["call-0", "call-1"]
    );
    assert!(
        result
            .tool_calls
            .iter()
            .all(|call| call.arguments == "{\"n\":1}")
    );
    assert_eq!(result.usage, Some(usage));
    Ok(())
}

#[tokio::test]
async fn llm_e_007_collect_cut_has_partial_text() {
    let result = collect(
        chunks(vec![StreamChunk::TextDelta("partial".into())]),
        StreamLimits::RECOMMENDED,
    )
    .await;
    assert!(matches!(result, Err(LlmError::StreamCut { partial }) if partial.text == "partial"));
}

#[tokio::test]
async fn llm_e_009_collect_rejects_incomplete_tool_json() {
    let result = collect(
        chunks(vec![
            StreamChunk::ToolCallDelta {
                index: 0,
                id: Some("c".into()),
                name: Some("read".into()),
                arguments_delta: "{\"path\":".into(),
            },
            StreamChunk::Finish(FinishReason::ToolCalls),
        ]),
        StreamLimits::RECOMMENDED,
    )
    .await;
    assert!(matches!(
        result,
        Err(LlmError::ToolArgsInvalid { index: 0, .. })
    ));
}

/// A call whose arguments stop mid-JSON because the answer finished on the output limit.
fn cut_call(usage: Option<u64>) -> CompletionStream {
    let mut values = vec![StreamChunk::ToolCallDelta {
        index: 0,
        id: Some("c".into()),
        name: Some("read".into()),
        arguments_delta: "{\"path\":\"src/ma".into(),
    }];
    values.extend(usage.map(|completion_tokens| {
        StreamChunk::Usage(Usage {
            prompt_tokens: 10,
            completion_tokens,
            cached_tokens: None,
            reasoning_tokens: None,
        })
    }));
    values.push(StreamChunk::Finish(FinishReason::Length));
    chunks(values)
}

#[tokio::test]
async fn llm_u_045_arguments_cut_on_the_output_limit_say_so() -> Result<(), Box<dyn Error>> {
    for (usage, text) in [
        (
            Some(64),
            "tool 0 arguments were cut at the output limit (64 tokens)",
        ),
        (None, "tool 0 arguments were cut at the output limit"),
    ] {
        let error = collect(cut_call(usage), StreamLimits::RECOMMENDED)
            .await
            .err()
            .ok_or("a cut call was accepted")?;
        assert!(
            matches!(
                error,
                LlmError::ToolArgsCut {
                    index: 0,
                    output_tokens
                } if output_tokens == usage
            ),
            "{usage:?}: {error:?}"
        );
        assert_eq!(error.to_string(), text);
        assert_eq!(error.code(), "tool_args_cut");
        assert!(!error.retryable());
    }
    Ok(())
}

#[tokio::test]
async fn llm_e_016_collect_enforces_byte_and_call_limits() {
    let small = StreamLimits {
        max_completion_bytes: 3,
        ..StreamLimits::RECOMMENDED
    };
    let result = collect(chunks(vec![StreamChunk::TextDelta("界x".into())]), small).await;
    assert!(matches!(
        result,
        Err(LlmError::StreamLimit {
            which: StreamLimitKind::Completion,
            max: 3
        })
    ));
    let small = StreamLimits {
        max_tool_calls: 0,
        ..StreamLimits::RECOMMENDED
    };
    let result = collect(
        chunks(vec![StreamChunk::ToolCallDelta {
            index: u32::MAX,
            id: None,
            name: None,
            arguments_delta: String::new(),
        }]),
        small,
    )
    .await;
    assert!(matches!(
        result,
        Err(LlmError::StreamLimit {
            which: StreamLimitKind::ToolCalls,
            max: 0
        })
    ));
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, failure_persistence: None, rng_seed: proptest::test_runner::RngSeed::Fixed(5005), ..ProptestConfig::default() })]
    #[test]
    fn llm_c_002_property_chunk_splits_preserve_completion(parts in proptest::collection::vec("[^\\p{C}]{0,12}", 0..40)) {
        let expected = parts.concat();
        let mut values: Vec<_> = parts.into_iter().map(StreamChunk::TextDelta).collect();
        values.push(StreamChunk::Finish(FinishReason::Stop));
        let result = futures::executor::block_on(collect(chunks(values), StreamLimits::RECOMMENDED));
        prop_assert!(matches!(result, Ok(completion) if completion.text == expected));
    }
}

#[tokio::test]
async fn llm_e_009_call_identity_must_be_complete_and_stable() {
    let initial = StreamChunk::ToolCallDelta {
        index: 2,
        id: Some("call".into()),
        name: Some("read".into()),
        arguments_delta: "{}".into(),
    };
    for (id, name) in [(Some("different"), None), (None, Some("different"))] {
        let changed = StreamChunk::ToolCallDelta {
            index: 2,
            id: id.map(str::to_owned),
            name: name.map(str::to_owned),
            arguments_delta: String::new(),
        };
        assert!(matches!(
            collect(
                chunks(vec![initial.clone(), changed]),
                StreamLimits::RECOMMENDED
            )
            .await,
            Err(LlmError::Malformed(_))
        ));
    }
    for (id, name) in [(Some("call"), None), (None, Some("read"))] {
        let incomplete = StreamChunk::ToolCallDelta {
            index: 2,
            id: id.map(str::to_owned),
            name: name.map(str::to_owned),
            arguments_delta: "{}".into(),
        };
        assert!(matches!(
            collect(
                chunks(vec![
                    incomplete,
                    StreamChunk::Finish(FinishReason::ToolCalls)
                ]),
                StreamLimits::RECOMMENDED
            )
            .await,
            Err(LlmError::Malformed(_))
        ));
    }
}

/// An answer that couldn't be read is `Malformed`, which the loop asks again once (CORE-E-040); a refused request
/// stays `Protocol` and is never retried. The 13h Gate 5 failure that couldn't be reproduced ended on such an answer.
#[tokio::test]
async fn llm_u_050_an_unreadable_answer_is_malformed_not_protocol() {
    let nameless = StreamChunk::ToolCallDelta {
        index: 0,
        id: None,
        name: None,
        arguments_delta: "{}".into(),
    };
    let collected = collect(
        chunks(vec![nameless, StreamChunk::Finish(FinishReason::ToolCalls)]),
        StreamLimits::RECOMMENDED,
    )
    .await;
    assert!(
        matches!(collected, Err(LlmError::Malformed(_))),
        "{collected:?}"
    );
    assert_eq!(LlmError::Malformed("x".into()).code(), "malformed");
    assert!(
        !LlmError::Malformed("x".into()).retryable(),
        "the loop's one retry is its own"
    );
}
