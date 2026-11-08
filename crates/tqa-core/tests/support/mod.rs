//! Synthetic loop inputs and an append-only in-memory sink.
#![allow(
    dead_code,
    reason = "Integration suites share a fixture vocabulary and select relevant helpers."
)]
use async_trait::async_trait;
use serde_json::{Value, json};
use std::{
    error::Error,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use tqa_core::{
    EventContext, EventSink, ModelDecides, PromptInputs, RunnerDeps, SinkError, UserTurn,
};
use tqa_llm::{FinishReason, LlmError, Registry, StreamChunk, Usage};
use tqa_testkit::{
    FakeClock, FakeProvider, FakeSleeper, FakeTool, FakeToolReply, FixedJitter, ProviderChunk,
    ProviderReply,
};
use tqa_tools::{
    ToolError, ToolLimits, ToolName, ToolOutput, ToolRegistry, ToolSpec, WorkspaceRoot,
};
use tqa_types::{ErrorCode, Event, RouteSnapshot, RunLimits, Seq, ServerFrame, WireProtocol};
pub type TestResult = Result<(), Box<dyn Error>>;

pub struct MemorySink {
    frames: Mutex<Vec<ServerFrame>>,
    initial: usize,
    pub cut: Option<(usize, CancellationToken)>,
    pub fail_at: Option<usize>,
    pub sequence_base: Seq,
    /// Woken on every appended frame, so a test waits for what it expects instead of counting polls.
    appended: tokio::sync::Notify,
}
impl MemorySink {
    pub fn new(history: &[ServerFrame]) -> Self {
        Self {
            frames: Mutex::new(history.to_vec()),
            initial: history.len(),
            cut: None,
            fail_at: None,
            sequence_base: Seq::ZERO,
            appended: tokio::sync::Notify::new(),
        }
    }
    pub fn frames(&self) -> Vec<ServerFrame> {
        self.frames
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
    pub fn events(&self) -> Vec<Event> {
        self.frames()
            .into_iter()
            .filter_map(|frame| match frame {
                ServerFrame::Event { event, .. } => Some(event),
                _ => None,
            })
            .collect()
    }
    pub fn types(&self) -> Vec<String> {
        self.events()
            .iter()
            .map(|event| {
                serde_json::to_value(event)
                    .ok()
                    .and_then(|v| v["type"].as_str().map(str::to_owned))
                    .unwrap_or_default()
            })
            .collect()
    }
}
#[async_trait]
impl EventSink for MemorySink {
    async fn emit(&self, event: Event, context: EventContext) -> Result<Seq, SinkError> {
        self.append_frame(|seq| ServerFrame::Event {
            seq,
            run_id: context.run_id,
            step: context.step,
            ts: "2001-01-01T00:00:00Z".into(),
            event,
        })
    }
}
impl MemorySink {
    fn append_frame(&self, build: impl FnOnce(Seq) -> ServerFrame) -> Result<Seq, SinkError> {
        let mut frames = self.frames.lock().unwrap_or_else(PoisonError::into_inner);
        let next = frames.len() - self.initial + 1;
        if self.fail_at == Some(next) {
            return Err(SinkError {
                code: ErrorCode::Io,
            });
        }
        let seq = frames
            .iter()
            .rev()
            .find_map(|frame| match frame {
                ServerFrame::Event { seq, .. } | ServerFrame::PermissionRequest { seq, .. } => {
                    Some(*seq)
                }
                _ => None,
            })
            .unwrap_or(self.sequence_base)
            .next()
            .ok_or(SinkError {
                code: ErrorCode::Internal,
            })?;
        frames.push(build(seq));
        drop(frames);
        self.appended.notify_waiters();
        if let Some((cut, token)) = &self.cut
            && *cut == next
        {
            token.cancel();
        }
        Ok(seq)
    }
}

pub struct Harness {
    pub directory: tempfile::TempDir,
    pub clock: FakeClock,
    pub sleeper: FakeSleeper,
    pub provider: Arc<FakeProvider>,
    pub deps: RunnerDeps,
    pub sink: Arc<MemorySink>,
}
impl Harness {
    pub fn new(script: Vec<ProviderReply>, tools: ToolRegistry) -> Result<Self, Box<dyn Error>> {
        let directory = tempfile::tempdir()?;
        let clock = FakeClock::new();
        let sleeper = FakeSleeper::new(clock.clone());
        let routes = Registry::from_toml(
            "[[routes]]\nname='test'\nprotocol='openai_chat'\nbase_url='http://localhost'\nmodel='fake-model'\nauth={kind='none'}\n",
        )?;
        let provider = Arc::new(FakeProvider::new(
            routes.get("test").ok_or("missing route")?.clone(),
            script,
            Arc::new(sleeper.clone()),
        ));
        let sink = Arc::new(MemorySink::new(&[]));
        let deps = RunnerDeps {
            conversation_id: "conversation".parse()?,
            provider: provider.clone(),
            tools: Arc::new(tools),
            decision: Arc::new(ModelDecides),
            sink: sink.clone(),
            clock: Arc::new(clock.clone()),
            sleeper: Arc::new(sleeper.clone()),
            prompt: PromptInputs {
                system: "system slot".into(),
                instructions: "instruction slot".into(),
                loaded: None,
            },
            spill_dir: directory.path().join(".tqa/conversation/spill"),
            limits: RunLimits {
                max_steps: 50,
                wall_clock_secs: 600,
                tool_result_bytes: 8192,
                ..RunLimits::default()
            },
            route: route(),
            workspace: WorkspaceRoot::open(directory.path())?,
            env_allow: vec![],
            tool_limits: ToolLimits::RECOMMENDED,
            jitter: Box::new(FixedJitter::new(0.5)?),
            fallback_routes: vec![],
            gateway_link: None,
            screenshots: None,
            screen_store: None,
        };
        Ok(Self {
            directory,
            clock,
            sleeper,
            provider,
            deps,
            sink,
        })
    }
    pub fn sink_with(&mut self, sink: MemorySink) {
        self.sink = Arc::new(sink);
        self.deps.sink = self.sink.clone();
    }
}
pub fn route() -> RouteSnapshot {
    RouteSnapshot {
        name: "test".into(),
        protocol: WireProtocol::OpenAiChat,
        model: "fake-model".into(),
        relay_path: "/relay/r/test".into(),
        context_window: Some(32768),
        max_output: Some(4096),
        native_tools: true,
        parallel_tools: true,
        vision: false,
        reasoning: None,
        max_request_bytes: None,
        cache_hint: tqa_types::CacheHint::None,
        first_response_secs: None,
        pixel_grounding: false,
    }
}
pub fn turn() -> UserTurn {
    UserTurn {
        lead_brief: None,
        team_wake: None,
        text: "inspect files".into(),
        attachments: vec!["uploads/attachment".into()],
    }
}
pub fn reply(chunks: Vec<StreamChunk>) -> ProviderReply {
    ProviderReply {
        delay: Duration::ZERO,
        result: Ok(chunks
            .into_iter()
            .map(|chunk| ProviderChunk {
                delay: Duration::ZERO,
                result: Ok(chunk),
            })
            .collect()),
    }
}
pub fn answer() -> ProviderReply {
    reply(vec![
        StreamChunk::TextDelta("final ".into()),
        StreamChunk::TextDelta("answer".into()),
        usage(),
        StreamChunk::Finish(FinishReason::Stop),
    ])
}
pub fn usage() -> StreamChunk {
    StreamChunk::Usage(Usage {
        prompt_tokens: 10,
        completion_tokens: 5,
        cached_tokens: Some(2),
        reasoning_tokens: None,
    })
}
pub fn calls(calls: &[(&str, &str, &str)]) -> ProviderReply {
    let mut chunks: Vec<_> = calls
        .iter()
        .enumerate()
        .map(|(index, (id, name, args))| StreamChunk::ToolCallDelta {
            index: u32::try_from(index).unwrap_or(u32::MAX),
            id: Some((*id).into()),
            name: Some((*name).into()),
            arguments_delta: (*args).into(),
        })
        .collect();
    chunks.extend([usage(), StreamChunk::Finish(FinishReason::ToolCalls)]);
    reply(chunks)
}
pub fn http(status: u16) -> ProviderReply {
    ProviderReply {
        delay: Duration::ZERO,
        result: Err(LlmError::Http {
            status,
            body_excerpt: "private-response-marker".into(),
        }),
    }
}
pub fn spec(name: &str, parallel: bool) -> Result<ToolSpec, ToolError> {
    Ok(ToolSpec {
        name: ToolName::new(name)?,
        description: format!("Test {name}"),
        parameters: json!({"type":"object","required":["path"],"properties":{"path":{"type":"string"}},"additionalProperties":false}),
        parallel_safe: parallel,
        // A fake of a tool that writes files says so, as the real one does (CORE-E-041).
        changes_workspace: matches!(name, "shell" | "write_file" | "edit_file" | "take_files"),
    })
}
pub fn fake(
    name: &str,
    parallel: bool,
    replies: Vec<FakeToolReply>,
) -> Result<Arc<FakeTool>, ToolError> {
    Ok(Arc::new(FakeTool::new(spec(name, parallel)?, replies)))
}
pub fn output(text: &str, delay_ms: u64) -> FakeToolReply {
    FakeToolReply {
        delay: Duration::from_millis(delay_ms),
        result: Ok(ToolOutput::text(text)),
    }
}
pub fn registry(tools: &[Arc<FakeTool>]) -> Result<ToolRegistry, Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    for tool in tools {
        registry.register(tool.clone())?;
    }
    Ok(registry)
}
pub fn event_types(text: &str) -> Vec<String> {
    text.lines().map(str::to_owned).collect()
}
pub fn tool_results(events: &[Event]) -> Vec<(String, tqa_types::ToolStatus, String)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::ToolResult {
                call_id,
                status,
                content,
                ..
            } => Some((call_id.to_string(), *status, content.clone())),
            _ => None,
        })
        .collect()
}
pub fn request_value(request: &tqa_llm::CompletionRequest) -> Value {
    let messages: Vec<_> = request.messages.iter().map(|message| json!({
        "role": message.role, "content":message.content.iter().map(|part| match part { tqa_llm::ContentPart::Text(text)=>text.as_str(), _=>"" }).collect::<Vec<_>>(),
        "calls":message.tool_calls.iter().map(|call|json!([call.id,call.name,call.arguments])).collect::<Vec<_>>(), "call_id":message.tool_call_id,"name":message.name
    })).collect();
    json!({"messages":messages,"tools":request.tools.iter().map(|tool|json!([tool.name,tool.description,tool.parameters,tool.strict])).collect::<Vec<_>>(),"reasoning":request.reasoning_effort,"output":request.max_output,"temperature":request.temperature,"stop":request.stop,"metadata":request.metadata})
}
pub fn assert_closed(frames: &[ServerFrame]) -> TestResult {
    let mut calls = std::collections::HashSet::new();
    let mut terminal = 0;
    for frame in frames {
        if let ServerFrame::Event {
            event,
            run_id,
            step,
            ..
        } = frame
        {
            match event {
                Event::ToolCall { call_id, .. } => {
                    calls.insert((run_id.clone(), *step, call_id.clone()));
                }
                Event::ToolResult { call_id, .. } => {
                    assert!(calls.remove(&(run_id.clone(), *step, call_id.clone())));
                }
                Event::RunCompleted { .. }
                | Event::RunFailed { .. }
                | Event::RunCancelled { .. } => {
                    terminal += 1;
                }
                _ => {}
            }
        }
    }
    assert!(calls.is_empty(), "unanswered: {calls:?}");
    assert_eq!(terminal, 1);
    assert!(
        matches!(tqa_context::resume_point(frames)?, tqa_context::ResumePoint::Idle { dangling } if dangling.is_empty())
    );
    Ok(())
}

