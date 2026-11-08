//! CORE-U-084: compaction triggers on the last call's real token count where the byte estimate undercounts.
//! Dense text (ledger digits and identifiers, CJK) takes more tokens than 3.5 bytes each: a Qwen model counts 1501
//! tokens for 3199 bytes of ledger lines, 1.64x the estimate, so on a true 32k window the request overflowed before
//! compaction began.
mod ledger_history;
mod support;
use async_trait::async_trait;
use futures::StreamExt as _;
use ledger_history::*;
use serde_json::json;
use std::{
    error::Error,
    sync::{Arc, Mutex},
};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner, UserTurn};
use tqa_llm::{
    CompletionRequest, CompletionStream, FinishReason, LlmError, ModelProvider, Route, StreamChunk,
};
use tqa_testkit::{FakeProvider, ProviderReply};
use tqa_types::{CompactionSource, Event, FinishReason as WireFinish, IMAGE_TOKENS, WireProtocol};

// TEST: CORE-U-084
/// Ledger lines: the estimate stays under the threshold, but the last call reported 1.64x its estimate, as a Qwen
/// model counts such lines. Without that usage nothing is compacted, as before; with it, the next turn compacts.
#[tokio::test]
async fn core_u_084_a_real_count_of_1_64x_the_estimate_compacts() -> TestResult {
    let (compacted, estimated) = next_turn(long_history(ledger)?).await?;
    assert_eq!(
        compacted, 0,
        "no usage reported: the estimate alone decides"
    );
    assert!(
        estimated < THRESHOLD,
        "the estimate must stay under: {estimated}"
    );
    let real = estimated * 164 / 100;
    assert!(real > THRESHOLD, "1.64x must pass the threshold: {real}");
    let (compacted, _) = next_turn(with_last_usage(
        long_history(ledger)?,
        older_usage(real, 5),
    )?)
    .await?;
    assert_eq!(compacted, 1);
    Ok(())
}

// TEST: CORE-U-084
/// CJK text: three bytes a character, about a token a character, so the real count is about 7/6 of the estimate.
#[tokio::test]
async fn core_u_084_cjk_text_compacts_on_its_real_count() -> TestResult {
    let (compacted, estimated) = next_turn(long_history(cjk)?).await?;
    assert_eq!(compacted, 0);
    assert!(
        estimated < THRESHOLD,
        "the estimate must stay under: {estimated}"
    );
    let real = estimated * 7 / 6;
    assert!(real > THRESHOLD, "7/6 must pass the threshold: {real}");
    let (compacted, _) =
        next_turn(with_last_usage(long_history(cjk)?, older_usage(real, 5))?).await?;
    assert_eq!(compacted, 1);
    Ok(())
}

// TEST: CORE-U-084
/// The reply counts its reported output tokens, not its estimate ("answer 16" is about 3): an input just under the
/// threshold compacts only when the reply's real size takes it over.
#[tokio::test]
async fn core_u_084_the_reply_counts_its_reported_output_tokens() -> TestResult {
    let under = THRESHOLD - 150;
    let (compacted, _) = next_turn(with_last_usage(
        long_history(ledger)?,
        older_usage(under, 5),
    )?)
    .await?;
    assert_eq!(compacted, 0, "a short reply keeps it under");
    let (compacted, _) = next_turn(with_last_usage(
        long_history(ledger)?,
        older_usage(under, 400),
    )?)
    .await?;
    assert_eq!(compacted, 1, "400 output tokens take it over");
    Ok(())
}

// TEST: CORE-U-084
/// Reasoning the provider reports separately is left to the estimate's states (CORE-U-044), not counted twice.
#[tokio::test]
async fn core_u_084_reported_reasoning_is_not_counted_as_the_reply() -> TestResult {
    let under = THRESHOLD - 150;
    let reasoning = Event::Usage {
        prompt_tokens: under,
        completion_tokens: 400,
        cached_tokens: None,
        reasoning_tokens: Some(395),
        estimated: false,
        estimated_input: None,
        estimated_images: None,
        estimated_route: None,
    };
    let (compacted, _) = next_turn(with_last_usage(long_history(ledger)?, reasoning)?).await?;
    assert_eq!(compacted, 0);
    Ok(())
}

