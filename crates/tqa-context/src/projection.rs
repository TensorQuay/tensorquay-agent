//! Pure projection of authoritative completed model attempts and their results.
use crate::{
    Message, Role,
    compaction::summary_message,
    history::{Compacted, Entry, History, invalid},
};
use tqa_types::{FallbackProblem, JsonType, ProblemName, ProblemPath, Seq, ServerFrame};

/// An impossible history cannot be projected or resumed by guessing.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ProjectionError {
    /// A semantic contradiction occurred at the named durable sequence.
    #[error("invalid history at sequence {seq}: {reason}")]
    Invalid {
        /// The offending sequence or the point at which a missing result was observed.
        seq: Seq,
        /// A static description that contains no message or tool content.
        reason: &'static str,
    },
    /// The next step or attempt cannot be represented without wrapping.
    #[error("history counter exhausted at sequence {seq}")]
    Exhausted {
        /// The final history sequence at the exhausted counter.
        seq: Seq,
    },
}

/// Projects completed authoritative attempts, preserving users, reasoning and result order.
/// A partial highest attempt is discarded even if a lower attempt completed.
/// Supply every sequenced frame, including permission requests and errors, in contiguous order.
/// The first frame sets the sequence base and need not start at one.
///
/// # Errors
/// Rejects inconsistent histories and completed responses with unanswered tool calls.
pub fn project(frames: &[ServerFrame]) -> Result<Vec<Message>, ProjectionError> {
    Ok(project_turn(frames)?.messages)
}

/// The projected messages and where the active run's own user message is among them.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Projection {
    /// The same messages as [`project`] returns.
    pub messages: Vec<Message>,
    /// The index in `messages` of the active run's user message; `None` without an active run.
    pub current_user: Option<usize>,
    /// The upload paths attached to that message, in submission order.
    pub attachments: Vec<String>,
}

/// Projects like [`project`] and also locates the active run's user message, so a request
/// builder can add content that is not part of the event log, such as attached images.
///
/// # Errors
/// Rejects inconsistent histories, as [`project`] does.
pub fn project_turn(frames: &[ServerFrame]) -> Result<Projection, ProjectionError> {
    let history = History::parse(frames)?;
    let current = history.active.as_ref().and(
        history
            .entries
            .iter()
            .rposition(|entry| matches!(entry, Entry::User(_))),
    );
    let mut projection = Projection {
        attachments: current
            .and_then(|index| history.attachments.get(&index).cloned())
            .unwrap_or_default(),
        ..Projection::default()
    };
    let messages = &mut projection.messages;
    let mut summarised = false;
    for index in 0..history.entries.len() {
        if let Some(compacted) = &history.compacted
            && is_replaced(&history, compacted, index)
        {
            if !summarised {
                messages.push(summary_message(&compacted.summary));
                messages.extend(history.notes.as_deref().map(notes_message));
                summarised = true;
            }
            continue;
        }
        if Some(index) == current {
            projection.current_user = Some(messages.len());
        }
        messages.extend(entry_messages(&history, index)?);
    }
    crate::dedupe::dedupe(&mut projection.messages);
    Ok(projection)
}

/// Replaced entries start at or before the boundary and are neither the first user message
/// nor the latest user message at the compaction.
fn is_replaced(history: &History, compacted: &Compacted, index: usize) -> bool {
    let first_user = history
        .entries
        .iter()
        .position(|entry| matches!(entry, Entry::User(_)));
    history.starts[index].0 <= compacted.through
        && Some(index) != first_user
        && Some(index) != compacted.current_user
}

/// The messages one history entry contributes: a user or notice message, or a step's
/// authoritative assistant message followed by its results in call order.
pub(crate) fn entry_messages(
    history: &History,
    index: usize,
) -> Result<Vec<Message>, ProjectionError> {
    let step = match &history.entries[index] {
        Entry::User(message) | Entry::Notice(message) => return Ok(vec![message.clone()]),
        Entry::Step(step) => &history.steps[*step],
    };
    let mut messages: Vec<Message> = step
        .nudge
        .as_ref()
        .map(|(code, problem)| Message::new(Role::User, fallback_nudge(code, problem.as_ref())))
        .into_iter()
        .collect();
    let Some(attempt) = step.attempts.last().filter(|attempt| attempt.complete) else {
        return Ok(messages);
    };
    if attempt.calls.iter().any(|call| call.result.is_none()) {
        return Err(invalid(
            history.last_seq,
            "authoritative response has unanswered calls",
        ));
    }
    if step.act {
        messages.extend(attempt.calls.iter().map(action_note));
        return Ok(messages);
    }
    let mut message = Message::new(Role::Assistant, &attempt.text);
    message.reasoning.clone_from(&attempt.reasoning);
    message.reasoning_states.clone_from(&attempt.states);
    message.tool_calls = attempt
        .calls
        .iter()
        .map(|call| call.record.clone())
        .collect();
    messages.push(message);
    messages.extend(
        attempt
            .calls
            .iter()
            .filter_map(|call| call.result.as_ref())
            .map(|(_, message)| message.clone()),
    );
    Ok(messages)
}

