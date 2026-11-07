//! Stable fixed prompt slots precede all projected conversation messages.
use super::{Execution, Stop};
use crate::{
    CoreError,
    limits::{DEFAULT_CONTEXT_WINDOW, DEFAULT_OUTPUT_RESERVE},
};
use serde_json::json;
use std::collections::BTreeSet;
use tqa_context::{Budget, Message, Window};
use tqa_llm::{CompletionRequest, ContentPart, Role, ToolCall, ToolSpec};
use tqa_types::{ErrorCode, Event, RouteSnapshot, ServerFrame, ToolStatus};

/// The request's input tokens, at the route's real density (CORE-U-084), and the route's input capacity, with every
/// tool.
pub(in crate::runner) async fn measure(execution: &Execution<'_>) -> Result<(u64, u32), Stop> {
    let count = count(execution, &execution.frames).await?;
    measure_frames(execution, &execution.frames, count)
}
/// [`measure`] while calls of the latest attempt at `context` still await their results. Each is counted as an empty
/// result, so the projection is complete and a result's cap reflects what the step's earlier results took
/// (CTX-E-004).
pub(in crate::runner) async fn measure_pending(
    execution: &Execution<'_>,
    context: &crate::EventContext,
) -> Result<(u64, u32), Stop> {
    let mut frames = execution.frames.clone();
    let last = frames.iter().rev().find_map(|frame| match frame {
        ServerFrame::Event { seq, ts, .. } => Some((*seq, ts.clone())),
        _ => None,
    });
    let pending = super::super::journal::attempts(&frames)
        .into_iter()
        .rev()
        .find(|attempt| &attempt.context == context)
        .map(|attempt| attempt.calls)
        .unwrap_or_default();
    if let Some((mut seq, ts)) = last {
        for call in pending.into_iter().filter(|call| call.result.is_none()) {
            seq = seq
                .next()
                .ok_or(CoreError::Internal("sequence exhausted"))?;
            frames.push(ServerFrame::Event {
                seq,
                run_id: context.run_id.clone(),
                step: context.step,
                ts: ts.clone(),
                event: Event::ToolResult {
                    call_id: call.id,
                    status: ToolStatus::Ok,
                    content: String::new(),
                    target: None,
                    evidence: None,
                },
            });
        }
    }
    let count = count(execution, &frames).await?;
    measure_frames(execution, &frames, count)
}

/// How much more densely the route counts than the byte estimate (CORE-U-084): a step call's real input, less the
/// images it sent, over the runner's estimate of that request as it was built (`Usage.estimated_input`); between one
/// and 3.5. The estimate alone undercounts dense text (digits, identifiers, CJK) by up to 3.5x, which let a true 32k
/// window overflow before compaction began. Scaling the estimate, rather than adding to the real count, keeps what the
/// next request no longer sends out of it: older replayed states dropped (CORE-U-044) and the images, counted once by
/// the caller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::runner) struct Density {
    real: u64,
    estimated: u64,
}

impl Density {
    /// The estimate as it is: no step call recorded its request.
    pub(in crate::runner) const ONE: Self = Self {
        real: 1,
        estimated: 1,
    };

    /// At most 3.5x, the estimator's own bound: it counts a token per 3.5 bytes, and a byte-level tokenizer never
    /// counts more than a token a byte, so a higher figure is a corrupt count, not a denser text. At least one: the
    /// estimate is the floor.
    fn new(real: u64, estimated: u64) -> Self {
        let most = estimated.saturating_mul(7) / 2;
        if estimated == 0 || real <= estimated {
            return Self::ONE;
        }
        Self {
            real: real.min(most),
            estimated,
        }
    }

    /// The larger of two densities.
    fn max(self, other: Self) -> Self {
        let left = u128::from(self.real) * u128::from(other.estimated);
        let right = u128::from(other.real) * u128::from(self.estimated);
        if left >= right { self } else { other }
    }

    /// `tokens` at this density.
    pub(in crate::runner) fn apply(self, tokens: u64) -> u64 {
        let scaled = u128::from(tokens) * u128::from(self.real) / u128::from(self.estimated);
        u64::try_from(scaled).unwrap_or(u64::MAX)
    }
}

