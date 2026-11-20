//! Channel messages (ADR-0014): the typed messages of a channel's append-only log, what a member may post, the roles,
//! the bounds and the filter `channel_read` and `channel_wait` select with. The gateway writes `decision` messages and
//! `task_claim` writes `claim` messages; neither is ever posted.
use crate::{ErrorCode, FrameError};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::IgnoredAny};

/// The largest message body, as serialized JSON.
pub const MAX_CHANNEL_BODY_BYTES: usize = 16 * 1024;
/// The most members a channel has.
pub const MAX_CHANNEL_MEMBERS: usize = 32;
/// The most messages a channel takes in a day.
pub const MAX_CHANNEL_MESSAGES_PER_DAY: u32 = 5_000;
/// The most messages one read returns.
pub const MAX_CHANNEL_READ: u32 = 200;
/// The longest a `channel_wait` waits, in seconds; an agent that wants longer waits again.
pub const MAX_CHANNEL_WAIT_SECONDS: u32 = 300;
/// A proposal's voting window when it names none, in seconds.
pub const DEFAULT_PROPOSAL_WINDOW_SECONDS: u32 = 600;
/// The shortest and longest voting window a proposal may name, in seconds.
pub const PROPOSAL_WINDOW_SECONDS: std::ops::RangeInclusive<u32> = 60..=86_400;

const MAX_TITLE_CHARS: usize = 200;
const MAX_REASON_CHARS: usize = 1_000;
const MAX_REFS: usize = 16;
const MAX_REF_BYTES: usize = 512;

/// A member's role: leads and members post and vote; observers only read.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChannelRole {
    /// Posts and votes, as a member does; v1.1's `lead` rule gives the lead's vote the decision.
    Lead,
    /// Posts and votes.
    Member,
    /// Reads only.
    Observer,
}

impl ChannelRole {
    /// Whether a member with this role may post.
    #[must_use]
    pub const fn may_post(self) -> bool {
        !matches!(self, Self::Observer)
    }

    /// Whether a member with this role votes on proposals.
    #[must_use]
    pub const fn may_vote(self) -> bool {
        !matches!(self, Self::Observer)
    }
}

/// How the gateway decides a proposal.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DecisionRule {
    /// The first approve or reject by a human voting member.
    Human,
    /// More than half of the voting members.
    Majority,
}

/// A vote; `veto` counts only from a human voting member, while the window is open.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VoteChoice {
    /// For the proposal.
    Accept,
    /// Against the proposal.
    Reject,
    /// Stops the proposal at once; a human voting member's only.
    Veto,
}

/// A decided proposal's outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// More than half of the voting members accepted, or a human approved.
    Accepted,
    /// More than half rejected, or a human rejected.
    Rejected,
    /// Every voting member voted and it was a tie.
    Undecided,
    /// The window closed first.
    TimedOut,
    /// A human voting member vetoed it while the window was open.
    Vetoed,
}

/// Where a claimed task stands in its claimant's report.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReportStatus {
    /// The task is done.
    Completed,
    /// The task could not be done.
    Failed,
    /// The claimant is waiting on something.
    Blocked,
}

/// Who wrote a message: a member conversation's agent, a user, or the gateway (decisions).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AuthorKind {
    /// A member conversation's agent.
    Conversation,
    /// A person.
    User,
    /// The gateway itself (decisions).
    Gateway,
}

/// A message's author, always set by the gateway from the credential.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Author {
    /// Who wrote it.
    pub kind: AuthorKind,
    /// The conversation's or user's ID; `gateway` for the gateway.
    pub id: String,
    /// A display name: the agent profile's or the user's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// How many claimants a task takes (ADR-0014 point 8): a number, 1 by default, or every voting member. `task_claim`
/// is granted in order until the limit is reached. In JSON, a number or `"all"`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClaimLimit {
    /// At most this many claimants, from 1 to [`MAX_CHANNEL_MEMBERS`].
    Count(u32),
    /// Every voting member of the channel.
    All,
}

