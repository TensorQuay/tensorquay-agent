//! CORE-U-044: opaque reasoning states are journaled in stream order within their bounds, and replayed
//! only to the route and model that produced them.
mod support;
use serde_json::json;
use std::{sync::Arc, time::Duration};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RouteProvider, RunOutcome, Runner, UserTurn};
use tqa_llm::{ContentPart, FinishReason, LlmError, Registry, StreamChunk};
use tqa_testkit::{FakeProvider, ProviderReply};
use tqa_types::{
    Event, MAX_REASONING_ITEM_BYTES, MAX_REASONING_ITEMS_PER_STEP, RouteSnapshot, WireProtocol,
};

fn item(id: &str, padding: usize) -> String {
    json!({"type":"reasoning","id":id,"encrypted_content":"x".repeat(padding)}).to_string()
}
/// A tool-call reply that first streams the given opaque states.
fn stateful(states: &[String], call: &str) -> ProviderReply {
    let mut chunks: Vec<StreamChunk> = vec![StreamChunk::ReasoningDelta("plan".into())];
    chunks.extend(states.iter().cloned().map(StreamChunk::ReasoningState));
    chunks.extend([
        StreamChunk::ToolCallDelta {
            index: 0,
            id: Some(call.into()),
            name: Some("read_file".into()),
            arguments_delta: r#"{"path":"a"}"#.into(),
        },
        usage(),
        StreamChunk::Finish(FinishReason::ToolCalls),
    ]);
    reply(chunks)
}
fn turn(text: &str) -> UserTurn {
    UserTurn {
        lead_brief: None,
        team_wake: None,
        text: text.into(),
        attachments: vec![],
    }
}
/// The recorded states of the run: (item, bytes) in journal order.
fn recorded(events: &[Event]) -> Vec<(Option<String>, u32)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::ReasoningState { item, bytes, .. } => {
                Some((item.as_ref().map(|item| item.as_str().to_owned()), *bytes))
            }
            _ => None,
        })
        .collect()
}
/// The opaque states sent in a request's assistant messages, in order.
fn replayed(request: &tqa_llm::CompletionRequest) -> Vec<String> {
    request
        .messages
        .iter()
        .flat_map(|message| &message.content)
        .filter_map(|part| match part {
            ContentPart::ReasoningState(item) => Some(item.clone()),
            _ => None,
        })
        .collect()
}
fn responses(mut route: RouteSnapshot) -> RouteSnapshot {
    route.protocol = WireProtocol::OpenAiResponses;
    route
}

#[tokio::test]
async fn core_u_044_states_are_journaled_before_the_calls_and_replayed_first() -> TestResult {
    let tool = fake("read_file", true, vec![output("ok", 0)])?;
    let states = [item("rs_a", 8), item("rs_b", 8)];
    let mut h = Harness::new(vec![stateful(&states, "c1"), answer()], registry(&[tool])?)?;
    h.deps.route = responses(route());
    let (provider, sink) = (h.provider.clone(), h.sink.clone());
    let runner = Runner::new(h.deps)?;
    let outcome = drive(
        runner.run(
            &[],
            Some(turn("go")),
            CancellationToken::new(),
            CancellationToken::new(),
        ),
        &h.clock,
    )
    .await??;
    assert_eq!(outcome, RunOutcome::Completed);
    let events = sink.events();
    let names: Vec<String> = events
        .iter()
        .map(event_name)
        .filter(|name| ["reasoning.delta", "reasoning.state", "tool.call"].contains(&name.as_str()))
        .collect();
    assert_eq!(
        names,
        [
            "reasoning.delta",
            "reasoning.state",
            "reasoning.state",
            "tool.call"
        ]
    );
    let state = events
        .iter()
        .find_map(|event| match event {
            Event::ReasoningState {
                protocol,
                route,
                model,
                ..
            } => Some((*protocol, route.clone(), model.clone())),
            _ => None,
        })
        .ok_or("no state")?;
    assert_eq!(
        state,
        (
            WireProtocol::OpenAiResponses,
            "test".into(),
            "fake-model".into()
        )
    );
    assert_eq!(
        recorded(&events),
        states
            .iter()
            .map(|item| (
                Some(item.clone()),
                u32::try_from(item.len()).unwrap_or(u32::MAX)
            ))
            .collect::<Vec<_>>()
    );
    let requests = provider.requests();
    assert_eq!(replayed(&requests[1]), states, "replayed in order");
    let assistant = requests[1]
        .messages
        .iter()
        .find(|message| message.role == tqa_llm::Role::Assistant)
        .ok_or("no assistant message")?;
    assert!(matches!(
        assistant.content[0],
        ContentPart::ReasoningState(_)
    ));
    assert!(matches!(
        assistant.content.last(),
        Some(ContentPart::Text(_))
    ));
    Ok(())
}

