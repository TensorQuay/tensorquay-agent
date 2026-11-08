//! The team tools over a [`TeamClient`]: strict schemas, the S1 contract's result texts, refusals as
//! `<code>: <message>`, and a shared flag that offers the lead's other tools from the model call after its first
//! successful `start_member` (ADR-0034, TOOL-U-037).
use super::client::{TeamClient, TeamFailure};
use super::done::{AcceptanceShell, Done};
use crate::{PreparedAction, Tool, ToolContext, ToolError, ToolOutput, ToolSpec, untrusted};
use async_trait::async_trait;
use serde_json::{Map, Value, json};
use std::fmt::Write as _;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tqa_types::{FilesTaken, TeamMessage};

/// A run's role, as `GET /runtime/teams/role` answers it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TeamRole {
    /// A member: its two tools.
    Member,
    /// A lead: the lead's tools, all offered.
    Lead,
    /// A task in no team: the lead's tools, `start_member` alone offered until it succeeds.
    MayLead,
    /// A lead whose team has ended and who hasn't read its end note: `read_reports` alone, which delivers it.
    Ended,
    /// Agent teams are off, or the task never leads: no team tool.
    None,
}

impl TeamRole {
    /// The role the gateway named; anything else is `None`.
    #[must_use]
    pub fn parse(role: &str) -> Self {
        match role {
            "member" => Self::Member,
            "lead" => Self::Lead,
            "may_lead" => Self::MayLead,
            "ended" => Self::Ended,
            _ => Self::None,
        }
    }
}

/// The team tools a run with `role` gets, `done` without a way to run an acceptance check.
pub fn tools_for_role(client: Arc<dyn TeamClient>, role: TeamRole) -> Vec<Arc<dyn Tool>> {
    tools_with_check(client, role, None)
}

/// As [`tools_for_role`], the lead's `done` running the administrator's acceptance check with `check` first
/// (ADR-0034 point 10).
pub fn tools_with_check(
    client: Arc<dyn TeamClient>,
    role: TeamRole,
    check: Option<AcceptanceShell>,
) -> Vec<Arc<dyn Tool>> {
    if role == TeamRole::None {
        return Vec::new();
    }
    let shared = Shared {
        client,
        leading: Arc::new(AtomicBool::new(matches!(
            role,
            TeamRole::Lead | TeamRole::Ended
        ))),
    };
    if role == TeamRole::Member {
        return vec![
            Arc::new(Post::new(Kind::SendToLead, shared.clone())),
            Arc::new(Read::new(true, shared)),
        ];
    }
    if role == TeamRole::Ended {
        // Its read answers team_ended with the end note as its result, once (ADR-0034 point 10).
        return vec![Arc::new(Read::new(false, shared))];
    }
    vec![
        Arc::new(Start::new(shared.clone())),
        Arc::new(Post::new(Kind::Assign, shared.clone())),
        Arc::new(Post::new(Kind::AskAll, shared.clone())),
        Arc::new(Post::new(Kind::Decide, shared.clone())),
        Arc::new(Post::new(Kind::AskPerson, shared.clone())),
        Arc::new(TakeFiles::new(shared.clone())),
        Arc::new(StopMember::new(shared.clone())),
        Arc::new(ReadHelper::new(shared.clone())),
        Arc::new(Done::new(shared.clone(), check)),
        Arc::new(Read::new(false, shared)),
    ]
}

/// The client and whether the run leads, shared by a run's team tools.
#[derive(Clone)]
pub(super) struct Shared {
    pub(super) client: Arc<dyn TeamClient>,
    pub(super) leading: Arc<AtomicBool>,
}

fn nullable(mut schema: Value) -> Value {
    if let Some(kind) = schema.get("type").cloned() {
        schema["type"] = json!([kind, "null"]);
    }
    if let Some(values) = schema.get_mut("enum").and_then(Value::as_array_mut) {
        values.push(Value::Null);
    }
    schema
}