impl Default for ClaimLimit {
    fn default() -> Self {
        Self::Count(1)
    }
}

impl ClaimLimit {
    /// How many claims the task takes in a channel with `voting_members`.
    #[must_use]
    pub const fn of(self, voting_members: u32) -> u32 {
        match self {
            Self::Count(count) => count,
            Self::All => voting_members,
        }
    }

    /// Whether it is the default, one claimant.
    #[must_use]
    pub const fn is_one(&self) -> bool {
        matches!(self, Self::Count(1))
    }
}

impl Serialize for ClaimLimit {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Count(count) => serializer.serialize_u32(*count),
            Self::All => serializer.serialize_str("all"),
        }
    }
}

impl<'de> Deserialize<'de> for ClaimLimit {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(LimitVisitor)
    }
}

/// Reads a claim limit: a whole number or `"all"`.
struct LimitVisitor;

impl serde::de::Visitor<'_> for LimitVisitor {
    type Value = ClaimLimit;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a whole number of claimants or \"all\"")
    }

    fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<ClaimLimit, E> {
        u32::try_from(value)
            .map(ClaimLimit::Count)
            .map_err(|_| E::custom("too many claimants"))
    }

    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<ClaimLimit, E> {
        if value == "all" {
            return Ok(ClaimLimit::All);
        }
        Err(E::invalid_value(serde::de::Unexpected::Str(value), &self))
    }
}

impl JsonSchema for ClaimLimit {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ClaimLimit".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "oneOf": [
                {"type": "integer", "minimum": 1, "maximum": MAX_CHANNEL_MEMBERS},
                {"const": "all"}
            ]
        })
    }
}

/// A message's type, as named in its body's `type` and in filters.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    /// Free text.
    Text,
    /// A proposal to vote on.
    Proposal,
    /// A vote on a proposal.
    Vote,
    /// A proposal's outcome.
    Decision,
    /// A task to claim.
    Task,
    /// A task's claim.
    Claim,
    /// A claimant's report.
    Report,
    /// The work is finished.
    Done,
    /// Written only by the gateway, for people (ADR-0014's amendment of 20 Nov 2026): never in an agent's reads.
    Notice,
}

impl MessageType {
    /// Every message type an agent reads or posts; a notice is the page's alone.
    pub const ALL: [Self; 8] = [
        Self::Text,
        Self::Proposal,
        Self::Vote,
        Self::Decision,
        Self::Task,
        Self::Claim,
        Self::Report,
        Self::Done,
    ];
}

