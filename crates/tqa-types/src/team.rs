//! Agent-team messages (ADR-0034, the slice S1 contract): `start_member`'s request and brief, the posts a lead or a
//! member makes, and the stored messages of an agent team's log with their read shape. The gateway decodes requests
//! strictly (unknown fields refused); a runtime reads answers tolerantly (TOOL-U-036).
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The largest request body on an agent-team route, in bytes.
pub const MAX_TEAM_REQUEST_BYTES: usize = 64 * 1024;
/// The longest helper role, in bytes.
pub const MAX_ROLE_BYTES: usize = 60;
/// The longest part of a brief, in bytes.
pub const MAX_BRIEF_PART_BYTES: usize = 4_000;
/// The longest brief as a whole, in bytes.
pub const MAX_BRIEF_BYTES: usize = 12 * 1024;

/// Where a new member works (ADR-0034 point 6).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceMode {
    /// A fork of the lead's workspace, worked in parallel; results come back as files.
    #[default]
    OwnCopy,
    /// The lead's own workspace, taking turns under its lease.
    MyFiles,
}

/// A member's first turn as the lead wrote it (revision 12): the member's role and workspace mode and the lead's
/// brief. The turn's `text` is the gateway's fixed frame; the runtime gives the model this brief after the frame,
/// inside the untrusted-content marker, and a client shows it as the brief from the lead, never as the person's words.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LeadBrief {
    /// The member's role, as the lead named it.
    pub role: String,
    /// Where the member works.
    pub workspace: WorkspaceMode,
    /// The lead's brief.
    pub brief: Brief,
}

/// The gateway's wake of an agent team reader (revision 13, ADR-0034 Waking): new team messages wait after the
/// reader's place in the team's log. It names no writer: the waiting messages may be several people's and the
/// gateway's, and the log shows who wrote. The turn's `text` stays the gateway's fixed sentence for the model; a client
/// shows this as the gateway's notice, in its own words, never as the person's message.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TeamWake {
    /// The reader's place in the team's log: the waiting messages come after it.
    pub after: u64,
}

/// What the lead writes for a new member: the objective, the expected output and the boundaries, all required
/// (proposal A, ADR-0034), and optionally the sources or tools to use.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Brief {
    /// What the member is to achieve.
    pub objective: String,
    /// What it hands back, and in what form.
    pub output: String,
    /// The sources or tools to use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sources: Option<String>,
    /// What not to do.
    pub boundaries: String,
}

impl Brief {
    /// Its parts by their field names, in the brief's order; the sources only when given.
    #[must_use]
    pub fn parts(&self) -> Vec<(&'static str, &str)> {
        let mut parts = vec![
            ("brief.objective", self.objective.as_str()),
            ("brief.output", self.output.as_str()),
        ];
        if let Some(sources) = &self.sources {
            parts.push(("brief.sources", sources.as_str()));
        }
        parts.push(("brief.boundaries", self.boundaries.as_str()));
        parts
    }
}

/// `start_member`'s request.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StartMember {
    /// The member's job in words ("researcher"); its name, made unique within the agent team.
    pub role: String,
    /// The lead's brief.
    pub brief: Brief,
    /// Where it works.
    #[serde(default)]
    pub workspace: WorkspaceMode,
    /// An agent profile the person may use, by ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
}

/// A stored message's type.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TeamMessageType {
    /// The lead to one member.
    Assign,
    /// The lead to every member.
    Question,
    /// A member's answer to a question.
    Answer,
    /// A member's report, with files.
    Report,
    /// A member's own question to the lead.
    Ask,
    /// The lead's decision, to every member.
    Decision,
    /// The lead's question to the person.
    PersonQuestion,
    /// A person's answer to the lead.
    PersonAnswer,
    /// A copy, for the lead, of what a person typed to a member.
    PersonMessage,
    /// The gateway to the lead.
    Notice,
    /// The lead's end of the work, to every member.
    Done,
    /// The gateway's end note, to the lead: how and why the team ended (ADR-0034 point 10).
    Ended,
}