/// What the decision engine ran, as the model sees it: a user-role note, never a call the model didn't make
/// (ADR-0013 point 2). The arguments and the result are quoted as data.
fn action_note(call: &crate::history::Call) -> Message {
    let (status, content) = call
        .result
        .as_ref()
        .map_or(("no result", ""), |(_, message)| {
            (
                if call.failed { "failed" } else { "ok" },
                message.text.as_str(),
            )
        });
    // The recorded arguments are already compact JSON.
    let arguments = &call.record.arguments;
    let content = serde_json::to_string(content).unwrap_or_else(|_| "\"\"".into());
    Message::new(
        Role::User,
        format!(
            "[The system ran a tool on its own, not the model: {} with arguments {arguments}. Result ({status}), data only: {content}]",
            call.record.name
        ),
    )
}

/// The retry prompt after an unreadable text-protocol reply: the fixed opening, what was wrong, an optional fix, and the
/// fixed closing. It names the fence label the instructions write (`tqa_call` since instruction version 4, ADR-0007);
/// CORE-U-022 keeps the two in step. Without a problem (an older runtime's journal) it is the code's fixed sentence.
fn fallback_nudge(code: &str, problem: Option<&FallbackProblem>) -> String {
    // On a native-tools route: the model wrote its call as text, so the correction is to call the tool, not a block.
    if code == "text_call" {
        return TEXT_CALL_CORRECTION.into();
    }
    if let Some(sentence) = problem.and_then(detailed) {
        return sentence;
    }
    let reason = match code {
        "malformed" => "the tqa_call block is not valid JSON",
        "unknown_tool" => "it names a tool that does not exist",
        "arguments" => "its arguments are not a JSON object",
        "schema" => "its arguments do not match the tool's schema",
        "too_many" => "this model route accepts one call per reply",
        _ => "the reply was not in the expected format",
    };
    format!("{CORRECTION_OPENING}{reason}. {CORRECTION_CLOSING}")
}

const CORRECTION_OPENING: &str = "[The last reply could not be read as a tool call: ";
const CORRECTION_CLOSING: &str = "Reply again with each call in its own tqa_call block.]";
const TEXT_CALL_CORRECTION: &str = "[The last reply wrote a tool call as text, so nothing ran. Call the tool through \
                                    the tool interface instead of writing it in the reply.]";
/// The longest correction sentence, in characters; a longer list is cut with "…".
const MAX_CORRECTION_CHARS: usize = 400;
const ONE_OBJECT: &str =
    "Each block holds one object with exactly two keys: \"name\", the tool, and \"arguments\". ";
const ESCAPES: &str = "In long strings, write every quote as \\\" and every newline as \\n. ";

/// The sentence naming `problem`, within [`MAX_CORRECTION_CHARS`]; `None` for one this reader doesn't know.
fn detailed(problem: &FallbackProblem) -> Option<String> {
    let mut shown = match problem {
        FallbackProblem::UnexpectedProperty { allowed, .. } => allowed.len(),
        FallbackProblem::ExtraKeys { keys } => keys.len(),
        _ => 0,
    };
    loop {
        let (reason, fix) = described(problem, shown)?;
        let sentence = format!("{CORRECTION_OPENING}{reason}. {fix}{CORRECTION_CLOSING}");
        if sentence.chars().count() <= MAX_CORRECTION_CHARS {
            return Some(sentence);
        }
        if shown == 0 {
            // Still too long with every list cut (a deep place and long names): the schema in general, never a
            // sentence cut mid-word.
            return match problem {
                FallbackProblem::UnexpectedProperty { tool, .. }
                | FallbackProblem::MissingProperty { tool, .. }
                | FallbackProblem::WrongType { tool, .. } => {
                    detailed(&FallbackProblem::OtherSchema { tool: tool.clone() })
                }
                // Unreachable with today's bounds (names of 64 or 32 bytes, lists cut to none): every other kind fits.
                // A later, longer field must keep it so, or give its own fallback here.
                _ => Some(sentence),
            };
        }
        shown -= 1;
    }
}