pub(super) fn object(properties: &[(&str, Value)]) -> Value {
    let map: Map<String, Value> = properties
        .iter()
        .map(|(name, schema)| ((*name).to_owned(), schema.clone()))
        .collect();
    let required: Vec<&str> = properties.iter().map(|(name, _)| *name).collect();
    json!({"type": "object", "additionalProperties": false, "required": required, "properties": map})
}

pub(super) fn text(description: &str) -> Value {
    json!({"type": "string", "description": description})
}

/// `args` without its null members, as the gateway takes a post: optional arguments left out.
pub(super) fn present(args: Value) -> Value {
    match args {
        Value::Object(members) => Value::Object(
            members
                .into_iter()
                .filter(|(_, value)| !value.is_null())
                .collect(),
        ),
        other => other,
    }
}

async fn cancellable<T>(
    cx: &ToolContext,
    call: impl Future<Output = Result<T, TeamFailure>>,
) -> Result<T, Refused> {
    tokio::select! {
        biased;
        () = cx.cancel.cancelled() => Err(Refused::Failed(ToolError::Cancelled)),
        result = call => result.map_err(|failure| refusal(&failure)),
    }
}

/// A team call that the gateway didn't carry out.
pub(super) enum Refused {
    /// A refusal or failure, shown as `<code>: <message>`.
    Failed(ToolError),
    /// The team has ended, and the lead hadn't read everything (the end note among it): those messages are the
    /// tool's result, shown as a read shows them. A tool error couldn't carry them, as it is at most 500 characters
    /// and holds no content (ADR-0034 point 10).
    Ended(Box<ToolOutput>),
}

impl From<ToolError> for Refused {
    fn from(error: ToolError) -> Self {
        Self::Failed(error)
    }
}

/// The tool's outcome: an ended team's unread messages are its result.
pub(super) fn settle(outcome: Result<ToolOutput, Refused>) -> Result<ToolOutput, ToolError> {
    match outcome {
        Ok(output) => Ok(output),
        Err(Refused::Ended(output)) => Ok(*output),
        Err(Refused::Failed(error)) => Err(error),
    }
}

/// A refusal as `<code>: <message>`. Once the team has ended, the lead's first `team_ended` carries what it hadn't read
/// (the end note among it), shown as a read shows it: each message its own marked block, nothing cut beyond what any
/// result is (ADR-0034 point 10).
pub(super) fn refusal(failure: &TeamFailure) -> Refused {
    let messages = failure
        .details
        .as_ref()
        .filter(|_| failure.code == "team_ended")
        .and_then(|details| details["messages"].as_array())
        .filter(|messages| !messages.is_empty());
    let Some(messages) = messages else {
        return Refused::Failed(ToolError::channel(&failure.code, &failure.message));
    };
    match blocks(messages) {
        Ok(text) => Refused::Ended(Box::new(ToolOutput::text(format!(
            "Your agent team has ended, so its team tools no longer work. What you hadn't read yet:\n{}\nFinish your \
             answer to the person.",
            text.trim_end()
        )))),
        Err(error) => Refused::Failed(error),
    }
}

/// The call's idempotency key and run, from its prepared action.
pub(super) fn identity(action: &PreparedAction) -> (String, String) {
    let context = action.context();
    let run = context.run_id.to_string();
    (
        super::super::channel::idempotency_key(&run, &context.call_id.to_string()),
        run,
    )
}

/// `start_member`: always offered to a task that may lead.
struct Start {
    spec: ToolSpec,
    shared: Shared,
}