// TEST: CORE-U-084
/// A count from before a compaction keeps its density, never its size: the shorter history after it is measured at
/// that density, so a count that passed the threshold before folds nothing more. A count from before a route fallback
/// is another route's: ignored.
#[tokio::test]
async fn core_u_084_a_stale_count_is_ignored() -> TestResult {
    let real = THRESHOLD + 200;
    // Compacted after the reported call: the summary replaced the first five runs.
    let mut compacted_after = with_last_usage(long_history(ledger)?, older_usage(real, 5))?;
    let through = compacted_after
        .iter()
        .enumerate()
        .filter(|(_, event)| matches!(event, Event::RunCompleted { .. }))
        .nth(4)
        .map(|(at, _)| at + 1)
        .ok_or("no fifth run")?;
    // A compaction opens the run after it, before that run's first step; that run reports no usage.
    let seq = compacted_after.len() + 1;
    let run_id: tqa_types::RunId = format!("r-{seq}").parse()?;
    compacted_after.extend([
        Event::MessageUser {
            lead_brief: None,
            team_wake: None,
            text: "one more request".into(),
            attachments: vec![],
        },
        Event::RunStarted {
            run_id: run_id.clone(),
        },
        Event::Compaction {
            through_seq: tqa_types::Seq::from(u64::try_from(through)?),
            source: CompactionSource::Fallback,
            version: 1,
            summary: model_summary(),
        },
        Event::StepStarted {
            step: 1,
            attempt: 1,
        },
        Event::TextDelta {
            text: "done".into(),
        },
        Event::ModelCompleted {
            step: 1,
            attempt: 1,
            finish_reason: WireFinish::Stop,
        },
        Event::StepCompleted {
            step: 1,
            duration_ms: 0,
        },
        Event::RunCompleted { run_id },
    ]);
    let (compacted, _) = next_turn(compacted_after).await?;
    assert_eq!(compacted, 0, "after a compaction");
    // A fallback route after the reported call, whose count would compact: ignored.
    let mut events = with_last_usage(long_history(ledger)?, older_usage(THRESHOLD * 2, 5))?;
    let last_completed = events
        .iter()
        .rposition(|event| matches!(event, Event::ModelCompleted { .. }))
        .ok_or("no model call")?;
    events.insert(
        last_completed,
        Event::RouteFallback {
            from: "local".into(),
            to: "local-text".into(),
            reason: "unavailable".into(),
        },
    );
    let (compacted, _) = next_turn(events).await?;
    assert_eq!(compacted, 0, "after a route fallback");
    Ok(())
}

// TEST: CORE-U-084
/// A reply whose text went on after its usage chunk journals `estimated` (its output is partly estimated), but its
/// input count is the provider's own: it still gives the density. Only a zero input count is no count (below).
#[tokio::test]
async fn core_u_084_a_reply_estimated_after_its_usage_keeps_its_real_input() -> TestResult {
    let (_, estimated) = next_turn(long_history(ledger)?).await?;
    let usage = Event::Usage {
        prompt_tokens: estimated * 164 / 100,
        completion_tokens: 5,
        cached_tokens: None,
        reasoning_tokens: None,
        estimated: true,
        estimated_input: Some(estimated),
        estimated_images: Some(0),
        estimated_route: Some("test".into()),
    };
    let (compacted, _) = next_turn(with_last_usage(long_history(ledger)?, usage)?).await?;
    assert_eq!(compacted, 1, "the real input count decides");
    Ok(())
}

// TEST: CORE-U-084
/// A provider that reports no usage journals an estimated count with no input tokens: the estimate decides, as before.
#[tokio::test]
async fn core_u_084_an_unreported_count_leaves_the_estimate_alone() -> TestResult {
    let unreported = Event::Usage {
        prompt_tokens: 0,
        completion_tokens: THRESHOLD * 2,
        cached_tokens: None,
        reasoning_tokens: None,
        estimated: true,
        estimated_input: None,
        estimated_images: None,
        estimated_route: None,
    };
    let (compacted, _) = next_turn(with_last_usage(long_history(ledger)?, unreported)?).await?;
    assert_eq!(compacted, 0);
    Ok(())
}