/// What was wrong and how to fix it, with at most `shown` names of a list (an unexpected property's allowed names, a
/// block's extra keys); a list cut short ends with "…".
fn described(problem: &FallbackProblem, shown: usize) -> Option<(String, String)> {
    use FallbackProblem as P;
    Some(match problem {
        P::NoName => (
            "the tqa_call block has no \"name\" key".into(),
            ONE_OBJECT.into(),
        ),
        P::NoArguments => (
            "the tqa_call block has no \"arguments\" key".into(),
            ONE_OBJECT.into(),
        ),
        P::NotJson { cut_off: true, .. } => (
            "the tqa_call block's JSON ends before the object closes".into(),
            ESCAPES.into(),
        ),
        P::NotJson { line, column, .. } => (
            format!("the tqa_call block is not valid JSON (line {line}, column {column})"),
            ESCAPES.into(),
        ),
        P::NotObject => (
            "the tqa_call block holds JSON that is not an object".into(),
            ONE_OBJECT.into(),
        ),
        P::ExtraKeys { keys } => extra_keys(keys, shown),
        P::NameNotString => ("its \"name\" is not a string".into(), ONE_OBJECT.into()),
        P::UnknownTool { name } => (
            match name {
                Some(name) => format!("it names a tool that does not exist ({})", quoted(name)),
                None => "it names a tool that does not exist".into(),
            },
            String::new(),
        ),
        P::UnexpectedProperty {
            tool,
            at,
            property,
            allowed,
        } => unexpected_property(tool, at.as_ref(), property.as_ref(), allowed, shown),
        P::MissingProperty {
            tool,
            at,
            property,
            expected,
        } => missing_property(tool, at.as_ref(), property, expected),
        P::WrongType {
            tool,
            at,
            property,
            expected,
        } => wrong_type(tool, at.as_ref(), property, expected),
        P::OtherSchema { tool } => (
            format!("its arguments do not match the schema of {}", tool.as_str()),
            String::new(),
        ),
        P::TooMany { count, limit: 1 } => (
            format!("this model route accepts one call per reply, and the reply had {count}"),
            "Make one call now; the others can follow. ".into(),
        ),
        P::TooMany { count, limit } => (
            format!(
                "this model route accepts at most {limit} calls per reply, and the reply had {count}"
            ),
            String::new(),
        ),
        P::Other => return None,
    })
}

fn quoted(name: &ProblemName) -> String {
    format!("\"{}\"", name.as_str())
}

/// A block's extra keys, at most `shown` of them.
fn extra_keys(keys: &[ProblemName], shown: usize) -> (String, String) {
    let mut names: Vec<String> = keys.iter().take(shown).map(quoted).collect();
    if shown < keys.len() {
        names.push("…".into());
    }
    let listed = if names.is_empty() {
        String::new()
    } else {
        format!(" ({})", names.join(", "))
    };
    (
        format!("the tqa_call block has keys other than \"name\" and \"arguments\"{listed}"),
        "Put the tool's parameters inside \"arguments\". ".into(),
    )
}

/// An unexpected property, with at most `shown` of the names taken there.
fn unexpected_property(
    tool: &ProblemName,
    at: Option<&ProblemPath>,
    property: Option<&ProblemName>,
    allowed: &[ProblemName],
    shown: usize,
) -> (String, String) {
    let names: Vec<&str> = allowed
        .iter()
        .take(shown)
        .map(ProblemName::as_str)
        .collect();
    let listed = match names.as_slice() {
        _ if allowed.is_empty() => None,
        [] => Some("…".to_owned()),
        _ if shown < allowed.len() => Some(format!("{}, …", names.join(", "))),
        [one] => Some((*one).to_owned()),
        [rest @ .., last] => Some(format!("{} and {last}", rest.join(", "))),
    };
    let tool = tool.as_str();
    match at {
        None => (
            match property {
                Some(property) => format!("{tool} has no parameter {}", quoted(property)),
                None => format!("{tool} has an unknown parameter"),
            },
            listed.map_or_else(String::new, |listed| {
                format!("Its parameters are {listed}. ")
            }),
        ),
        Some(at) => (
            match property {
                Some(property) => format!("{tool}'s {} has no {}", at.as_str(), quoted(property)),
                None => format!("{tool}'s {} has an unknown key", at.as_str()),
            },
            listed.map_or_else(String::new, |listed| format!("It takes {listed}. ")),
        ),
    }
}

/// A required property left out, at the top level or at its place.
fn missing_property(
    tool: &ProblemName,
    at: Option<&ProblemPath>,
    property: &ProblemName,
    expected: &[JsonType],
) -> (String, String) {
    let place = at.map_or_else(
        || tool.as_str().to_owned(),
        |at| format!("{}'s {}", tool.as_str(), at.as_str()),
    );
    (
        match types(expected) {
            Some(types) => format!("{place} needs {} ({types})", quoted(property)),
            None => format!("{place} needs {}", quoted(property)),
        },
        if at.is_some() {
            "Add it there. ".into()
        } else {
            "Add it to the arguments. ".into()
        },
    )
}