fn event_name(event: &Event) -> String {
    serde_json::to_value(event)
        .ok()
        .and_then(|value| value["type"].as_str().map(str::to_owned))
        .unwrap_or_default()
}

#[tokio::test]
async fn core_u_044_items_over_their_bounds_are_journaled_without_the_item() -> TestResult {
    let big = item("big", MAX_REASONING_ITEM_BYTES);
    let many: Vec<String> = (0..=MAX_REASONING_ITEMS_PER_STEP)
        .map(|index| item(&format!("m{index}"), 8))
        .collect();
    let near = MAX_REASONING_ITEM_BYTES - 200;
    let heavy: Vec<String> = (0..5)
        .map(|index| item(&format!("h{index}"), near))
        .collect();
    // Within the item bound, but its escaped form would not fit in one event frame.
    let quoted = format!(
        r#"{{"t":"{}"}}"#,
        r#"\""#.repeat((MAX_REASONING_ITEM_BYTES - 8) / 2)
    );
    assert!(quoted.len() <= MAX_REASONING_ITEM_BYTES);
    for (states, kept) in [
        (vec![big.clone()], vec![]),
        (many.clone(), many[..MAX_REASONING_ITEMS_PER_STEP].to_vec()),
        (heavy.clone(), heavy[..4].to_vec()),
        (vec![quoted.clone()], vec![]),
    ] {
        let tool = fake("read_file", true, vec![output("ok", 0)])?;
        let mut h = Harness::new(vec![stateful(&states, "c1"), answer()], registry(&[tool])?)?;
        h.deps.route = responses(route());
        let (provider, sink) = (h.provider.clone(), h.sink.clone());
        let runner = Runner::new(h.deps)?;
        drive(
            runner.run(
                &[],
                Some(turn("go")),
                CancellationToken::new(),
                CancellationToken::new(),
            ),
            &h.clock,
        )
        .await??;
        let journal = recorded(&sink.events());
        assert_eq!(journal.len(), states.len(), "every state leaves a record");
        for ((item, bytes), sent) in journal.iter().zip(&states) {
            assert_eq!(usize::try_from(*bytes)?, sent.len());
            assert_eq!(item.is_some(), kept.contains(sent));
        }
        assert_eq!(replayed(&provider.requests()[1]), kept);
    }
    Ok(())
}

fn rate_limited() -> ProviderReply {
    ProviderReply {
        delay: Duration::ZERO,
        result: Err(LlmError::RateLimited {
            retry_after: Some(Duration::from_secs(90)),
        }),
    }
}

#[tokio::test]
async fn core_u_044_states_go_back_only_to_the_route_and_model_that_made_them() -> TestResult {
    let tool = fake("read_file", true, vec![output("ok", 0), output("ok", 0)])?;
    let own = item("own", 8);
    // Step 1 records a state on the own route; step 2 is rate limited and moves to a route with the
    // same model name on another endpoint; the next run returns to the own route.
    let mut h = Harness::new(
        vec![
            stateful(std::slice::from_ref(&own), "c1"),
            rate_limited(),
            answer(),
        ],
        registry(&[tool])?,
    )?;
    h.deps.route = responses(route());
    let routes = Registry::from_toml(
        "[[routes]]\nname='backup'\nprotocol='openai_responses'\nbase_url='http://localhost'\nmodel='fake-model'\nauth={kind='none'}\n",
    )?;
    let backup = Arc::new(FakeProvider::new(
        routes.get("backup").ok_or("missing route")?.clone(),
        vec![answer()],
        Arc::new(h.sleeper.clone()),
    ));
    h.deps.fallback_routes = vec![RouteProvider {
        route: RouteSnapshot {
            name: "backup".into(),
            relay_path: "/relay/r/backup".into(),
            ..responses(route())
        },
        provider: backup.clone(),
    }];
    let (primary, sink) = (h.provider.clone(), h.sink.clone());
    let runner = Runner::new(h.deps)?;
    drive(
        runner.run(
            &[],
            Some(turn("first")),
            CancellationToken::new(),
            CancellationToken::new(),
        ),
        &h.clock,
    )
    .await??;
    assert!(
        replayed(&backup.requests()[0]).is_empty(),
        "another endpoint never gets the state"
    );
    let history = sink.frames();
    drive(
        runner.run(
            &history,
            Some(turn("second")),
            CancellationToken::new(),
            CancellationToken::new(),
        ),
        &h.clock,
    )
    .await??;
    let requests = primary.requests();
    assert_eq!(
        replayed(requests.last().ok_or("no request")?),
        [own],
        "the own route reuses it"
    );
    Ok(())
}