impl Start {
    fn new(shared: Shared) -> Self {
        let brief = object(&[
            ("objective", text("What the helper is to achieve.")),
            ("output", text("What it hands back, and in what form.")),
            (
                "sources",
                nullable(text("The files, sources or tools to use, if it matters.")),
            ),
            ("boundaries", text("What it must not do or touch.")),
        ]);
        let parameters = object(&[
            (
                "role",
                text(
                    "The helper's job in a few words, for example researcher; its name in the team.",
                ),
            ),
            ("brief", brief),
            (
                "workspace",
                nullable(json!({"type": "string", "enum": ["own_copy", "my_files"],
                "description": "own_copy (the default): a copy of your files, worked in parallel. my_files: your own files, taking turns with you."})),
            ),
            (
                "profile",
                nullable(text("An agent profile to start it from, by name.")),
            ),
        ]);
        Self {
            spec: super::super::specification(
                "start_member",
                "Start a helper: a task of the same person that works on one part of your task from your brief and reports back to you. Start helpers only when the work splits; one for simple work, two to four typically.",
                parameters,
                false,
            ),
            shared,
        }
    }

    async fn run(
        &self,
        args: Value,
        call: (&str, &str),
        cx: &ToolContext,
    ) -> Result<ToolOutput, Refused> {
        let mut body = present(args);
        if let Some(brief) = body.get_mut("brief") {
            *brief = present(brief.take());
        }
        let answer = cancellable(cx, self.shared.client.start(&body, call, &cx.cancel)).await?;
        if answer["lead_tools"] == true {
            // From the next model call the lead's other tools are offered too (TOOL-U-037).
            self.shared.leading.store(true, Ordering::SeqCst);
        }
        let member = &answer["member"];
        let name = member["name"].as_str().unwrap_or("the helper");
        let place = if member["workspace"] == "my_files" {
            "on your files"
        } else {
            "own copy"
        };
        let mut text = format!(
            "Started helper {name} ({place}); its brief is message {}. It reports to you.",
            answer["seq"]
        );
        if member["consistent"] == false {
            text.push_str(" Its copy was taken while you were working, so a file you were changing may be partial.");
        }
        if let Some(left) = member["left_out"]
            .as_array()
            .filter(|left| !left.is_empty())
        {
            let names: Vec<&str> = left.iter().filter_map(Value::as_str).collect();
            let _ = write!(
                text,
                " Left out of its profile, as your task can't use them: {}.",
                names.join(", ")
            );
        }
        Ok(ToolOutput::text(text))
    }
}

#[async_trait]
impl Tool for Start {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn null_means_absent(&self) -> bool {
        true
    }
    async fn call(&self, args: Value, cx: &ToolContext) -> Result<ToolOutput, ToolError> {
        settle(self.run(args, ("", ""), cx).await)
    }
    async fn call_prepared(
        &self,
        action: PreparedAction,
        cx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let (key, run) = identity(&action);
        let (_, args) = action.into_call();
        settle(self.run(args, (&key, &run), cx).await)
    }
}

/// The posting tools.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Kind {
    Assign,
    AskAll,
    Decide,
    AskPerson,
    SendToLead,
}

struct Post {
    spec: ToolSpec,
    kind: Kind,
    shared: Shared,
}

impl Post {
    fn new(kind: Kind, shared: Shared) -> Self {
        let message = || text("The message.");
        let (name, description, parameters) = match kind {
            Kind::Assign => (
                "assign",
                "Send one of your helpers more work or a correction. Only that helper reads it.",
                object(&[("to", text("The helper, by name.")), ("text", message())]),
            ),
            Kind::AskAll => (
                "ask_all",
                "Ask every helper a question; each answers you with send_to_lead.",
                object(&[("text", message())]),
            ),
            Kind::Decide => (
                "decide",
                "Tell every helper a decision, optionally the answer to a question: one you asked them, or one a helper asked you.",
                object(&[
                    ("text", message()),
                    (
                        "question_seq",
                        nullable(json!({"type": "integer", "minimum": 1,
                        "description": "The question this decides, by its message number: your question, or a helper's ask."})),
                    ),
                ]),
            ),
            Kind::AskPerson => (
                "ask_person",
                "Ask the person who started your task. Your task shows Waiting for you; the answer arrives as a message.",
                object(&[("text", message())]),
            ),
            Kind::SendToLead => (
                "send_to_lead",
                "Send your lead an answer to its question, a report of your work (with the files it should take), or a question of your own.",
                object(&[
                    (
                        "type",
                        json!({"type": "string", "enum": ["answer", "report", "ask"],
                        "description": "answer (to a question: question_seq), report (your work: files to hand back), or ask."}),
                    ),
                    ("text", message()),
                    (
                        "question_seq",
                        nullable(json!({"type": "integer", "minimum": 1,
                        "description": "For an answer: the question's message number."})),
                    ),
                    (
                        "files",
                        nullable(
                            json!({"type": "array", "items": {"type": "string"}, "maxItems": 32,
                        "description": "For a report: files to hand back, relative to your workspace."}),
                        ),
                    ),
                ]),
            ),
        };
        Self {
            spec: super::super::specification(name, description, parameters, false),
            kind,
            shared,
        }
    }

