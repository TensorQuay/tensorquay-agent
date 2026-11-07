//! Generated scripts check both journal/request agreement and an independent message oracle.
mod support;
use serde_json::{Value, json};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_llm::{FinishReason, StreamChunk};
use tqa_testkit::{FakeToolReply, ProviderReply};
use tqa_types::{Event, ServerFrame};

struct Scenario {
    replies: Vec<ProviderReply>,
    tools: Vec<FakeToolReply>,
    requests: Vec<Vec<Value>>,
    final_messages: Vec<Value>,
}

#[tokio::test]
async fn core_u_036_seeded_scripts_project_every_model_attempt_from_the_journal() -> TestResult {
    let mut seed = 0x243f_6a88_85a3_08d3;
    for case in 0..64 {
        let scenario = scenario(&mut seed, case);
        let tool = fake("read_file", true, scenario.tools)?;
        let mut h = Harness::new(scenario.replies, registry(&[tool])?)?;
        let history = superseded_history()?;
        h.sink_with(MemorySink::new(&history));
        let runner = Runner::new(h.deps)?;
        assert_eq!(
            drive(
                runner.run(
                    &history,
                    None,
                    CancellationToken::new(),
                    CancellationToken::new()
                ),
                &h.clock
            )
            .await??,
            RunOutcome::Completed
        );
        let frames = h.sink.frames();
        let requests = h.provider.requests();
        let starts: Vec<_> = frames
            .iter()
            .enumerate()
            .skip(history.len())
            .filter(|(_, frame)| {
                matches!(
                    frame,
                    ServerFrame::Event {
                        event: Event::StepStarted { .. },
                        ..
                    }
                )
            })
            .map(|(index, _)| index)
            .collect();
        assert_eq!(starts.len(), scenario.requests.len(), "case {case}");
        assert_eq!(requests.len(), starts.len(), "case {case}");
        for ((index, request), expected) in starts.iter().zip(&requests).zip(&scenario.requests) {
            let projected = tqa_context::project(&frames[..=*index])?;
            let messages: Vec<_> = projected.iter().map(projected_message).collect();
            assert_eq!(&messages, expected, "oracle case {case}, seq {index}");
            let actual = request_value(request);
            assert_eq!(
                &actual["messages"].as_array().ok_or("missing messages")?[3..],
                messages,
                "request case {case}, seq {index}"
            );
        }
        assert_eq!(
            tqa_context::project(&frames)?
                .iter()
                .map(projected_message)
                .collect::<Vec<_>>(),
            scenario.final_messages,
            "final case {case}"
        );
        assert_resumed(&history, &frames)?;
    }
    Ok(())
}

fn superseded_history() -> Result<Vec<ServerFrame>, Box<dyn std::error::Error>> {
    let mut events = initial()?;
    events.extend([
        Event::TextDelta {
            text: "superseded complete answer".into(),
        },
        model(),
        Event::StepStarted {
            step: 1,
            attempt: 2,
        },
        Event::TextDelta {
            text: "superseding incomplete answer".into(),
        },
    ]);
    history(events)
}

fn scenario(seed: &mut u64, case: usize) -> Scenario {
    let mut scenario = Scenario {
        replies: vec![],
        tools: vec![],
        requests: vec![],
        final_messages: vec![message("user", "question", &[], None)],
    };
    let steps = 1 + draw(seed) % 4;
    for step in 0..steps {
        for _ in 0..draw(seed) % 3 {
            scenario.requests.push(scenario.final_messages.clone());
            scenario
                .replies
                .push(reply(vec![StreamChunk::TextDelta(format!(
                    "discarded partial {case}/{step}"
                ))]));
        }
        scenario.requests.push(scenario.final_messages.clone());
        let text = format!("step {case}/{step}: λ\n");
        let mut chunks = vec![
            StreamChunk::ReasoningDelta("private reasoning".into()),
            StreamChunk::TextDelta(text.clone()),
        ];
        let mut prepared = vec![];
        let mut results = vec![];
        for index in 0..=(draw(seed) % 3) {
            let id = format!("c{case}s{step}t{index}");
            let args = json!({"path":format!("file-{step}-{index}")});
            let output_text = format!("result {case}/{step}/{index}: 内容");
            chunks.push(StreamChunk::ToolCallDelta {
                index,
                id: Some(id.clone()),
                name: Some("read_file".into()),
                arguments_delta: args.to_string(),
            });
            prepared.push(json!([id, "read_file", args.to_string()]));
            results.push(message("tool", &output_text, &[], Some(&id)));
            scenario
                .tools
                .push(output(&output_text, u64::from(draw(seed) % 4) * 10));
        }
        chunks.extend([usage(), StreamChunk::Finish(FinishReason::ToolCalls)]);
        scenario.replies.push(reply(chunks));
        scenario
            .final_messages
            .push(message("assistant", &text, &prepared, None));
        scenario.final_messages.extend(results);
    }
    scenario.requests.push(scenario.final_messages.clone());
    scenario.replies.push(answer());
    scenario
        .final_messages
        .push(message("assistant", "final answer", &[], None));
    scenario
}

fn draw(seed: &mut u64) -> u32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    (*seed >> 32) as u32
}

fn message(role: &str, text: &str, calls: &[Value], call_id: Option<&str>) -> Value {
    json!({"role":role,"content":[text],"calls":calls,"call_id":call_id,"name":null})
}

fn projected_message(value: &tqa_context::Message) -> Value {
    let role = match value.role {
        tqa_context::Role::System => "system",
        tqa_context::Role::User => "user",
        tqa_context::Role::Assistant => "assistant",
        tqa_context::Role::Tool => "tool",
    };
    message(
        role,
        &value.text,
        &value
            .tool_calls
            .iter()
            .map(|call| json!([call.call_id, call.name, call.arguments]))
            .collect::<Vec<_>>(),
        value
            .tool_call_id
            .as_ref()
            .map(tqa_types::ToolCallId::as_str),
    )
}