/// A message's typed body.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum MessageBody {
    /// Free text.
    Text {
        /// The text.
        text: String,
    },
    /// A proposal to vote on.
    Proposal {
        /// A one-line title.
        title: String,
        /// What is proposed and why.
        text: String,
        /// The voting window in seconds; [`DEFAULT_PROPOSAL_WINDOW_SECONDS`] when absent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window_seconds: Option<u32>,
    },
    /// A vote on a proposal.
    Vote {
        /// The proposal's seq.
        proposal_seq: u64,
        /// The vote.
        choice: VoteChoice,
        /// Why, in a sentence or two.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// Written only by the gateway.
    Decision {
        /// The proposal's seq.
        proposal_seq: u64,
        /// The outcome.
        outcome: Outcome,
        /// The rule that decided it.
        rule: DecisionRule,
    },
    /// A task voting members claim, up to its limit.
    Task {
        /// A one-line title.
        title: String,
        /// What is to be done.
        spec: String,
        /// How many claimants it takes; one when absent.
        #[serde(default, skip_serializing_if = "ClaimLimit::is_one")]
        claims: ClaimLimit,
    },
    /// Written only by `task_claim`.
    Claim {
        /// The task's seq.
        task_seq: u64,
    },
    /// The claimant's report on its task.
    Report {
        /// The task's seq.
        task_seq: u64,
        /// Where the task stands.
        status: ReportStatus,
        /// What was done.
        summary: String,
        /// Commits, files or links the report points at.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        refs: Vec<String>,
    },
    /// The work is finished.
    Done {
        /// A closing summary.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },
    /// Written only by the gateway, for people: a line about the channel itself, in the gateway's fixed words (ADR-0014's
    /// amendment of 20 Nov 2026, A5, A6, A10). Never in an agent's reads.
    Notice {
        /// What it says.
        kind: ChannelNoticeKind,
        /// The member it is about, by its task's name, so a page words it in the reader's language.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        member: Option<String>,
        /// That member's id (its task's), so a page names it as Members does: its owner sees the title of their own
        /// unnamed agent, never two members alike as "Agent" (decided on 21 Nov 2026, C-15).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        member_id: Option<String>,
        /// A woken run's failure code, so a page words it as the task page does (decided on 20 Nov 2026, C-6).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
        /// The sentence.
        text: String,
    },
}

/// What a channel's notice says (ADR-0014's amendment of 20 Nov 2026).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum ChannelNoticeKind {
    /// Woken runs have spent 80 % of the channel's budget (A5).
    #[serde(rename = "wake_budget_80")]
    WakeBudget80,
    /// Woken runs have spent the channel's budget: no agent is woken any more (A5).
    #[serde(rename = "wake_budget_spent")]
    WakeBudgetSpent,
    /// Agents here stopped waking each other: a person's post starts them again (A6).
    #[serde(rename = "wakes_bounded")]
    WakesBounded,
    /// A member can't be woken: its environment's runtime is too old (A10).
    #[serde(rename = "unreachable_runtime")]
    UnreachableRuntime,
    /// A member can't be woken: its task was deleted (A10).
    #[serde(rename = "member_deleted")]
    MemberDeleted,
    /// A person set the channel's wake budget; `member` names them (A5, decided on 20 Nov 2026).
    #[serde(rename = "wake_budget_set")]
    WakeBudgetSet,
    /// A run a wake started failed; `member` names it and `code` says why (decided on 20 Nov 2026, C-6).
    #[serde(rename = "woken_run_failed")]
    WokenRunFailed,
    /// A run a wake started was stopped; `member` names it (C-6).
    #[serde(rename = "woken_run_stopped")]
    WokenRunStopped,
}

impl MessageBody {
    /// The body's type.
    #[must_use]
    pub const fn message_type(&self) -> MessageType {
        match self {
            Self::Text { .. } => MessageType::Text,
            Self::Proposal { .. } => MessageType::Proposal,
            Self::Vote { .. } => MessageType::Vote,
            Self::Decision { .. } => MessageType::Decision,
            Self::Task { .. } => MessageType::Task,
            Self::Claim { .. } => MessageType::Claim,
            Self::Report { .. } => MessageType::Report,
            Self::Done { .. } => MessageType::Done,
            Self::Notice { .. } => MessageType::Notice,
        }
    }

    /// The task a message is about: a claim's or a report's.
    #[must_use]
    pub const fn task_seq(&self) -> Option<u64> {
        match self {
            Self::Claim { task_seq } | Self::Report { task_seq, .. } => Some(*task_seq),
            _ => None,
        }
    }