pub fn assert_resumed(before: &[ServerFrame], after: &[ServerFrame]) -> TestResult {
    assert_eq!(&after[..before.len()], before);
    assert!(matches!(&after[before.len()], ServerFrame::Event {
        event: Event::RunResumed { run_id, from_step: 1 }, ..
    } if run_id.as_str() == "r-1"));
    for (index, frame) in after.iter().enumerate() {
        assert!(matches!(frame, ServerFrame::Event { seq, .. }
            if *seq == Seq::from(u64::try_from(index + 1)?)));
    }
    Ok(())
}

/// The real time `drive` allows a run at least, scaled with `TQA_TEST_DEADLINE_SCALE` (CORE-U-077).
pub const DRIVE_REAL_FLOOR: Duration = Duration::from_secs(10);

/// Each poll advances fake time by 10 ms, including polls that only wait for a real blocking
/// thread; a test that asserts a tight wall-clock budget is met must not drive across such work.
/// The run ends unfinished only once both bounds have passed: 20,000 such polls (200 s of fake
/// time) and [`DRIVE_REAL_FLOOR`] of real time, so a real thread the run waits on is never cut off
/// by polls that take microseconds each (TEST-PLAN §19a, CORE-U-059).
pub async fn drive<F: std::future::Future>(
    future: F,
    clock: &FakeClock,
) -> Result<F::Output, Box<dyn Error>> {
    drive_within(
        future,
        clock,
        20_000,
        tqa_testkit::deadline::scaled(DRIVE_REAL_FLOOR),
    )
    .await
}