#[tokio::test]
async fn core_u_044_a_new_model_on_the_same_route_does_not_get_the_old_state() -> TestResult {
    let tool = fake("read_file", true, vec![output("ok", 0)])?;
    let mut first = Harness::new(
        vec![stateful(&[item("old", 8)], "c1"), answer()],
        registry(std::slice::from_ref(&tool))?,
    )?;
    first.deps.route = responses(route());
    let sink = first.sink.clone();
    let runner = Runner::new(first.deps)?;
    drive(
        runner.run(
            &[],
            Some(turn("first")),
            CancellationToken::new(),
            CancellationToken::new(),
        ),
        &first.clock,
    )
    .await??;
    let history = sink.frames();
    // The same route, now pointing at another model.
    let routes = Registry::from_toml(
        "[[routes]]\nname='test'\nprotocol='openai_responses'\nbase_url='http://localhost'\nmodel='new-model'\nauth={kind='none'}\n",
    )?;
    let mut second = Harness::new(vec![answer()], registry(&[tool])?)?;
    let provider = Arc::new(FakeProvider::new(
        routes.get("test").ok_or("missing route")?.clone(),
        vec![answer()],
        Arc::new(second.sleeper.clone()),
    ));
    second.deps.provider = provider.clone();
    second.deps.route = RouteSnapshot {
        model: "new-model".into(),
        ..responses(route())
    };
    second.sink_with(MemorySink::new(&history));
    let runner = Runner::new(second.deps)?;
    drive(
        runner.run(
            &history,
            Some(turn("second")),
            CancellationToken::new(),
            CancellationToken::new(),
        ),
        &second.clock,
    )
    .await??;
    assert!(replayed(&provider.requests()[0]).is_empty());
    Ok(())
}

// TEST: CORE-U-044
#[tokio::test]
async fn core_u_044_a_short_window_drops_the_oldest_states_and_keeps_the_latest_turns() -> TestResult
{
    // Decided (option a): replayed states from earlier turns are optional to the providers; only the latest
    // assistant turn's (for its tool calls) must go back. On the harness route's 32k window, two steps' 70 KB states
    // don't both fit: the older goes, the latest stays, and the run never fails for states alone.
    let (older, latest) = (item("rs_old", 70_000), item("rs_new", 70_000));
    let tools = [fake(
        "read_file",
        true,
        vec![output("ok", 0), output("ok", 0)],
    )?];
    let mut h = Harness::new(
        vec![
            stateful(std::slice::from_ref(&older), "c1"),
            stateful(std::slice::from_ref(&latest), "c2"),
            answer(),
        ],
        registry(&tools)?,
    )?;
    h.deps.route = responses(route());
    let (provider, sink) = (h.provider.clone(), h.sink.clone());
    let runner = Runner::new(h.deps)?;
    let outcome = drive(
        runner.run(
            &[],
            Some(turn("go")),
            CancellationToken::new(),
            CancellationToken::new(),
        ),
        &h.clock,
    )
    .await??;
    assert_eq!(outcome, RunOutcome::Completed);
    let requests = provider.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(
        replayed(&requests[1]),
        std::slice::from_ref(&older),
        "one step's state fits"
    );
    assert_eq!(
        replayed(&requests[2]),
        [latest],
        "the older state goes first"
    );
    // Both stay journaled: dropping is only what a request sends.
    assert!(
        recorded(&sink.events())
            .iter()
            .all(|(item, _)| item.is_some())
    );
    assert!(
        !sink
            .events()
            .iter()
            .any(|event| matches!(event, Event::Compaction { .. }))
    );
    Ok(())
}