    /// Checks each field's bounds.
    ///
    /// # Errors
    /// `invalid_frame` naming the field (`body.<field>`).
    pub fn validate(&self) -> Result<(), FrameError> {
        match self {
            Self::Text { text } | Self::Notice { text, .. } => required("body.text", text),
            Self::Proposal {
                title,
                text,
                window_seconds,
            } => {
                self::title(title)?;
                required("body.text", text)?;
                match window_seconds {
                    Some(seconds) if !PROPOSAL_WINDOW_SECONDS.contains(seconds) => Err(invalid(
                        "body.window_seconds",
                        "expected 60 to 86400 seconds",
                    )),
                    _ => Ok(()),
                }
            }
            Self::Vote {
                proposal_seq,
                reason,
                ..
            } => {
                sequence("body.proposal_seq", *proposal_seq)?;
                match reason {
                    Some(reason) if reason.chars().count() > MAX_REASON_CHARS => {
                        Err(invalid("body.reason", "expected at most 1000 characters"))
                    }
                    _ => Ok(()),
                }
            }
            Self::Decision { proposal_seq, .. } => sequence("body.proposal_seq", *proposal_seq),
            Self::Task {
                title,
                spec,
                claims,
            } => {
                self::title(title)?;
                required("body.spec", spec)?;
                match claims {
                    ClaimLimit::Count(count)
                        if usize::try_from(*count)
                            .map_or(true, |count| !(1..=MAX_CHANNEL_MEMBERS).contains(&count)) =>
                    {
                        Err(invalid("body.claims", "expected 1 to 32, or \"all\""))
                    }
                    _ => Ok(()),
                }
            }
            Self::Claim { task_seq } => sequence("body.task_seq", *task_seq),
            Self::Report {
                task_seq,
                summary,
                refs,
                ..
            } => {
                sequence("body.task_seq", *task_seq)?;
                required("body.summary", summary)?;
                if refs.len() > MAX_REFS
                    || refs
                        .iter()
                        .any(|item| item.is_empty() || item.len() > MAX_REF_BYTES)
                {
                    return Err(invalid(
                        "body.refs",
                        "expected at most 16 references of 1 to 512 bytes",
                    ));
                }
                Ok(())
            }
            Self::Done { .. } => Ok(()),
        }
    }
}

/// The gateway's wake of a channel member (revision 16, ADR-0014's amendment of 20 Nov 2026, A4): the channel and the
/// member's place, after which messages wait for it. Nothing written by anyone else: those reach the model only through
/// `channel_read`'s marked block.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChannelWake {
    /// The channel's id.
    pub channel: crate::ChannelId,
    /// The member's place in the channel's log: the waiting messages come after it.
    pub after: u64,
}

/// A message in a channel's log.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChannelMessage {
    /// Contiguous per channel, from 1.
    pub seq: u64,
    /// When the gateway appended it, RFC 3339.
    pub ts: String,
    /// Who wrote it.
    pub author: Author,
    /// The message this one replies to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<u64>,
    /// The typed body.
    pub body: MessageBody,
    /// The files posted with it (ADR-0014's amendment of 20 Nov 2026, A9; revision 16); none when absent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<ChannelFile>,
}

/// A file posted with a channel message (ADR-0014's amendment of 20 Nov 2026, A9).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChannelFile {
    /// Its name as posted.
    pub name: String,
    /// Its size in bytes.
    pub size: u64,
    /// Where it is in the reading agent member's workspace, once placed there; absent for people and before then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Why it isn't in the reading agent member's workspace and won't be (`uploads_full`, `upload_too_large`); absent
    /// when it is, or may still be, placed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_placed: Option<String>,
}

/// What a member posts: a body and an optional parent. An `author` in the request is accepted and dropped: the
/// gateway sets the author from the credential (SEC-015).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::manual_non_exhaustive,
    reason = "the private field accepts and drops a forged author (SEC-015); it does not mark the struct as extensible"
)]
pub struct ChannelPost {
    /// The message this one replies to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<u64>,
    /// The typed body.
    pub body: MessageBody,
    #[serde(default, skip_serializing, deserialize_with = "dropped")]
    #[schemars(skip)]
    author: (),
}

impl ChannelPost {
    /// A post with no parent.
    #[must_use]
    pub const fn new(body: MessageBody) -> Self {
        Self {
            parent: None,
            body,
            author: (),
        }
    }