impl TeamMessageType {
    /// Every type.
    pub const ALL: [Self; 12] = [
        Self::Assign,
        Self::Question,
        Self::Answer,
        Self::Report,
        Self::Ask,
        Self::Decision,
        Self::PersonQuestion,
        Self::PersonAnswer,
        Self::PersonMessage,
        Self::Notice,
        Self::Done,
        Self::Ended,
    ];

    /// The longest text a message of this type carries, in bytes.
    #[must_use]
    pub const fn max_text_bytes(self) -> usize {
        match self {
            Self::Assign => 12 * 1024,
            Self::Answer
            | Self::Report
            | Self::PersonAnswer
            | Self::PersonMessage
            | Self::Ended => 16 * 1024,
            Self::Question | Self::Ask | Self::Decision | Self::Done => 8 * 1024,
            Self::PersonQuestion | Self::Notice => 4 * 1024,
        }
    }

    /// Whether a person wrote it: shown to a model as a signed-in person's words, not inside the agent marker.
    #[must_use]
    pub const fn from_person(self) -> bool {
        matches!(self, Self::PersonAnswer | Self::PersonMessage)
    }
}

/// Why the gateway tells the lead something.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
    /// A member stopped without reporting.
    MemberIdle,
    /// A member's run has made no progress.
    MemberStuck,
    /// A member's run failed.
    MemberFailed,
    /// A member was stopped.
    MemberStopped,
    /// A member's task was deleted.
    MemberDeleted,
    /// A member waits for a slot.
    MemberQueued,
    /// 80 % of the budget is used.
    #[serde(rename = "budget_80")]
    Budget80,
    /// The deadline is near.
    DeadlineNear,
    /// The team reached its helpers' share of the budget: they stop, and the lead writes up with the rest (GW-I-304).
    BudgetHelpersStopped,
    /// The lead's turn ended without `done` while its team runs (GW-I-306): call done, or keep leading.
    LeadUnfinished,
}

/// A file a member handed back with a report.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HandedFile {
    /// Its name, relative to the member's workspace.
    pub name: String,
    /// Its size in bytes.
    pub size: u64,
    /// Its SHA-256, in lowercase hex.
    pub sha256: String,
}

/// `take_files`' request (ADR-0034 point 6): a report's files to place in the lead's `uploads/`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TakeFiles {
    /// The report, by its seq.
    pub report_seq: u64,
    /// Paths as the report lists them, at most 32; all of the report's files when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
}

/// One file `take_files` placed.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlacedFile {
    /// Its path as the report lists it.
    pub name: String,
    /// Where it went in the lead's workspace: `uploads/<member>-<path>`, or the next free name.
    pub path: String,
    /// Its size in bytes.
    pub size: u64,
}

/// `take_files`' answer.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FilesTaken {
    /// Each file placed, in the request's order.
    pub placed: Vec<PlacedFile>,
}

/// The most of an acceptance check's output a `done` report carries, in bytes: its last 4 KiB (ADR-0034 point 10).
pub const MAX_CHECK_OUTPUT_BYTES: usize = 4_096;

/// What the administrator's acceptance check did in the lead's workspace, sent with `done` (ADR-0034 point 10,
/// GW-I-198). The gateway holds `done` to it: refused unless `command` is the check set now and it exited 0.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckReport {
    /// The command it ran, as the administrator set it.
    pub command: String,
    /// Its exit code; `None` when a signal ended it or it was stopped at its time limit.
    pub exit_code: Option<i32>,
    /// Whether it was stopped at its time limit.
    pub timed_out: bool,
    /// The end of what it printed, at most [`MAX_CHECK_OUTPUT_BYTES`], its standard error marked.
    pub output: String,
}