/// [`drive`] with its two bounds: `polls` polls of 10 ms of fake time, and at least `floor` of real
/// time. Past the poll budget each poll first waits 1 ms of real time on this thread (not a tokio
/// sleep, so it holds under paused time too), which lets a blocking-pool thread finish.
pub async fn drive_within<F: std::future::Future>(
    future: F,
    clock: &FakeClock,
    polls: u32,
    floor: Duration,
) -> Result<F::Output, Box<dyn Error>> {
    tokio::pin!(future);
    let started = std::time::Instant::now();
    for poll in 0_u32.. {
        if let std::task::Poll::Ready(result) = futures::poll!(&mut future) {
            return Ok(result);
        }
        if poll >= polls {
            if started.elapsed() >= floor {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        clock.advance(Duration::from_millis(10))?;
        tokio::task::yield_now().await;
    }
    Err("script did not finish within its bounded fake time".into())
}
pub fn history(events: Vec<Event>) -> Result<Vec<ServerFrame>, Box<dyn Error>> {
    let (mut run, mut step) = (None, None);
    let mut frames = Vec::new();
    for (index, event) in events.into_iter().enumerate() {
        match &event {
            Event::MessageUser { .. } => {
                run = None;
                step = None;
            }
            Event::RunStarted { run_id } => {
                run = Some(run_id.clone());
                step = None;
            }
            Event::StepStarted { step: number, .. } => step = Some(*number),
            _ => {}
        }
        frames.push(ServerFrame::Event {
            seq: Seq::from(u64::try_from(index + 1)?),
            run_id: run.clone(),
            step,
            ts: "2002-01-01T00:00:00Z".into(),
            event,
        });
    }
    Ok(frames)
}
pub fn initial() -> Result<Vec<Event>, Box<dyn Error>> {
    Ok(vec![
        Event::MessageUser {
            lead_brief: None,
            team_wake: None,
            text: "question".into(),
            attachments: vec![],
        },
        Event::RunStarted {
            run_id: "r-1".parse()?,
        },
        Event::StepStarted {
            step: 1,
            attempt: 1,
        },
    ])
}
pub fn model() -> Event {
    Event::ModelCompleted {
        step: 1,
        attempt: 1,
        finish_reason: tqa_types::FinishReason::ToolCalls,
    }
}
pub fn call(id: &str) -> Result<Event, Box<dyn Error>> {
    Ok(Event::ToolCall {
        call_id: id.parse()?,
        name: "read_file".into(),
        arguments: json!({"path":id}),
    })
}

pub async fn wait_for_results<F: std::future::Future + Unpin>(
    future: &mut F,
    sink: &MemorySink,
    count: usize,
) -> Option<F::Output> {
    match until_frame(future, sink, |sink| {
        (tool_results(&sink.events()).len() >= count).then_some(())
    })
    .await
    {
        Ok(Err(result)) => Some(result),
        Ok(Ok(())) | Err(_) => None,
    }
}

/// Drives `run` until `found` sees what it looks for in the sink, or the run ends (`Err` with its output, which is
/// never dropped: a finished run can't be polled again). Each
/// round polls the run before looking, as a resumed run registers its pending approval on its first poll; then it waits
/// for the next appended frame, not for a count of polls, since the run may wait on a real blocking thread (a result's
/// spill) for as long as a loaded host takes. The deadline only stops a run that hangs.
pub async fn until_frame<F: std::future::Future + Unpin, T>(
    run: &mut F,
    sink: &MemorySink,
    found: impl Fn(&MemorySink) -> Option<T>,
) -> Result<Result<T, F::Output>, &'static str> {
    tokio::time::timeout(Duration::from_mins(1), async {
        loop {
            let appended = sink.appended.notified();
            tokio::pin!(appended);
            appended.as_mut().enable();
            if let std::task::Poll::Ready(output) = futures::poll!(&mut *run) {
                return Err(output);
            }
            if let Some(value) = found(sink) {
                return Ok(value);
            }
            tokio::select! {
                output = &mut *run => {
                    return Err(output);
                }
                () = &mut appended => {}
            }
        }
    })
    .await
    .map_err(|_| "the run neither ended nor produced the awaited frame within a minute")
}