    /// Parses and validates a post.
    ///
    /// # Errors
    /// `invalid_frame` naming the field: unparsable JSON, an unknown field or type, a `decision` or `claim` body, or a
    /// bound.
    pub fn parse(bytes: &[u8]) -> Result<Self, FrameError> {
        let post: Self =
            serde_json::from_slice(bytes).map_err(|error| invalid("$", &error.to_string()))?;
        post.validate()?;
        Ok(post)
    }

    /// Checks what a member may post and its bounds.
    ///
    /// # Errors
    /// As [`ChannelPost::parse`].
    pub fn validate(&self) -> Result<(), FrameError> {
        match self.body {
            MessageBody::Decision { .. } => {
                return Err(invalid("body.type", "a decision is written by the gateway"));
            }
            MessageBody::Claim { .. } => {
                return Err(invalid("body.type", "a claim is made with task_claim"));
            }
            MessageBody::Notice { .. } => {
                return Err(invalid("body.type", "a notice is written by the gateway"));
            }
            _ => {}
        }
        if let Some(parent) = self.parent {
            sequence("parent", parent)?;
        }
        let size = serde_json::to_vec(&self.body).map_or(usize::MAX, |bytes| bytes.len());
        if size > MAX_CHANNEL_BODY_BYTES {
            return Err(invalid("body", "expected at most 16384 bytes of JSON"));
        }
        self.body.validate()
    }
}

/// What `channel_read` and `channel_wait` select; an empty filter selects every message.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChannelFilter {
    /// Only these types; all when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub types: Vec<MessageType>,
    /// Only replies to this message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<u64>,
    /// Only this task's message, its claim and its reports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_seq: Option<u64>,
    /// Only messages by this kind of author.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_kind: Option<AuthorKind>,
}

impl ChannelFilter {
    /// Parses and validates a filter.
    ///
    /// # Errors
    /// `invalid_frame` naming the field.
    pub fn parse(bytes: &[u8]) -> Result<Self, FrameError> {
        let filter: Self =
            serde_json::from_slice(bytes).map_err(|error| invalid("$", &error.to_string()))?;
        if let Some(parent) = filter.parent {
            sequence("parent", parent)?;
        }
        if let Some(task) = filter.task_seq {
            sequence("task_seq", task)?;
        }
        Ok(filter)
    }

    /// Whether `message` is selected.
    #[must_use]
    pub fn matches(&self, message: &ChannelMessage) -> bool {
        (self.types.is_empty() || self.types.contains(&message.body.message_type()))
            && self
                .parent
                .is_none_or(|parent| message.parent == Some(parent))
            && self.task_seq.is_none_or(|task| {
                message.seq == task && message.body.message_type() == MessageType::Task
                    || message.body.task_seq() == Some(task)
            })
            && self
                .author_kind
                .is_none_or(|kind| message.author.kind == kind)
    }
}

fn invalid(path: &str, detail: &str) -> FrameError {
    FrameError {
        code: ErrorCode::InvalidFrame,
        path: path.into(),
        detail: detail.into(),
    }
}

fn required(path: &str, text: &str) -> Result<(), FrameError> {
    if text.trim().is_empty() {
        return Err(invalid(path, "expected a non-empty text"));
    }
    Ok(())
}

fn title(title: &str) -> Result<(), FrameError> {
    let characters = title.chars().count();
    if !(1..=MAX_TITLE_CHARS).contains(&characters)
        || title.trim().is_empty()
        || title.chars().any(char::is_control)
    {
        return Err(invalid(
            "body.title",
            "expected 1 to 200 characters without control characters",
        ));
    }
    Ok(())
}

fn sequence(path: &str, seq: u64) -> Result<(), FrameError> {
    if seq == 0 {
        return Err(invalid(path, "expected a sequence number from 1"));
    }
    Ok(())
}

/// Reads and drops any value.
fn dropped<'de, D: Deserializer<'de>>(deserializer: D) -> Result<(), D::Error> {
    IgnoredAny::deserialize(deserializer).map(|_| ())
}