    async fn run(
        &self,
        args: Value,
        call: (&str, &str),
        cx: &ToolContext,
    ) -> Result<ToolOutput, Refused> {
        let mut body = present(args);
        let kind = match self.kind {
            Kind::Assign => "assign",
            Kind::AskAll => "question",
            Kind::Decide => "decision",
            Kind::AskPerson => "person_question",
            Kind::SendToLead => body["type"].as_str().unwrap_or("ask"),
        }
        .to_owned();
        body["type"] = json!(kind);
        let answer = cancellable(cx, self.shared.client.post(&body, call, &cx.cancel)).await?;
        let seq = &answer["seq"];
        let helpers = answer["to"].as_array().map_or(0, Vec::len);
        let text = match self.kind {
            Kind::Assign => format!(
                "Sent to {} as message {seq}.",
                body["to"].as_str().unwrap_or("the helper")
            ),
            Kind::AskAll => format!("Asked {helpers} helpers as message {seq}."),
            Kind::Decide => format!("Decision sent to {helpers} helpers as message {seq}."),
            Kind::AskPerson => {
                "Asked; your task shows Waiting for you. The answer arrives as a message."
                    .to_owned()
            }
            Kind::SendToLead => format!("Sent to your lead as message {seq}."),
        };
        Ok(ToolOutput::text(text))
    }
}

#[async_trait]
impl Tool for Post {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn offered(&self) -> bool {
        self.kind == Kind::SendToLead || self.shared.leading.load(Ordering::SeqCst)
    }
    fn null_means_absent(&self) -> bool {
        true
    }
    async fn call(&self, args: Value, cx: &ToolContext) -> Result<ToolOutput, ToolError> {
        settle(self.run(args, ("", ""), cx).await)
    }
    async fn call_prepared(
        &self,
        action: PreparedAction,
        cx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let (key, run) = identity(&action);
        let (_, args) = action.into_call();
        settle(self.run(args, (&key, &run), cx).await)
    }
}

/// `take_files` (the S1 contract after the 2 Oct flat option): the files a helper handed back with a report, placed in
/// your workspace's `uploads/` as `<member>-<path>`, never over a file already there. A lead's, offered once it leads.
pub struct TakeFiles {
    spec: ToolSpec,
    shared: Shared,
}

impl TakeFiles {
    fn new(shared: Shared) -> Self {
        let parameters = object(&[
            (
                "report_seq",
                json!({"type": "integer", "minimum": 1, "description": "The report, by its message number."}),
            ),
            (
                "files",
                nullable(
                    json!({"type": "array", "items": {"type": "string"}, "maxItems": 32,
                "description": "Files to take, as the report lists them; leave out for all of them."}),
                ),
            ),
        ]);
        Self {
            // It places files in the lead's workspace: a success may have changed it (CORE-E-041).
            spec: ToolSpec {
                changes_workspace: true,
                ..super::super::specification(
                    "take_files",
                    "Take the files a helper handed back with a report: each is placed in your workspace's uploads/ under the helper's name, never over a file already there. Then move them where they belong with your shell.",
                    parameters,
                    false,
                )
            },
            shared,
        }
    }

