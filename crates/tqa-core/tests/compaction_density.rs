//! CORE-U-084 since c16: each step call's usage records the runner's estimate of the request it sent
//! (`estimated_input`) and its image tokens (`estimated_images`), so the route's density is read from the journal:
//! exactly, in one frame, the same after a resume, whatever states that request had dropped.
mod ledger_history;
mod support;
use ledger_history::*;
use serde_json::json;
use std::error::Error;
use support::*;
use tqa_types::{CompactionSource, Event, IMAGE_TOKENS, ReasoningItem, ServerFrame, WireProtocol};

/// A route on the Responses protocol, which replays reasoning states.
fn on_responses() -> impl FnOnce(&mut Harness) -> Result<(), Box<dyn Error>> {
    |h: &mut Harness| {
        h.deps.route.protocol = WireProtocol::OpenAiResponses;
        Ok(())
    }
}

/// The replayed states' share of a request, at the estimate's rate.
fn states_tokens(request: &tqa_llm::CompletionRequest) -> u64 {
    full_tokens(request).saturating_sub(full_tokens(&without_states(request)))
}
fn without_states(request: &tqa_llm::CompletionRequest) -> tqa_llm::CompletionRequest {
    let mut copy = request.clone();
    for message in &mut copy.messages {
        message
            .content
            .retain(|part| !matches!(part, tqa_llm::ContentPart::ReasoningState(_)));
    }
    copy
}

// TEST: CORE-U-084
// TEST: CORE-U-044
/// A request that had dropped its older states recorded what it really sent, so the next one knows the route's
/// density exactly and never sends those states back past the window uncompacted. This is the review's P2-A case of
/// 7 Oct, which a recomputed density got wrong (80 of 390 cases overflowed). The same journal read twice gives the
/// same requests: nothing is kept in memory.
#[tokio::test]
async fn core_u_084_a_request_that_dropped_states_records_what_it_sent() -> TestResult {
    const STATE_RUNS: usize = 12;
    let capacity = u64::from(WINDOW - RESERVE);
    for padding in [300_usize, 800, 1300] {
        let events = || with_states(long_history_of(ledger, STATE_RUNS)?, padding);
        let (_, requests) = next_turn_with(events()?, on_responses(), &[]).await?;
        let first = requests.first().ok_or("no request")?;
        // The last call had dropped every older state: it sent the text and the tools only.
        let sent = full_tokens(first).saturating_sub(states_tokens(first));
        for tenths in [13_u64, 16, 20, 26, 35] {
            let real = sent * tenths / 10;
            let journal = || with_last_usage(events()?, recorded_usage(real, 5, sent));
            let (compacted, requests) = next_turn_with(journal()?, on_responses(), &[]).await?;
            let next = requests.first().ok_or("no request")?;
            let true_size = full_tokens(next) * tenths / 10;
            assert!(
                compacted > 0 || true_size <= capacity,
                "{tenths}/10, {padding}-byte states: {true_size} sent uncompacted into {capacity}"
            );
            let (again, replay) = next_turn_with(journal()?, on_responses(), &[]).await?;
            assert_eq!(
                (again, replay.first().map(full_tokens)),
                (compacted, Some(full_tokens(next))),
                "the density is read from the journal, not from memory"
            );
        }
    }
    Ok(())
}

// TEST: CORE-U-084
// TEST: CORE-U-081
/// The image a call sent is recorded beside its estimate, so its real count's image comes out of the density and the
/// next request's image is counted once (the review's P2-2).
#[tokio::test]
async fn core_u_084_a_recorded_image_counts_once() -> TestResult {
    let image = u64::from(IMAGE_TOKENS);
    let mut found = None;
    for runs in (4..=RUNS).rev() {
        let (compacted, requests) =
            next_turn_with(with_image(runs)?, vision, &["uploads/a.png"]).await?;
        if compacted == 0 {
            found = Some((
                runs,
                requests.first().map(request_tokens).ok_or("no request")?,
            ));
            break;
        }
    }
    let (runs, text) = found.ok_or("no history stays under")?;
    assert!(
        text + 2 * image > THRESHOLD,
        "counted twice, the image would pass: {text}"
    );
    let usage = Event::Usage {
        prompt_tokens: text + image,
        completion_tokens: 5,
        cached_tokens: None,
        reasoning_tokens: None,
        estimated: false,
        estimated_input: Some(text),
        estimated_images: Some(image),
        estimated_route: Some("test".into()),
    };
    let (compacted, _) = next_turn_with(
        with_last_usage(with_image(runs)?, usage)?,
        vision,
        &["uploads/a.png"],
    )
    .await?;
    assert_eq!(compacted, 0, "the image counted once stays under");
    Ok(())
}