/// What the journal says of the route's real count (CORE-U-084).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::runner) enum Count {
    /// No step call reported a real count: the estimate as it is.
    Unreported,
    /// A step call recorded the estimate of the request it sent, so the density is exact: no walk, nothing kept in
    /// memory, the same after a resume.
    Recorded(Density),
    /// A journal from before the estimate was recorded: the last call's real size at its usage frame, less the images
    /// it sent; the earlier rule applies (need = the larger of the estimate and that size plus what was estimated
    /// since), until a step call records its request.
    Older {
        /// The usage frame's index.
        at: usize,
        /// Its real size: input plus the reply's output without reasoning, less the images.
        real: u64,
    },
}

impl Count {
    /// The density requests are built and measured at: an older journal's count lifts the need, not the density.
    pub(in crate::runner) fn density(self) -> Density {
        match self {
            Self::Recorded(density) => density,
            Self::Unreported | Self::Older { .. } => Density::ONE,
        }
    }
}

/// The count a request built on `frames` goes by.
pub(in crate::runner) async fn count(
    execution: &Execution<'_>,
    frames: &[ServerFrame],
) -> Result<Count, Stop> {
    if let Some(density) = recorded(frames, &execution.route().name) {
        return Ok(Count::Recorded(density));
    }
    Ok(match older(frames) {
        Some((at, real)) => Count::Older {
            at,
            real: real.saturating_sub(execution.image_tokens_in(&frames[..=at]).await?),
        },
        None => Count::Unreported,
    })
}

/// The density from the newest step call on `route` (by its immutable name) that recorded its request and reported a
/// real input count. A density holds only on the route that measured it: a person switching the task's route, or a
/// fallback, starts from the estimate until the new route reports, and the earlier route's count is good again when
/// it is back. A compaction's summary call records no estimate, so it is passed over; so is a zero count (a route that
/// reports no usage, or a cut stream). A compaction is no boundary: the density is the route's, and it holds over a
/// shorter history. When the newest call sent images, their tokens are estimated (1,600 each, where a provider may
/// count 85 to 1,600), so its density may read low: then the larger of it and the newest image-free call's is taken.
/// The newest call is never read lower than it is, and an older image-free call never lowers it.
fn recorded(frames: &[ServerFrame], route: &str) -> Option<Density> {
    let mut newest: Option<Density> = None;
    for frame in frames.iter().rev() {
        let ServerFrame::Event {
            event:
                Event::Usage {
                    prompt_tokens,
                    estimated_input: Some(estimated),
                    estimated_images,
                    estimated_route: Some(counted_on),
                    ..
                },
            ..
        } = frame
        else {
            continue;
        };
        if *prompt_tokens == 0 || *estimated == 0 || counted_on != route {
            continue;
        }
        let images = estimated_images.unwrap_or(0);
        let density = Density::new(prompt_tokens.saturating_sub(images), *estimated);
        match newest {
            // The newest call, without images: its density as it is.
            None if images == 0 => return Some(density),
            // The newest call sent images: look on for the newest image-free call.
            None => newest = Some(density),
            Some(with_images) if images == 0 => return Some(with_images.max(density)),
            Some(_) => {}
        }
    }
    newest
}

/// The need after `plan`'s compaction, at the route's density now: the journal with the plan's own summary in place,
/// measured as the next request would be.
pub(in crate::runner) async fn measure_after(
    execution: &Execution<'_>,
    plan: &tqa_context::CompactionPlan,
) -> Result<u64, Stop> {
    let mut frames = execution.frames.clone();
    let last = frames.iter().rev().find_map(|frame| match frame {
        ServerFrame::Event { seq, ts, .. } => Some((*seq, ts.clone())),
        _ => None,
    });
    if let Some((seq, ts)) = last {
        frames.push(ServerFrame::Event {
            seq: seq
                .next()
                .ok_or(CoreError::Internal("sequence exhausted"))?,
            run_id: execution.context.run_id.clone(),
            step: execution.context.step,
            ts,
            event: Event::Compaction {
                through_seq: plan.through_seq,
                source: tqa_types::CompactionSource::Fallback,
                version: tqa_types::COMPACTION_VERSION,
                summary: plan.fallback.clone(),
            },
        });
    }
    // Counted on the journal as it would be: an older count ends at the compaction (its size was the history's before
    // it), while a recorded density holds through it.
    let count = count(execution, &frames).await?;
    Ok(measure_frames(execution, &frames, count)?.0)
}