    async fn run(
        &self,
        args: Value,
        call: (&str, &str),
        cx: &ToolContext,
    ) -> Result<ToolOutput, Refused> {
        let body = present(args);
        let answer =
            cancellable(cx, self.shared.client.take_files(&body, call, &cx.cancel)).await?;
        // The gateway's answer as slice 5's contract types it; one that doesn't parse is the gateway's error.
        let FilesTaken { placed } = serde_json::from_value(answer).map_err(|_| {
            ToolError::channel("gateway_error", "the gateway's answer could not be read")
        })?;
        let each: Vec<String> = placed
            .iter()
            .map(|file| {
                format!(
                    "{} as {} ({} bytes)",
                    file.name,
                    file.path,
                    grouped(file.size)
                )
            })
            .collect();
        let count = if placed.len() == 1 {
            "1 file".to_owned()
        } else {
            format!("{} files", placed.len())
        };
        Ok(ToolOutput::text(format!(
            "Placed {count} from report {} in uploads/: {}. Move them where they belong with your shell.",
            body["report_seq"],
            each.join(", ")
        )))
    }
}

/// `n` with thousands separators, as the contract's text writes a size (2,104) and `read_helper` a token count.
#[must_use]
pub fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

#[async_trait]
impl Tool for TakeFiles {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn offered(&self) -> bool {
        self.shared.leading.load(Ordering::SeqCst)
    }
    fn null_means_absent(&self) -> bool {
        true
    }
    async fn call(&self, args: Value, cx: &ToolContext) -> Result<ToolOutput, ToolError> {
        settle(self.run(args, ("", ""), cx).await)
    }
    async fn call_prepared(
        &self,
        action: PreparedAction,
        cx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let (key, run) = identity(&action);
        let (_, args) = action.into_call();
        settle(self.run(args, (&key, &run), cx).await)
    }
}

/// `take_files` for a lead's tools over `client`, for when the gateway's route is on, and for its tests.
pub fn take_files(client: Arc<dyn TeamClient>, leading: bool) -> Arc<dyn Tool> {
    Arc::new(TakeFiles::new(Shared {
        client,
        leading: Arc::new(AtomicBool::new(leading)),
    }))
}

/// The `Tool` methods every lead tool shares: offered once the task leads, nulls as absent, and the call's key and
/// run from its prepared action.
macro_rules! lead_tool {
    ($tool:ty) => {
        #[async_trait]
        impl Tool for $tool {
            fn spec(&self) -> &ToolSpec {
                &self.spec
            }
            fn offered(&self) -> bool {
                self.shared.leading.load(Ordering::SeqCst)
            }
            fn null_means_absent(&self) -> bool {
                true
            }
            async fn call(&self, args: Value, cx: &ToolContext) -> Result<ToolOutput, ToolError> {
                settle(self.run(args, ("", ""), cx).await)
            }
            async fn call_prepared(
                &self,
                action: PreparedAction,
                cx: &ToolContext,
            ) -> Result<ToolOutput, ToolError> {
                let (key, run) = identity(&action);
                let (_, args) = action.into_call();
                settle(self.run(args, (&key, &run), cx).await)
            }
        }
    };
}

/// `stop_member`: stops one helper; its work ends and it reads no more messages, and its task stays.
struct StopMember {
    spec: ToolSpec,
    shared: Shared,
}

impl StopMember {
    fn new(shared: Shared) -> Self {
        let parameters = object(&[
            ("name", text("The helper, by name.")),
            ("reason", text("Why, in a sentence.")),
        ]);
        Self {
            spec: super::super::specification(
                "stop_member",
                "Stop one of your helpers: its work ends now and it reads no more messages. Its task stays for the person to see.",
                parameters,
                false,
            ),
            shared,
        }
    }

    async fn run(
        &self,
        args: Value,
        call: (&str, &str),
        cx: &ToolContext,
    ) -> Result<ToolOutput, Refused> {
        let name = args["name"].as_str().unwrap_or_default().to_owned();
        let body = json!({"reason": args["reason"]});
        let answer = cancellable(
            cx,
            self.shared
                .client
                .stop_member(&name, &body, call, &cx.cancel),
        )
        .await?;
        let stopped = answer["member"]["name"].as_str().unwrap_or(&name);
        Ok(ToolOutput::text(format!("Stopped helper {stopped}.")))
    }
}