// TEST: CORE-U-084
// TEST: CORE-U-044
/// Older replayed states go before any turn is folded on the real count too: a real count past the threshold that
/// dropping older states brings back under drops them and folds nothing.
#[tokio::test]
async fn core_u_084_states_go_before_turns_on_the_real_count() -> TestResult {
    const STATE_RUNS: usize = 12;
    let responses = |h: &mut Harness| -> Result<(), Box<dyn Error>> {
        h.deps.route.protocol = WireProtocol::OpenAiResponses;
        Ok(())
    };
    let events = || with_states(long_history_of(ledger, STATE_RUNS)?, 240);
    let (compacted, requests) = next_turn_with(events()?, responses, &[]).await?;
    assert_eq!(compacted, 0, "without usage, states and all stay under");
    let answer = requests.last().ok_or("no request")?;
    assert_eq!(
        states_sent(answer),
        STATE_RUNS,
        "every state goes while it fits"
    );
    // The last call sent every state at the estimate's density and was counted past the threshold.
    let real = THRESHOLD + 200;
    let sent = full_tokens(answer);
    let (compacted, requests) = next_turn_with(
        with_last_usage(events()?, recorded_usage(real, 5, sent))?,
        responses,
        &[],
    )
    .await?;
    assert_eq!(
        compacted, 0,
        "dropping older states brings the real count under"
    );
    let answer = requests.last().ok_or("no request")?;
    assert!(
        states_sent(answer) < STATE_RUNS,
        "older states were dropped"
    );
    Ok(())
}

// TEST: CORE-U-084
// TEST: CORE-U-081
/// The current turn's image counts once: the last call's real count holds the image that call sent, so it isn't added
/// to the image the next request sends.
#[tokio::test]
async fn core_u_084_an_image_counts_once() -> TestResult {
    // The longest history that, with the current turn's image, still stays under without usage.
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
    let image = u64::from(IMAGE_TOKENS);
    assert!(
        text + 2 * image > THRESHOLD,
        "counted twice, the image would pass the threshold: {text}"
    );
    // The previous call sent the history's text and its image, at the estimate's density.
    let real = text + image;
    let (compacted, _) = next_turn_with(
        with_last_usage(with_image(runs)?, older_usage(real, 5))?,
        vision,
        &["uploads/a.png"],
    )
    .await?;
    assert_eq!(compacted, 0, "the image counted once stays under");
    Ok(())
}

/// A route that counts `tenths / 10` tokens for every estimated one, and says so in each reply's usage, as a provider
/// reports its real count. Step calls and summary calls (no tools) have their own scripts; it keeps each step
/// request's true size.
struct Dense {
    steps: Arc<FakeProvider>,
    summaries: Arc<FakeProvider>,
    tenths: u64,
    sizes: Mutex<Vec<u64>>,
}
#[async_trait]
impl ModelProvider for Dense {
    fn route(&self) -> &Route {
        self.steps.route()
    }
    async fn complete(
        &self,
        request: CompletionRequest,
        cancel: CancellationToken,
    ) -> Result<CompletionStream, LlmError> {
        // The latest assistant message's states always go and never call for a compaction (CORE-U-044, the lead's
        // decision (a)), so they are left out of what this checks; everything else must fit.
        // A provider counts everything it was sent, the latest states too; that is what its usage reports.
        let size = full_tokens(&request) * self.tenths / 10;
        let summary = request.tools.is_empty();
        if !summary {
            // What the server is asked to hold: the input at its real size and the output the request allows, less
            // the latest states, which the check exempts.
            let output = request.max_output.map_or(0, u64::from);
            let checked =
                full_tokens(&request).saturating_sub(latest_states(&request)) * self.tenths / 10;
            self.sizes
                .lock()
                .map_err(|_| LlmError::Cancelled)?
                .push(checked + output);
        }
        let stream = if summary {
            self.summaries.complete(request, cancel).await?
        } else {
            self.steps.complete(request, cancel).await?
        };
        Ok(Box::pin(stream.map(move |chunk| match chunk {
            Ok(StreamChunk::Usage(mut usage)) => {
                usage.prompt_tokens = size;
                Ok(StreamChunk::Usage(usage))
            }
            other => other,
        })))
    }
}