// TEST: CORE-U-084
/// A usage that records no estimate of its request (a compaction's summary call, or an older runtime's) gives no
/// density: it doesn't hide the step call's recorded one before it.
#[tokio::test]
async fn core_u_084_a_usage_without_its_estimate_does_not_hide_a_recorded_one() -> TestResult {
    let (_, estimated) = next_turn(long_history(ledger)?).await?;
    let mut events = with_last_usage(
        long_history(ledger)?,
        recorded_usage(estimated * 164 / 100, 5, estimated),
    )?;
    let last = events
        .iter()
        .rposition(|event| matches!(event, Event::RunCompleted { .. }))
        .ok_or("no run")?;
    events.insert(last, older_usage(40, 5));
    let (compacted, _) = next_turn(events).await?;
    assert_eq!(compacted, 1, "the recorded density decides");
    Ok(())
}

// TEST: CORE-U-084
/// A recorded count past 3.5 times its estimate is read as 3.5: the estimator's bound (a byte-level tokenizer counts at
/// most a token a byte), so a higher figure is a corrupt count, not denser text.
#[tokio::test]
async fn core_u_084_a_count_past_three_and_a_half_times_is_read_as_that() -> TestResult {
    let mut chosen = None;
    for runs in 1..=RUNS {
        let (_, estimated) = next_turn(long_history_of(ledger, runs)?).await?;
        // At most 3.5x stays under the threshold with room for the next request's few new tokens; 5x would pass it.
        if estimated * 7 / 2 + 60 <= THRESHOLD && estimated * 5 > THRESHOLD {
            chosen = Some((runs, estimated));
        }
    }
    let (runs, estimated) = chosen.ok_or("no history between the bounds")?;
    let events = with_last_usage(
        long_history_of(ledger, runs)?,
        recorded_usage(estimated * 5, 5, estimated),
    )?;
    let (compacted, _) = next_turn(events).await?;
    assert_eq!(compacted, 0, "5x is read as 3.5x, which stays under");
    Ok(())
}

/// One run of the long journal: a recorded usage before each `model.completed`, and every 10th run a compaction opening
/// it, folded through `through`.
fn journal_run(events: &mut Vec<Event>, run: Vec<Event>, index: usize, through: u64) {
    for event in run {
        if matches!(event, Event::ModelCompleted { .. }) {
            events.push(recorded_usage(700 + index as u64, 5, 500));
        }
        let compacts = matches!(event, Event::RunStarted { .. }) && index % 10 == 1 && index > 1;
        events.push(event);
        if compacts {
            events.push(Event::Compaction {
                through_seq: tqa_types::Seq::from(through),
                source: CompactionSource::Fallback,
                version: 1,
                summary: model_summary(),
            });
        }
    }
}

/// A replayable state of `padding` bytes before each tool call, distinct per run, as a Responses route records them.
fn run_states(
    events: Vec<Event>,
    padding: usize,
    run: usize,
) -> Result<Vec<Event>, Box<dyn Error>> {
    let mut out = Vec::new();
    for event in events {
        if matches!(event, Event::ToolCall { .. }) {
            let item = json!({"type": "reasoning", "id": format!("rs_{run}"),
                              "encrypted_content": format!("{run:06}{}", "x".repeat(padding))})
            .to_string();
            out.push(Event::ReasoningState {
                protocol: WireProtocol::OpenAiResponses,
                route: "test".into(),
                model: "fake-model".into(),
                bytes: u32::try_from(item.len())?,
                item: Some(ReasoningItem::new(item)?),
            });
        }
        out.push(event);
    }
    Ok(out)
}