lead_tool!(StopMember);

/// `read_helper`: one helper's messages, text, tool calls and results in order, each marked untrusted. Read-only.
struct ReadHelper {
    spec: ToolSpec,
    shared: Shared,
}

impl ReadHelper {
    fn new(shared: Shared) -> Self {
        let parameters = object(&[
            ("name", text("The helper, by name.")),
            (
                "since",
                nullable(json!({"type": "integer", "minimum": 0,
                "description": "Only what came after this point (last_seq from an earlier read); leave out for the start."})),
            ),
            (
                "limit",
                nullable(json!({"type": "integer", "minimum": 1, "maximum": 200,
                "description": "The most events to return (default 50)."})),
            ),
        ]);
        Self {
            spec: super::super::specification(
                "read_helper",
                "Read where one of your helpers is now (its step and for how long, how much it has thought so far, \
                 or waiting or stopped), then what it has done: its messages, text, tool calls and results, in order. \
                 Read-only. A long first step that only thinks is slow, not stuck.",
                parameters,
                true,
            ),
            shared,
        }
    }

    async fn run(
        &self,
        args: Value,
        (_, run): (&str, &str),
        cx: &ToolContext,
    ) -> Result<ToolOutput, Refused> {
        let name = args["name"].as_str().unwrap_or_default().to_owned();
        let since = args["since"].as_u64();
        let limit = args["limit"]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok());
        let answer = cancellable(
            cx,
            self.shared
                .client
                .transcript(&name, (since, limit), run, &cx.cancel),
        )
        .await?;
        let mut text = now_line(&name, &answer["now"]).map_or_else(String::new, |line| line + "\n");
        for event in answer["events"].as_array().cloned().unwrap_or_default() {
            let seq = event["seq"]
                .as_u64()
                .map_or_else(|| "unknown".to_owned(), |seq| seq.to_string());
            let kind = event["type"].as_str().unwrap_or("unknown").to_owned();
            let body = match kind.as_str() {
                "message" | "text" => event["text"].as_str().unwrap_or_default().to_owned(),
                "tool_call" => format!(
                    "{} {}",
                    event["name"].as_str().unwrap_or("?"),
                    event["arguments"]
                ),
                "tool_result" => format!(
                    "[{}] {}",
                    event["status"].as_str().unwrap_or("?"),
                    event["content"].as_str().unwrap_or_default()
                ),
                // The gateway woke the helper for new team messages (revision 13): never words it was sent.
                "wake" => format!(
                    "The gateway woke the helper: new team messages after message {}.",
                    event["after"].as_u64().unwrap_or(0)
                ),
                _ => event.to_string(),
            };
            let attributes = [
                ("helper", name.as_str()),
                ("seq", seq.as_str()),
                ("type", kind.as_str()),
            ];
            text.push_str(&untrusted::wrap("helper", &attributes, &body)?);
            text.push('\n');
        }
        let next = if answer["more"] == true {
            format!(
                "More is waiting: read again with since {}.",
                answer["last_seq"]
            )
        } else {
            format!(
                "That is all so far; since {} reads what comes next.",
                answer["last_seq"]
            )
        };
        text.push_str(&next);
        Ok(ToolOutput::text(text))
    }
}

lead_tool!(ReadHelper);