/// The need and the input capacity. At a recorded density, the estimate scaled by it, older states dropped at it. For
/// an older journal, the larger of the estimate and the last call's real size plus what was estimated since: its own
/// reply is already in the real size.
fn measure_frames(
    execution: &Execution<'_>,
    frames: &[ServerFrame],
    count: Count,
) -> Result<(u64, u32), Stop> {
    let budget = budget(execution)?;
    let density = count.density();
    let estimated = density.apply(estimate(execution, frames, density)?);
    let needed = match count {
        Count::Older { at, real } => {
            let added =
                estimated.saturating_sub(estimate(execution, &frames[..=at], Density::ONE)?);
            estimated.max(real.saturating_add(added))
        }
        Count::Unreported | Count::Recorded(_) => estimated,
    };
    Ok((needed, budget.input_capacity()))
}

/// The byte estimate of a request built on `frames`, older replayed states dropped while the need at `density` passes
/// where compaction would begin; the latest turn's states left out, as they always go (CORE-U-044).
fn estimate(
    execution: &Execution<'_>,
    frames: &[ServerFrame],
    density: Density,
) -> Result<u64, Stop> {
    let runner = execution.runner;
    let all: BTreeSet<String> = runner
        .tools
        .offered_specs()
        .map(|spec| spec.name.as_str().into())
        .collect();
    let (_, tools_json) = tools(execution, &all)?;
    let system = system(execution);
    let budget = budget(execution)?;
    let (history, latest) = super::states::sendable(
        tqa_context::project(frames)?,
        execution.route(),
        states_limit(&budget),
        |history| {
            density.apply(Window::needed_tokens(
                &system,
                &runner.prompt.instructions,
                &tools_json,
                history,
            ))
        },
        |tokens| density.apply(tokens),
    );
    let needed = Window::needed_tokens(&system, &runner.prompt.instructions, &tools_json, &history);
    // The latest turn's states always go, so they never call for a compaction on their own (CORE-U-044).
    Ok(needed.saturating_sub(latest))
}

/// An older journal's last call, at its usage frame (a usage that recorded its request is c16's, read by [`recorded`]
/// or, on another route, not at all): the request's reported input tokens plus its reply's output
/// tokens without reasoning (replayed reasoning is counted by the estimate's states). `None` when no call reported
/// usage since the history last changed shape: a compaction folds what that call was sent (and its own summary call's
/// usage sits just before it), and a fallback route counts tokens another way.
fn older(frames: &[ServerFrame]) -> Option<(usize, u64)> {
    frames
        .iter()
        .enumerate()
        .rev()
        .map_while(|(at, frame)| match frame {
            ServerFrame::Event {
                event: Event::Compaction { .. } | Event::RouteFallback { .. },
                ..
            } => None,
            ServerFrame::Event {
                event:
                    Event::Usage {
                        prompt_tokens,
                        completion_tokens,
                        reasoning_tokens,
                        estimated_input: None,
                        ..
                    },
                ..
            } => Some((*prompt_tokens > 0).then(|| {
                let reply = completion_tokens.saturating_sub(reasoning_tokens.unwrap_or(0));
                (at, prompt_tokens.saturating_add(reply))
            })),
            _ => Some(None),
        })
        .flatten()
        .next()
}

/// The need beyond which older replayed states are dropped: where compaction would begin, so they go before any turn
/// is folded (the lead's decision (a), CORE-U-044).
fn states_limit(budget: &Budget) -> u64 {
    u64::from(budget.input_capacity()).saturating_mul(tqa_types::COMPACT_AT_PERCENT) / 100
}
/// The window's plan: the route's Max output, or with none the documented reserve (planning only: no limit is sent,
/// CORE-U-071); always within the run's remaining budget.
fn budget(execution: &Execution<'_>) -> Result<Budget, Stop> {
    let route = execution.route();
    Ok(Budget {
        context_window: route.context_window.unwrap_or(DEFAULT_CONTEXT_WINDOW),
        reserve_output: route
            .max_output
            .unwrap_or(DEFAULT_OUTPUT_RESERVE)
            .min(execution.remaining_output()?),
        guessed: route.context_window.is_none(),
    })
}