/// The last `max` bytes of `text` at most, cut on a character boundary.
#[must_use]
pub fn last_bytes(text: &str, max: usize) -> &str {
    let mut start = text.len().saturating_sub(max);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

/// A stored message's body.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TeamMessageBody {
    /// The lead to one member; the first one carries the brief.
    Assign {
        /// The text.
        text: String,
        /// The brief, on a member's first message.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        brief: Option<Brief>,
    },
    /// The lead to every member.
    Question {
        /// The text.
        text: String,
    },
    /// A member's answer.
    Answer {
        /// The text.
        text: String,
        /// The question it answers.
        question_seq: u64,
    },
    /// A member's report.
    Report {
        /// The text.
        text: String,
        /// The files handed back with it.
        #[serde(default)]
        files: Vec<HandedFile>,
    },
    /// A member's own question to the lead.
    Ask {
        /// The text.
        text: String,
    },
    /// The lead's decision.
    Decision {
        /// The text.
        text: String,
        /// The question it decides, if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        question_seq: Option<u64>,
    },
    /// The lead's question to the person.
    PersonQuestion {
        /// The text.
        text: String,
    },
    /// A person's answer to the lead.
    PersonAnswer {
        /// The text.
        text: String,
        /// The `person_question` it answers.
        question_seq: u64,
        /// The person's display name.
        by: String,
    },
    /// A copy, for the lead, of what a person typed to a member, or to the lead itself while its team works.
    PersonMessage {
        /// The text.
        text: String,
        /// The member it was typed to; empty when it was typed to the lead (GW-I-307).
        member: String,
        /// The person's display name.
        by: String,
    },
    /// The gateway to the lead: a fixed sentence.
    Notice {
        /// Why.
        kind: NoticeKind,
        /// The member it is about, if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        member: Option<String>,
        /// The member's place in the queue, for `member_queued`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<u32>,
        /// The sentence.
        text: String,
    },
    /// The lead's end of the work.
    Done {
        /// The text.
        text: String,
    },
    /// The gateway's end note to the lead (ADR-0034 point 10): the whole note in fixed sentences in `text`, for any
    /// reader, and its parts beside it for a page that words them itself.
    Ended {
        /// Why the team ended.
        reason: EndReason,
        /// The lead's `done` summary, when it ended that way.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
        /// The tokens the team spent.
        tokens: EndTokens,
        /// Each member's reports, by message seq.
        reports: Vec<EndReport>,
        /// The files the members handed back with their reports.
        files: Vec<EndFile>,
        /// The note in the gateway's fixed sentences.
        text: String,
    },
}

/// Why an agent team ended (`agent_teams.stop_reason`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    /// The lead called `done`.
    Done,
    /// A person pressed Stop on the lead's task.
    Stopped,
    /// The token budget was spent.
    Budget,
    /// The deadline passed.
    Deadline,
    /// Nothing happened for `idle_stop_minutes`.
    Idle,
    /// The lead's task was deleted.
    LeadDeleted,
    /// The lead's person was disabled.
    PersonDisabled,
}

/// The tokens an agent team spent, in all and by member.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EndTokens {
    /// The lead's and the members' tokens together.
    pub total: i64,
    /// Each member's, in the order they were started.
    pub members: Vec<MemberTokens>,
}

/// One member's tokens.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemberTokens {
    /// The member.
    pub name: String,
    /// Its prompt and completion tokens.
    pub tokens: i64,
}

/// A member's report, by its seq in the team's log.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EndReport {
    /// The member.
    pub member: String,
    /// The report's seq.
    pub seq: u64,
}

/// A file a member handed back with a report.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EndFile {
    /// The member.
    pub member: String,
    /// The report it came with.
    pub seq: u64,
    /// Its name, relative to the member's workspace.
    pub name: String,
    /// Its size in bytes.
    pub size: u64,
}

impl TeamMessageBody {
    /// Its type.
    #[must_use]
    pub const fn message_type(&self) -> TeamMessageType {
        match self {
            Self::Assign { .. } => TeamMessageType::Assign,
            Self::Question { .. } => TeamMessageType::Question,
            Self::Answer { .. } => TeamMessageType::Answer,
            Self::Report { .. } => TeamMessageType::Report,
            Self::Ask { .. } => TeamMessageType::Ask,
            Self::Decision { .. } => TeamMessageType::Decision,
            Self::PersonQuestion { .. } => TeamMessageType::PersonQuestion,
            Self::PersonAnswer { .. } => TeamMessageType::PersonAnswer,
            Self::PersonMessage { .. } => TeamMessageType::PersonMessage,
            Self::Notice { .. } => TeamMessageType::Notice,
            Self::Done { .. } => TeamMessageType::Done,
            Self::Ended { .. } => TeamMessageType::Ended,
        }
    }