/// `read_helper`'s first line: where the helper is now, from the gateway's `now`, so a lead can tell a helper that is
/// slow (a long first step's thinking) from one that is stuck. It is the gateway's account, not the helper's words, so
/// it isn't marked untrusted; the helper's name is the one the lead gave. The tool's name comes from the helper's own
/// call, so only a plain tool name is shown; anything else is "a tool". None when the gateway sent no `now`.
fn now_line(name: &str, now: &Value) -> Option<String> {
    let state = now["state"].as_str()?;
    let line = match (state, now["step"].as_u64()) {
        ("working", Some(step)) => {
            let seconds = now["seconds"].as_u64().unwrap_or(0);
            match now["tool"].as_str() {
                Some(tool) => format!(
                    "{name} is working: step {step}, running {} for {}.",
                    plain_tool(tool),
                    duration(seconds)
                ),
                None => format!(
                    "{name} is working: step {step}, the model has been answering for {}, about {} tokens of \
                     thinking and {} of text so far.",
                    duration(seconds),
                    grouped(now["thinking_tokens"].as_u64().unwrap_or(0)),
                    grouped(now["text_tokens"].as_u64().unwrap_or(0))
                ),
            }
        }
        ("working", None) => format!("{name} is working: starting its next step."),
        ("waiting_lead", _) => format!("{name} is waiting for a message from you."),
        ("queued", _) => format!("{name} is queued: it starts when a slot frees."),
        ("stuck", _) => format!("{name} is stuck: its run can't go on."),
        ("done", _) => format!("{name} has finished."),
        ("stopped", _) => format!("{name} was stopped."),
        ("failed", _) => format!("{name}'s run failed."),
        _ => format!("{name}'s state is {state}."),
    };
    Some(line)
}

/// `tool` when it is a plain tool name (a letter, then letters, digits, `_`, `-` or `.` as in `mcp.<server>.<tool>`;
/// at most 128 bytes, no spaces, so no sentence), else "a tool".
fn plain_tool(tool: &str) -> &str {
    let plain = (1..=128).contains(&tool.len())
        && tool.starts_with(|c: char| c.is_ascii_alphabetic())
        && tool
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    if plain { tool } else { "a tool" }
}

/// `seconds` as "40 s", "3 min 42 s" or "1 h 5 min".
fn duration(seconds: u64) -> String {
    match seconds {
        0..60 => format!("{seconds} s"),
        60..3600 => format!("{} min {} s", seconds / 60, seconds % 60),
        _ => format!("{} h {} min", seconds / 3600, seconds % 3600 / 60),
    }
}

/// `read_reports`, a lead's, and `wait_for_lead`, a member's.
struct Read {
    spec: ToolSpec,
    member: bool,
    shared: Shared,
}

impl Read {
    fn new(member: bool, shared: Shared) -> Self {
        let parameters = object(&[
            (
                "wait",
                nullable(json!({"type": "integer", "minimum": 0, "maximum": 30,
                "description": "Seconds to wait for a message when none is new (0 to 30)."})),
            ),
            (
                "limit",
                nullable(json!({"type": "integer", "minimum": 1, "maximum": 200,
                "description": "The most messages to return (default 50)."})),
            ),
        ]);
        let (name, description) = if member {
            (
                "wait_for_lead",
                "Read what your lead sent you since your last read, waiting up to 30 seconds for it.",
            )
        } else {
            (
                "read_reports",
                "Read your helpers' answers, reports and questions, and the person's answers and messages, since your last \
                 read. The person may write while the team works; their word comes first: re-brief or start helpers \
                 as they ask.",
            )
        };
        Self {
            spec: super::super::specification(name, description, parameters, true),
            member,
            shared,
        }
    }

    async fn run(
        &self,
        args: &Value,
        (key, run): (&str, &str),
        cx: &ToolContext,
    ) -> Result<ToolOutput, Refused> {
        let number = |name: &str| {
            args[name]
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
        };
        // A member waits 30 seconds unless it asks otherwise (the contract's default).
        let wait = number("wait").or(self.member.then_some(30));
        let answer = cancellable(
            cx,
            self.shared
                .client
                .read((number("limit"), wait), (key, run), &cx.cancel),
        )
        .await?;
        Ok(render(&answer, self.member)?)
    }
}

#[async_trait]
impl Tool for Read {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn offered(&self) -> bool {
        self.member || self.shared.leading.load(Ordering::SeqCst)
    }
    fn null_means_absent(&self) -> bool {
        true
    }
    async fn call(&self, args: Value, cx: &ToolContext) -> Result<ToolOutput, ToolError> {
        settle(self.run(&args, ("", ""), cx).await)
    }
    async fn call_prepared(
        &self,
        action: PreparedAction,
        cx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let (key, run) = identity(&action);
        let (_, args) = action.into_call();
        settle(self.run(&args, (&key, &run), cx).await)
    }
}