/// The budget a request is built with: a set Max output is a cap, not a need, so when the prompt leaves less than
/// it, the request asks for what the window leaves, down to the documented reserve (CORE-U-073). Compaction still
/// plans with the whole limit (it acts first, when it can); only a request that then still doesn't fit fails.
fn fitted(mut budget: Budget, needed: u64) -> Budget {
    let window = Budget {
        reserve_output: 0,
        ..budget
    }
    .input_capacity();
    let left = u32::try_from(u64::from(window).saturating_sub(needed)).unwrap_or(u32::MAX);
    let floor = DEFAULT_OUTPUT_RESERVE.min(budget.reserve_output);
    if left < budget.reserve_output {
        budget.reserve_output = left.max(floor);
    }
    budget
}

/// `budget` with room for `tokens` more input, which the window then never refuses (a guessed window counts three
/// quarters of its size).
fn with_room_for(budget: Budget, tokens: u64) -> Budget {
    let extra = if budget.guessed {
        tokens.saturating_mul(4).div_ceil(3)
    } else {
        tokens
    };
    Budget {
        context_window: budget
            .context_window
            .saturating_add(u32::try_from(extra).unwrap_or(u32::MAX)),
        ..budget
    }
}

/// The window's input capacity once the request's output is narrowed to the documented reserve (or less, when the run
/// or the route allows less): what a request can hold at most before it fails (CORE-U-073).
pub(in crate::runner) fn capacity_at_floor(execution: &Execution<'_>) -> Result<u32, Stop> {
    let mut budget = budget(execution)?;
    budget.reserve_output = DEFAULT_OUTPUT_RESERVE.min(budget.reserve_output);
    Ok(budget.input_capacity())
}