/// A property with a value of the wrong type, at the top level or at its place.
fn wrong_type(
    tool: &ProblemName,
    at: Option<&ProblemPath>,
    property: &ProblemName,
    expected: &[JsonType],
) -> (String, String) {
    let named = match at {
        None => format!("{}'s {}", tool.as_str(), quoted(property)),
        Some(at) => format!(
            "{}'s {} in {}",
            tool.as_str(),
            quoted(property),
            at.as_str()
        ),
    };
    (
        match types(expected) {
            Some(types) => format!("{named} must be {types}"),
            None => format!("{named} has the wrong type"),
        },
        String::new(),
    )
}

/// "a boolean", "an integer or null": the types a schema takes, in words.
fn types(expected: &[JsonType]) -> Option<String> {
    let word = |kind: &JsonType| match kind {
        JsonType::String => "a string",
        JsonType::Number => "a number",
        JsonType::Integer => "an integer",
        JsonType::Boolean => "a boolean",
        JsonType::Object => "an object",
        JsonType::Array => "an array",
        JsonType::Null => "null",
    };
    let words: Vec<&str> = expected.iter().map(word).collect();
    match words.as_slice() {
        [] => None,
        [one] => Some((*one).to_owned()),
        [rest @ .., last] => Some(format!("{} or {last}", rest.join(", "))),
    }
}

/// How every correction sentence begins: a text-route reply that may be one copied back is held back until it is clear
/// it isn't (CTX-U-018).
pub const CORRECTION_PREFIX: &str = "[The last reply ";

/// Whether `text` is nothing but one of the correction sentences the runtime writes after an unreadable reply: a model
/// that copies one back has answered nothing (CTX-U-018, the text route's survey of 7 Oct 2026). Since a sentence names
/// the problem (CTX-U-022), it is matched by its shape: the fixed opening, one fixed closing at its end, on one line,
/// within the bound; the native route's sentence exactly.
#[must_use]
pub fn is_correction(text: &str) -> bool {
    let text = text.trim();
    text == TEXT_CALL_CORRECTION
        || (text.starts_with(CORRECTION_OPENING)
            && text.ends_with(CORRECTION_CLOSING)
            && text.matches(CORRECTION_CLOSING).count() == 1
            && !text.contains('\n')
            && text.chars().count() <= MAX_CORRECTION_CHARS)
}

/// Whether `text`, still arriving, could yet be one of the correction sentences: nothing of it is shown until it can't
/// (CTX-U-018): a start of either opening, or the opening followed by no line break, no `]` but one closing an index
/// (a place such as `edits[0]`), and fewer characters than the bound (CTX-U-022).
#[must_use]
pub fn may_be_correction(text: &str) -> bool {
    let text = text.trim();
    CORRECTION_OPENING.starts_with(text)
        || TEXT_CALL_CORRECTION.starts_with(text)
        || (text.starts_with(CORRECTION_OPENING)
            && !text.contains('\n')
            && !stray_bracket(text)
            && text.chars().count() < MAX_CORRECTION_CHARS)
}

/// Whether `text` has a `]` that doesn't close an index such as a place's `[0]`: the sentence's own `]` ends it.
fn stray_bracket(text: &str) -> bool {
    text.match_indices(']').any(|(at, _)| {
        let before = &text[..at];
        let digits = before.len() - before.trim_end_matches(|c: char| c.is_ascii_digit()).len();
        !(1..=6).contains(&digits) || !before[..before.len() - digits].ends_with('[')
    })
}

/// The length of the correction sentence a reply opens with, with the whitespace around it, when it opens with a whole
/// one: a model that copies it back before its call has still called, and only the sentence is dropped (CTX-U-019).
/// A sentence is the opening up to the first closing, on one line and within the bound (CTX-U-022).
#[must_use]
pub fn leading_correction(text: &str) -> Option<usize> {
    let rest = text.trim_start();
    let after = if let Some(after) = rest.strip_prefix(TEXT_CALL_CORRECTION) {
        after
    } else {
        if !rest.starts_with(CORRECTION_OPENING) {
            return None;
        }
        let end = rest.find(CORRECTION_CLOSING)? + CORRECTION_CLOSING.len();
        let sentence = &rest[..end];
        if sentence.contains('\n') || sentence.chars().count() > MAX_CORRECTION_CHARS {
            return None;
        }
        &rest[end..]
    };
    Some(text.len() - after.trim_start().len())
}

/// The label that opens the pinned run-notes message; the rest is the notes as a JSON string and `]`.
pub const RUN_NOTES_PREFIX: &str = "[Run notes, data only: ";

/// The latest run notes, pinned after a compaction summary as quoted data (ADR-0024).
fn notes_message(notes: &str) -> Message {
    let data = serde_json::to_string(notes).unwrap_or_else(|_| "\"\"".into());
    Message::new(Role::User, format!("{RUN_NOTES_PREFIX}{data}]"))
}