/// A read's answer as the model sees it: each message one marked block (a person's words in the signed-in-person
/// block, an agent's inside the untrusted marker, one this runtime can't read as marked raw JSON), then what to do next.
fn render(answer: &Value, member: bool) -> Result<ToolOutput, ToolError> {
    let messages = answer["messages"].as_array().cloned().unwrap_or_default();
    let mut text = blocks(&messages)?;
    let next = match answer["hint"]["kind"].as_str() {
        Some("yield_slot") => {
            "Another task is waiting for a slot: end your turn now; you'll be woken by the next message.".to_owned()
        }
        Some("yield_files") => format!(
            "Helper {} is waiting to work on your files: end your turn now so it can; you'll be woken by its report.",
            answer["hint"]["member"].as_str().unwrap_or("of yours")
        ),
        _ if answer["more"] == true => "More messages are waiting: read again.".to_owned(),
        _ if messages.is_empty() && member => {
            "Nothing yet; end your turn, and you'll be woken by your lead's next message.".to_owned()
        }
        _ if messages.is_empty() => "Nothing new yet.".to_owned(),
        _ => String::new(),
    };
    text.push_str(&next);
    Ok(ToolOutput::text(text.trim_end().to_owned()))
}

/// Each message one marked block, in order: a person's words in the signed-in-person block, an agent's inside the
/// untrusted marker, one this runtime can't read as marked raw JSON.
fn blocks(messages: &[Value]) -> Result<String, ToolError> {
    let mut text = String::new();
    for message in messages {
        let block = if let Ok(message) = serde_json::from_value::<TeamMessage>(message.clone()) {
            block(&message)?
        } else {
            let seq = message["seq"]
                .as_u64()
                .map_or_else(|| "unknown".to_owned(), |seq| seq.to_string());
            let body = format!("a message this runtime does not recognise:\n{message}");
            untrusted::wrap("team", &[("seq", seq.as_str()), ("type", "unknown")], &body)?
        };
        text.push_str(&block);
        text.push('\n');
    }
    Ok(text)
}

fn block(message: &TeamMessage) -> Result<String, ToolError> {
    let seq = message.seq.to_string();
    let body = serde_json::to_value(&message.body)
        .map_err(|_| ToolError::invalid("", "unreadable message"))?;
    let kind = body["type"].as_str().unwrap_or("unknown").to_owned();
    // A person's message names its person (`by`); an agent's, its member name or its kind.
    let from = body["by"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| message.from.name.clone())
        .unwrap_or_else(|| format!("{:?}", message.from.kind).to_lowercase());
    let mut words = body["text"].as_str().unwrap_or_default().to_owned();
    // What the person typed to the lead itself while the team works names no helper (GW-I-307, TOOL-U-065).
    if kind == "person_message" && body["member"] == "" {
        words = format!("The person wrote, while the team works: {words}");
    }
    for (name, label) in [("question_seq", "question"), ("decision_seq", "decision")] {
        if let Some(seq) = body[name].as_u64() {
            words = format!("({label} {seq}) {words}");
        }
    }
    if let Some(brief) = body["brief"].as_object() {
        for (name, label) in [
            ("output", "Expected output"),
            ("sources", "Sources"),
            ("boundaries", "Boundaries"),
        ] {
            if let Some(part) = brief.get(name).and_then(Value::as_str) {
                let _ = write!(words, "\n{label}: {part}");
            }
        }
    }
    let attributes = [
        ("seq", seq.as_str()),
        ("type", kind.as_str()),
        ("from", from.as_str()),
    ];
    if message.body.message_type().from_person() {
        untrusted::person(&attributes, &words)
    } else {
        untrusted::wrap("team", &attributes, &words)
    }
}