/// The output limit a request names: the route's Max output, narrowed to what the window leaves, or none, so the
/// model server's own applies (CORE-U-071; a Messages route always sets one).
fn requested_output(execution: &Execution<'_>, budget: &Budget) -> Option<u32> {
    execution
        .route()
        .max_output
        .map(|limit| limit.min(budget.reserve_output))
}
fn tools(
    execution: &Execution<'_>,
    subset: &BTreeSet<String>,
) -> Result<(Vec<ToolSpec>, String), Stop> {
    let tools: Vec<_> = execution
        .runner
        .tools
        .offered_specs()
        .filter(|spec| subset.contains(spec.name.as_str()))
        .map(|spec| ToolSpec {
            name: execution.runner.names.to_model(spec.name.as_str()).into(),
            description: spec.description.clone(),
            parameters: spec.parameters.clone(),
            // Only a schema that meets the strict rules is sent strict: a strict endpoint refuses the whole request
            // over one tool that breaks them (CORE-U-051).
            strict: tqa_types::strict_compatible(&spec.parameters),
        })
        .collect();
    let definitions: Vec<_> = tools.iter().map(|tool| json!({"name":tool.name,"description":tool.description,"parameters":tool.parameters,"strict":tool.strict})).collect();
    let tools_json = serde_json::to_string(&definitions)
        .map_err(|_| CoreError::Internal("cannot encode tool definitions"))?;
    Ok((tools, tools_json))
}
/// A text request that fits the window, with what the image step needs to extend it.
pub(super) struct Built {
    pub request: CompletionRequest,
    /// The active run's user message: its index in `request.messages` and its attachments.
    pub current: Option<(usize, Vec<String>)>,
    /// Estimated input tokens of the text request.
    pub needed: u64,
    /// The route's input capacity in tokens.
    pub capacity: u32,
}
/// `declare` sends the tools natively on a text-protocol route too: the one retry after a swallowed reply (CORE-U-065).
pub(super) fn build(
    execution: &Execution<'_>,
    subset: &BTreeSet<String>,
    declare: bool,
    density: Density,
) -> Result<Built, Stop> {
    let runner = execution.runner;
    let (tools, tools_json) = tools(execution, subset)?;
    let system = system(execution);
    let fallback = execution.is_fallback();
    let turn = tqa_context::project_turn(&execution.frames)?;
    let planned = budget(execution)?;
    // Older states go while the need at the route's density passes where compaction would begin, as the measure
    // counted them (CORE-U-084), so a request sends what the trigger saw.
    let (history, latest) = super::states::sendable(
        turn.messages,
        execution.route(),
        states_limit(&planned),
        |history| {
            density.apply(Window::needed_tokens(
                &system,
                &runner.prompt.instructions,
                &tools_json,
                history,
            ))
        },
        |tokens| density.apply(tokens),
    );
    let needed = Window::needed_tokens(&system, &runner.prompt.instructions, &tools_json, &history);
    // The latest turn's states narrow the output like any input (CORE-U-074), but never fail the request on their own:
    // the provider needs them for that turn's calls (CORE-U-044).
    // The output narrows to what the input leaves at the route's density, so input and output fit the real window
    // (CORE-U-084); the window itself still counts estimated tokens.
    let budget = fitted(planned, density.apply(needed));
    let room = with_room_for(budget, latest);
    // With native tool calls the catalogue travels once, in `tools`; the window still counts it (CORE-U-053).
    let assemble = if fallback {
        Window::assemble
    } else {
        Window::assemble_native
    };
    let window = assemble(
        &system,
        &runner.prompt.instructions,
        &tools_json,
        &history,
        room,
    )
    .map_err(|_| window_exceeded())?;
    let request = CompletionRequest {
        messages: window
            .messages()
            .iter()
            .map(|item| {
                if fallback {
                    super::fallback::render(item, &execution.runner.names)
                } else {
                    message(item, execution.route(), &execution.runner.names)
                }
            })
            .collect(),
        // On a route without native tool calls the definitions travel only in the tools slot, unless this is the one
        // retry that declares them.
        tools: if fallback && !declare {
            Vec::new()
        } else {
            tools
        },
        declare_tools: fallback && declare,
        max_output: requested_output(execution, &budget),
        reasoning_effort: execution
            .route()
            .reasoning
            .as_ref()
            .map(|reasoning| reasoning.default.clone()),
        ..CompletionRequest::default()
    };
    // The window places the three fixed slots before the history.
    let current = turn
        .current_user
        .map(|index| (index + FIXED_SLOTS, turn.attachments));
    Ok(Built {
        request,
        current,
        needed,
        capacity: budget.input_capacity(),
    })
}
/// The system slot, with the text-protocol instructions on routes without native tool calls.
fn system(execution: &Execution<'_>) -> String {
    let prompt = &execution.runner.prompt.system;
    match tqa_prompt::fallback_instructions(execution.route().native_tools) {
        Some(instructions) => format!("{prompt}\n\n{instructions}"),
        None => prompt.clone(),
    }
}
/// System prompt, instruction files and tool definitions (an empty slot on native-tool routes) precede the history.
const FIXED_SLOTS: usize = 3;
pub(super) fn window_exceeded() -> Stop {
    Stop::Failed(ErrorCode::Budget, "context window exceeded".into())
}
/// One projected message as a provider message; an assistant response carries its replayable
/// reasoning states first (ADR-0027).
fn message(
    message: &Message,
    route: &RouteSnapshot,
    names: &tqa_tools::ModelNames,
) -> tqa_llm::Message {
    let role = match message.role {
        tqa_context::Role::System => Role::System,
        tqa_context::Role::User => Role::User,
        tqa_context::Role::Assistant => Role::Assistant,
        tqa_context::Role::Tool => Role::Tool,
    };
    tqa_llm::Message {
        role,
        content: super::states::replayable(&message.reasoning_states, route)
            .map(|item| ContentPart::ReasoningState(item.to_owned()))
            .chain([ContentPart::Text(message.text.clone())])
            .collect(),
        tool_calls: message
            .tool_calls
            .iter()
            .map(|call| ToolCall {
                id: call.call_id.to_string(),
                name: names.to_model(&call.name).into(),
                arguments: call.arguments.clone(),
            })
            .collect(),
        tool_call_id: message.tool_call_id.as_ref().map(ToString::to_string),
        name: None,
    }
}