// TEST: CORE-U-084
/// Reading the density costs one frame, on any route: a long conversation (400 runs, a recorded usage on each of its
/// 800 calls) on a route that replays states, kept under the threshold by a compaction every 10 runs that keeps the
/// last 5, starts its next turn as fast as a short one. The review of 7 Oct measured 7.3 s there for a density
/// recomputed by walking the chain.
#[tokio::test]
async fn core_u_084_the_density_costs_one_frame_on_a_route_with_states() -> TestResult {
    const RUNS_LONG: usize = 400;
    let mut events: Vec<Event> = Vec::new();
    let mut ends: Vec<u64> = Vec::new();
    for index in 1..=RUNS_LONG {
        let run = run_states(
            completed_run(index, events.len() + 1, ledger(index)?)?,
            300,
            index,
        )?;
        let through = if ends.len() <= 5 {
            u64::try_from(events.len())?
        } else {
            ends[ends.len() - 6]
        };
        journal_run(&mut events, run, index, through);
        ends.push(u64::try_from(events.len())?);
    }
    let frames: Vec<ServerFrame> = history(events.clone())?;
    assert!(frames.len() > 6000, "a long journal: {}", frames.len());
    let started = std::time::Instant::now();
    next_turn_with(events, on_responses(), &[]).await?;
    let took = started.elapsed();
    let bound = tqa_testkit::deadline::scaled(std::time::Duration::from_secs(1));
    assert!(
        took <= bound,
        "the next turn took {took:?} (bound {bound:?})"
    );
    Ok(())
}

/// A recorded usage counted on `route`, with `images` image tokens.
fn usage_on(route: &str, prompt_tokens: u64, estimated_input: u64, images: u64) -> Event {
    Event::Usage {
        prompt_tokens,
        completion_tokens: 5,
        cached_tokens: None,
        reasoning_tokens: None,
        estimated: false,
        estimated_input: Some(estimated_input),
        estimated_images: Some(images),
        estimated_route: Some(route.into()),
    }
}

// TEST: CORE-U-084
/// A density holds only on the route that measured it, by the route's immutable name: a count of 1.64x on another
/// route (a person switched the task's route) gives none, and the next turn goes by the estimate; the same count on
/// this route compacts.
#[tokio::test]
async fn core_u_084_a_density_holds_only_on_the_route_that_measured_it() -> TestResult {
    let (_, estimated) = next_turn(long_history(ledger)?).await?;
    let dense = estimated * 164 / 100;
    let elsewhere = with_last_usage(
        long_history(ledger)?,
        usage_on("other", dense, estimated, 0),
    )?;
    let (compacted, _) = next_turn(elsewhere).await?;
    assert_eq!(compacted, 0, "another route's density doesn't hold here");
    let here = with_last_usage(long_history(ledger)?, usage_on("test", dense, estimated, 0))?;
    let (compacted, _) = next_turn(here).await?;
    assert_eq!(compacted, 1, "this route's density does");
    Ok(())
}

// TEST: CORE-U-084
/// A call without images gives the density in preference to a later one with images: an image's tokens are
/// estimated (1,600), and a provider that counted 85 for each would make the later call read low enough to miss a
/// compaction.
#[tokio::test]
async fn core_u_084_a_call_without_images_gives_the_density_first() -> TestResult {
    let (_, estimated) = next_turn(long_history(ledger)?).await?;
    let mut events = long_history(ledger)?;
    // An earlier call, text only, counted 1.64x.
    let first = events
        .iter()
        .position(|event| matches!(event, Event::ModelCompleted { .. }))
        .ok_or("no model call")?;
    events.insert(first, usage_on("test", estimated * 164 / 100, estimated, 0));
    // The last call sent four images the provider counted as 85 tokens each, against the runner's 1,600: alone it
    // would read about 1.64x less 6,060 tokens, under the threshold.
    let images = 4 * u64::from(IMAGE_TOKENS);
    let late = with_last_usage(
        events,
        usage_on("test", estimated * 164 / 100 + 4 * 85, estimated, images),
    )?;
    let (compacted, _) = next_turn(late).await?;
    assert_eq!(compacted, 1, "the text-only call's 1.64x decides");
    Ok(())
}

// TEST: CORE-U-084
/// An older call without images never lowers a newer call's density: with images on the newest call, the density is
/// the larger of the two. Here a text-only call at 1x in an earlier run, then the last call at a true 1.64x with
/// images the provider counted as the runner did, must compact.
#[tokio::test]
async fn core_u_084_an_older_call_without_images_never_lowers_a_newer_density() -> TestResult {
    let (_, estimated) = next_turn(long_history(ledger)?).await?;
    let mut events = long_history(ledger)?;
    let first = events
        .iter()
        .position(|event| matches!(event, Event::ModelCompleted { .. }))
        .ok_or("no model call")?;
    events.insert(first, usage_on("test", estimated, estimated, 0));
    let images = 4 * u64::from(IMAGE_TOKENS);
    let late = with_last_usage(
        events,
        usage_on("test", estimated * 164 / 100 + images, estimated, images),
    )?;
    let (compacted, _) = next_turn(late).await?;
    assert_eq!(compacted, 1, "the newest call's 1.64x decides");
    Ok(())
}