/// A step that streams one replayable state of `padding` bytes, then reads part `step`.
fn stateful_read(step: usize, padding: usize) -> ProviderReply {
    let item = json!({"type": "reasoning", "id": format!("rs_{step}"), "encrypted_content": "x".repeat(padding)})
        .to_string();
    reply(vec![
        StreamChunk::ReasoningDelta("plan".into()),
        StreamChunk::ReasoningState(item),
        StreamChunk::ToolCallDelta {
            index: 0,
            id: Some(format!("c{step}")),
            name: Some("read_file".into()),
            arguments_delta: format!(r#"{{"path":"ledger/part-{step:02}.txt"}}"#),
        },
        support::usage(),
        StreamChunk::Finish(FinishReason::ToolCalls),
    ])
}

/// The largest a step request of a 30-step run asks the server to hold, on a route at `tenths / 10` with states of
/// `padding` bytes, and how many compactions the run made.
async fn largest_request(tenths: u64, padding: usize) -> Result<(u64, usize), Box<dyn Error>> {
    const STEPS: usize = 30;
    let script: Vec<ProviderReply> = (0..STEPS)
        .map(|step| stateful_read(step, padding))
        .chain(std::iter::once(answer()))
        .collect();
    let outputs = (0..STEPS)
        .map(|step| Ok(output(&ledger(step)?, 0)))
        .collect::<Result<Vec<_>, std::fmt::Error>>()?;
    let tool = fake("read_file", true, outputs)?;
    let mut h = Harness::new(script, registry(&[tool])?)?;
    h.deps.route.context_window = Some(WINDOW);
    h.deps.route.max_output = Some(RESERVE);
    h.deps.route.protocol = WireProtocol::OpenAiResponses;
    h.deps.limits.max_output_tokens = 1_000_000;
    let summaries = (0..STEPS)
        .map(|_| summary_reply())
        .collect::<Result<Vec<_>, _>>()?;
    let dense = Arc::new(Dense {
        steps: h.provider.clone(),
        summaries: Arc::new(FakeProvider::new(
            h.provider.route().clone(),
            summaries,
            Arc::new(h.sleeper.clone()),
        )),
        tenths,
        sizes: Mutex::new(Vec::new()),
    });
    h.deps.provider = dense.clone();
    let sink = h.sink.clone();
    let turn = UserTurn {
        lead_brief: None,
        team_wake: None,
        text: "audit the ledger parts".into(),
        attachments: vec![],
    };
    let outcome = Runner::new(h.deps)?
        .run(
            &[],
            Some(turn),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert_eq!(
        outcome,
        RunOutcome::Completed,
        "{tenths}/10, padding {padding}"
    );
    let largest = dense
        .sizes
        .lock()
        .map_err(|_| "lock")?
        .iter()
        .copied()
        .max()
        .unwrap_or(0);
    Ok((largest, compactions(&sink.events())))
}

// TEST: CORE-U-084
// TEST: CORE-U-044
/// The density each request was built at, from the journal the runner itself wrote: on a route that counts up to 3.5x
/// the estimate and replays states, no step request asks the server for more than its window (its input at its real
/// size plus the output it allows), whatever the states' size. The review of 7 Oct's sweep, made
/// self-consistent: every call reports its real count, as the runner's own journal would hold it.
#[tokio::test]
async fn core_u_084_no_request_passes_the_window_at_a_real_density() -> TestResult {
    let capacity = u64::from(WINDOW);
    let mut worst = (0_i64, 0_u64, 0_usize);
    let cases = [11_u64, 14, 17, 20, 26, 35]
        .into_iter()
        .flat_map(|tenths| [300_usize, 800, 1300].map(|padding| (tenths, padding)));
    for (tenths, padding) in cases {
        let over =
            i64::try_from(largest_request(tenths, padding).await?.0)? - i64::try_from(capacity)?;
        if over > worst.0 {
            worst = (over, tenths, padding);
        }
    }
    assert!(
        worst.0 <= 0,
        "a request passed the window by {} tokens at {}/10 with {}-byte states",
        worst.0,
        worst.1,
        worst.2
    );
    Ok(())
}

/// One run as a long conversation journals it: a usage before each `model.completed`, and every 20 runs a compaction
/// opening the run, folded through the run before (`through`).
fn reported_run(run: Vec<Event>, index: usize, through: u64) -> Vec<Event> {
    let mut events = Vec::new();
    for event in run {
        if matches!(event, Event::ModelCompleted { .. }) {
            events.push(older_usage(400 + index as u64, 5));
        }
        let compacts = matches!(event, Event::RunStarted { .. }) && index % 20 == 1 && index > 1;
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
    events
}

// TEST: CORE-U-084
/// A long journal from before c16 (no recorded estimates): a usage on every model call and a compaction every 20 runs.
/// Its next turn starts as fast as a short one: the earlier rule reads one count, back to the last compaction. (A
/// density recomputed by walking every call took 2.39 s here at 800 calls, debug; the recorded density's own cost is
/// `compaction_density.rs`'s.)
#[tokio::test]
async fn core_u_084_the_density_walk_stays_short_on_a_long_conversation() -> TestResult {
    const RUNS: usize = 400;
    let mut events = Vec::new();
    for index in 1..=RUNS {
        let run = completed_run(index, events.len() + 1, ledger(index)?)?;
        let through = u64::try_from(events.len())?;
        events.extend(reported_run(run, index, through));
    }
    let started = std::time::Instant::now();
    next_turn_with(events, |_| Ok(()), &[]).await?;
    let took = started.elapsed();
    let bound = tqa_testkit::deadline::scaled(std::time::Duration::from_secs(1));
    assert!(
        took <= bound,
        "the next turn took {took:?} with 800 reported calls (bound {bound:?})"
    );
    Ok(())
}

// TEST: CORE-U-084
// TEST: CORE-U-073
/// An older journal's count ends at the compaction it plans, as the earlier rule's counts always did: the planner judges
/// the plan by the estimate after it, so the compaction keeps its usual turns. Carrying the count's absolute size past
/// the plan made every plan look over the threshold, and the fewest turns were always kept.
#[tokio::test]
async fn core_u_084_an_older_count_ends_at_the_compaction_it_plans() -> TestResult {
    let (_, estimated) = next_turn(long_history(ledger)?).await?;
    let events = with_last_usage(long_history(ledger)?, older_usage(estimated * 164 / 100, 5))?;
    let (written, requests) = next_turn_journal(events, |_| Ok(()), &[]).await?;
    assert_eq!(compactions(&written), 1, "the older count compacts");
    // The usual plan keeps the last run whole, as a compaction on the estimate alone does here; the fewest turns
    // would fold even its request.
    let answer = requests.last().ok_or("no request")?;
    let last = format!("request {RUNS}");
    let kept = answer.messages.iter().any(|message| {
        message
            .content
            .iter()
            .any(|part| matches!(part, tqa_llm::ContentPart::Text(text) if *text == last))
    });
    assert!(
        kept,
        "the usual turns are kept, not the fewest: {last} is gone"
    );
    Ok(())
}

// TEST: CORE-U-084
// TEST: CORE-U-073
/// Room, not a step (the thrash at 2.6x on an 8k window, 7 Oct 2026): a compaction keeps the most turns that bring the
/// need to the low-water mark (70 %), or the fewest; one that can't bring it under the threshold while the request
/// still fits waits until it doesn't. A 30-step run compacted on 23 of its steps; now 6 to 8, and every request
/// still fits the window (the sweep above).
#[tokio::test]
async fn core_u_084_a_compaction_that_cannot_make_room_waits() -> TestResult {
    for (tenths, padding) in [(26_u64, 300_usize), (26, 800), (35, 300)] {
        let (_, compacted) = largest_request(tenths, padding).await?;
        assert!(
            compacted <= 10,
            "{compacted} compactions in 30 steps at {tenths}/10 with {padding}-byte states"
        );
    }
    Ok(())
}

/// A page of `bytes` bytes of text, distinct per run (identical results would be deduplicated, CTX-U-012).
fn page(index: usize, bytes: usize) -> String {
    let mut text = format!("page {index:03} ");
    while text.len() < bytes {
        let line = format!("line {:05} of the report ", text.len() * 7 + index);
        text.push_str(&line);
    }
    text.truncate(bytes);
    text
}

// TEST: CORE-U-073
/// The low-water mark holds at any density (decided on 7 Oct 2026: one rule). At 1x, with no usage reported, four
/// runs of large pages (about 2,400 estimated tokens each) pass the threshold; the usual six turns would keep two pages,
/// landing between 70 % and 85 %, where the next step would cross 85 % again. So the compaction keeps fewer turns: one
/// page, under 70 %.
#[tokio::test]
async fn core_u_073_a_compaction_aims_at_the_low_water_mark_at_any_density() -> TestResult {
    const PAGE_BYTES: usize = 9_200;
    let mut events = Vec::new();
    for index in 1..=4 {
        let seq = events.len() + 1;
        events.extend(completed_run(index, seq, page(index, PAGE_BYTES))?);
    }
    let (compacted, requests) = next_turn_with(events, |_| Ok(()), &[]).await?;
    assert_eq!(compacted, 1);
    let answer = requests.last().ok_or("no request")?;
    let pages = answer
        .messages
        .iter()
        .filter(|message| {
            message.content.iter().any(|part| {
                matches!(part, tqa_llm::ContentPart::Text(text) if text.starts_with("page "))
            })
        })
        .count();
    let tokens = request_tokens(answer);
    assert_eq!(
        pages, 1,
        "one page kept, not the usual two ({tokens} tokens)"
    );
    assert!(
        tokens * 100 <= u64::from(WINDOW - RESERVE) * tqa_types::COMPACT_LOW_WATER_PERCENT,
        "{tokens} tokens"
    );
    Ok(())
}