    /// Its text.
    #[must_use]
    pub fn text(&self) -> &str {
        match self {
            Self::Assign { text, .. }
            | Self::Question { text }
            | Self::Answer { text, .. }
            | Self::Report { text, .. }
            | Self::Ask { text }
            | Self::Decision { text, .. }
            | Self::PersonQuestion { text }
            | Self::PersonAnswer { text, .. }
            | Self::PersonMessage { text, .. }
            | Self::Notice { text, .. }
            | Self::Done { text }
            | Self::Ended { text, .. } => text,
        }
    }
}

/// Who wrote a stored message.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FromKind {
    /// The lead.
    Lead,
    /// A member.
    Member,
    /// A signed-in person.
    Person,
    /// The gateway.
    Gateway,
}

/// A stored message's author, set by the gateway from the credential.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TeamFrom {
    /// Who.
    pub kind: FromKind,
    /// A member's name or a person's display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Whom a stored message is for.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToKind {
    /// The lead.
    Lead,
    /// One member.
    Member,
    /// Every member.
    Members,
    /// The lead and every member.
    Team,
    /// The person the agent team works for (`person_question`); no agent reads it.
    Person,
}

/// A stored message's addressee.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TeamTo {
    /// Whom.
    pub kind: ToKind,
    /// The member's name, for `member`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// A stored message as a read answers it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TeamMessage {
    /// Its place in the agent team's log.
    pub seq: u64,
    /// Its author.
    pub from: TeamFrom,
    /// Its addressee.
    pub to: TeamTo,
    /// When it was stored, RFC 3339 in UTC.
    pub at: String,
    /// Its type and body.
    #[serde(flatten)]
    pub body: TeamMessageBody,
}

/// A post by a lead or a member; the gateway writes the other types.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TeamPost {
    /// The lead to one member (`assign`).
    Assign {
        /// The member, by name.
        to: String,
        /// The text.
        text: String,
    },
    /// The lead to every member (`ask_all`).
    Question {
        /// The text.
        text: String,
    },
    /// The lead's decision (`decide`).
    Decision {
        /// The text.
        text: String,
        /// The question it decides, if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        question_seq: Option<u64>,
    },
    /// The lead's question to the person (`ask_person`).
    PersonQuestion {
        /// The text.
        text: String,
    },
    /// A member's answer (`send_to_lead`).
    Answer {
        /// The question it answers.
        question_seq: u64,
        /// The text.
        text: String,
    },
    /// A member's report (`send_to_lead`), naming the files to hand back.
    Report {
        /// The text.
        text: String,
        /// Paths in the member's workspace.
        #[serde(default)]
        files: Vec<String>,
    },
    /// A member's own question (`send_to_lead`).
    Ask {
        /// The text.
        text: String,
    },
}

impl TeamPost {
    /// The stored type it becomes.
    #[must_use]
    pub const fn message_type(&self) -> TeamMessageType {
        match self {
            Self::Assign { .. } => TeamMessageType::Assign,
            Self::Question { .. } => TeamMessageType::Question,
            Self::Decision { .. } => TeamMessageType::Decision,
            Self::PersonQuestion { .. } => TeamMessageType::PersonQuestion,
            Self::Answer { .. } => TeamMessageType::Answer,
            Self::Report { .. } => TeamMessageType::Report,
            Self::Ask { .. } => TeamMessageType::Ask,
        }
    }

    /// Its text.
    #[must_use]
    pub fn text(&self) -> &str {
        match self {
            Self::Assign { text, .. }
            | Self::Question { text }
            | Self::Decision { text, .. }
            | Self::PersonQuestion { text }
            | Self::Answer { text, .. }
            | Self::Report { text, .. }
            | Self::Ask { text } => text,
        }
    }
}
