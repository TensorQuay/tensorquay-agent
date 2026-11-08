//! Short task titles: after a task's first run completes, one small call on the task's own route names it in 3 to
//! 6 words. The call goes the relay's way (the runtime's encoding, the relay's shaping, the route's key and limiter,
//! never queueing behind real traffic), is recorded as `title` usage, and changes a title only while nobody has set it
//! (`title_source` is still `message`). Real work always comes first: the call gives up its slot the moment any request
//! waits on the route, and on a one-slot route it isn't made while the task has a message waiting, and a message for
//! the task cancels it at once. Off with the file's
//! `[tasks] auto_title = false`.
use super::request::{self, Operation};
use crate::{GatewayError, app::State};
use secrecy::SecretString;
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use tqa_llm::{
    Completion, CompletionRequest, ContentPart, FinishReason, HttpConfig, Message, Protocol, Role,
    Route, StreamLimits,
};
use tqa_types::{ConversationId, ServerFrame};

/// The most characters of the task's own words the call carries.
const MAX_ASKED_CHARS: usize = 1_000;
/// The call's output tokens on a plain route, and on one that reasons before it answers.
const PLAIN_TOKENS: u32 = 32;
const REASONING_TOKENS: u32 = 256;
/// The room a model that thinks unasked gets for a second try, within the route's own limit (GW-U-099).
const THINKING_ROOM: u32 = 2048;
/// A title the gateway takes: at most this many words and characters.
const MAX_WORDS: usize = 8;
const MAX_CHARS: usize = 80;
/// The whole call's bound, short: it takes a route slot only when one is free, and must never hold one long (a slow
/// or scripted model answers a task first).
const DEADLINE: Duration = Duration::from_secs(20);
/// How often a running title call looks for a request waiting on its route, which it then gives the slot to.
const WAITER_CHECK: Duration = Duration::from_millis(25);

/// The title calls on one-slot routes, by task, each cancelled by the task's next message.
#[derive(Default)]
pub(super) struct Calls(Mutex<(u64, Running)>);

/// Each task's running call: its serial number (so an ended call never removes a newer one) and its token.
type Running = HashMap<ConversationId, (u64, CancellationToken)>;

/// One registered call; it leaves the table however the call ends.
struct Registered<'a> {
    calls: &'a Calls,
    id: ConversationId,
    serial: u64,
    token: CancellationToken,
}

impl Calls {
    fn register(&self, id: &ConversationId) -> Registered<'_> {
        let token = CancellationToken::new();
        let mut calls = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        calls.0 += 1;
        let serial = calls.0;
        calls.1.insert(id.clone(), (serial, token.clone()));
        Registered {
            calls: self,
            id: id.clone(),
            serial,
            token,
        }
    }
}

impl Drop for Registered<'_> {
    fn drop(&mut self) {
        let mut calls = self.calls.0.lock().unwrap_or_else(PoisonError::into_inner);
        if calls
            .1
            .get(&self.id)
            .is_some_and(|(serial, _)| *serial == self.serial)
        {
            calls.1.remove(&self.id);
        }
    }
}

/// A message for the task arrived: its title call, if one holds a one-slot route, stops now.
pub(crate) fn interrupt(state: &State, id: &ConversationId) {
    if let Some((_, token)) = state
        .relay
        .titles
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .1
        .remove(id)
    {
        token.cancel();
    }
}

/// The call: a one-line instruction and the task's own words, cut short; no tools.
pub(crate) fn request(asked: &str, route: &Route) -> CompletionRequest {
    let asked: String = asked.chars().take(MAX_ASKED_CHARS).collect();
    let wanted = if route.reasoning.is_some() {
        REASONING_TOKENS
    } else {
        PLAIN_TOKENS
    };
    // Naming the writing system keeps a small model from answering in another language (F23, GW-U-096).
    let instruction = script(&asked).map_or_else(
        || "Name this task in 3 to 6 words, in its own language. Reply with the name only.".to_owned(),
        |script| {
            format!(
                "Name this task in 3 to 6 words, in the same language as the task below, which is written in {}. \
                 Reply with the name only.",
                script.named()
            )
        },
    );
    CompletionRequest {
        messages: vec![
            message(Role::System, &instruction),
            message(Role::User, &asked),
        ],
        max_output: Some(
            route
                .limits
                .max_output
                .map_or(wanted, |most| wanted.min(most)),
        ),
        ..CompletionRequest::default()
    }
}

/// A writing system, as the task's words and a title are written in (GW-U-096).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Script {
    Latin,
    Han,
    /// Kana with or without kanji.
    Japanese,
    Hangul,
    Thai,
    Tamil,
    Cyrillic,
    Arabic,
    Devanagari,
}
impl Script {
    /// How the instruction names it.
    const fn named(self) -> &'static str {
        match self {
            Self::Latin => "the Latin alphabet",
            Self::Han => "Chinese characters",
            Self::Japanese => "Japanese",
            Self::Hangul => "Korean Hangul",
            Self::Thai => "Thai script",
            Self::Tamil => "Tamil script",
            Self::Cyrillic => "the Cyrillic alphabet",
            Self::Arabic => "Arabic script",
            Self::Devanagari => "Devanagari",
        }
    }
}

/// The writing system of one character, if it is a letter of one we know. Kana is Japanese; kanji counts as Han.
const fn letter(character: char) -> Option<Script> {
    Some(match character as u32 {
        0x41..=0x5A | 0x61..=0x7A | 0xC0..=0xD6 | 0xD8..=0xF6 | 0xF8..=0x24F | 0x1E00..=0x1EFF => {
            Script::Latin
        }
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x2_0000..=0x2_FFFF => Script::Han,
        0x3040..=0x30FF | 0x31F0..=0x31FF | 0xFF66..=0xFF9F => Script::Japanese,
        0x1100..=0x11FF | 0x3130..=0x318F | 0xAC00..=0xD7AF => Script::Hangul,
        0x0E00..=0x0E7F => Script::Thai,
        0x0B80..=0x0BFF => Script::Tamil,
        0x0400..=0x04FF => Script::Cyrillic,
        0x0600..=0x06FF => Script::Arabic,
        0x0900..=0x097F => Script::Devanagari,
        _ => return None,
    })
}

/// The writing system most of a text's letters are in: a letter of Latin counts once and one of any other script three
/// times (a Chinese character or a syllable carries about a word), so `CSV` inside a Chinese sentence doesn't make it
/// Latin. Any kana makes Han letters Japanese. `None` for a text with no letters.
pub(crate) fn script(text: &str) -> Option<Script> {
    const ALL: [Script; 9] = [
        Script::Latin,
        Script::Han,
        Script::Japanese,
        Script::Hangul,
        Script::Thai,
        Script::Tamil,
        Script::Cyrillic,
        Script::Arabic,
        Script::Devanagari,
    ];
    let mut counts = [0_usize; 9];
    for found in text.chars().filter_map(letter) {
        if let Some(index) = ALL.iter().position(|script| *script == found) {
            counts[index] += 1;
        }
    }
    let (han, kana) = (1, 2);
    if counts[kana] > 0 {
        counts[kana] += counts[han];
        counts[han] = 0;
    }
    ALL.iter()
        .zip(counts)
        .filter(|(_, count)| *count > 0)
        .map(|(script, count)| {
            (
                *script,
                if *script == Script::Latin {
                    count
                } else {
                    count * 3
                },
            )
        })
        .max_by_key(|(_, weight)| *weight)
        .map(|(script, _)| script)
}

/// Whether a title is in the writing system of the task's words (GW-U-096): a Japanese task may be named in kanji
/// alone. With no letters on either side there is nothing to compare, so it fits.
pub(crate) fn fits(task: &str, title: &str) -> bool {
    match (script(task), script(title)) {
        (Some(Script::Japanese), Some(Script::Han)) | (None, _) | (_, None) => true,
        (Some(task), Some(title)) => task == title,
    }
}

fn message(role: Role, text: &str) -> Message {
    Message {
        role,
        content: vec![ContentPart::Text(text.into())],
        tool_calls: vec![],
        tool_call_id: None,
        name: None,
    }
}

/// Reasoning blocks a local model may write before its answer (`<think>…</think>` and its kin).
const REASONING_TAGS: [&str; 4] = ["think", "thinking", "reasoning", "reflection"];

/// The answer after any leading reasoning blocks; `None` for one never closed (the model ran out while reasoning).
fn after_reasoning(answer: &str) -> Option<&str> {
    let mut rest = answer.trim_start();
    'blocks: loop {
        for tag in REASONING_TAGS {
            let open = format!("<{tag}>");
            if rest
                .get(..open.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(&open))
            {
                let close = format!("</{tag}>");
                let end = rest.to_ascii_lowercase().find(&close)?;
                rest = rest[end + close.len()..].trim_start();
                continue 'blocks;
            }
        }
        return Some(rest);
    }
}

/// The answer as a title: its first non-empty line after any reasoning block, without a `Title:` label (plain or in
/// Markdown), quotes, emphasis, control characters or a closing full stop; `None` for markup, or unless it is 1 to 8
/// words and at most 80 characters.
pub(crate) fn clean(answer: &str) -> Option<String> {
    let line = after_reasoning(answer)?
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    if line.starts_with('<') {
        return None;
    }
    let unmarked = line.trim_start_matches(['*', '_']);
    let line = unmarked
        .get(..6)
        .filter(|label| label.eq_ignore_ascii_case("title:"))
        .map_or(line, |_| unmarked[6..].trim_start_matches(['*', '_']));
    let kept: String = line
        .chars()
        .filter(|character| !character.is_control())
        .collect();
    // Quotes, emphasis and a closing full stop, in any order ("Name". or "Name.").
    let mut trimmed = kept.as_str();
    loop {
        let next = trimmed
            .trim()
            .trim_matches(|character: char| {
                matches!(character, '"' | '\'' | '`' | '*' | '“' | '”' | '«' | '»')
            })
            .trim_end_matches(['.', '!', '。']);
        if next == trimmed {
            break;
        }
        trimmed = next;
    }
    let words: Vec<&str> = trimmed.split_whitespace().collect();
    let title = words.join(" ");
    ((1..=MAX_WORDS).contains(&words.len()) && title.chars().count() <= MAX_CHARS).then_some(title)
}

/// Names the task after its first completed run, in the background; a failure is only logged (the task keeps the
/// title it has).
pub(crate) fn after_first_run(state: &Arc<State>, id: &ConversationId) {
    if !state.config.tasks.auto_title {
        return;
    }
    let (state, id) = (Arc::clone(state), id.clone());
    let tasks = state.tasks.clone();
    tasks.spawn(async move {
        let named = tokio::select! {biased;
            () = state.cancel.cancelled() => Ok(()),
            named = tokio::time::timeout(DEADLINE, name(&state, &id)) => named.unwrap_or(Err(GatewayError::Timeout)),
        };
        if let Err(error) = named {
            tracing::warn!(conversation_id = %id, code = error.code(), "the task was not given a short title");
        }
    });
}

async fn name(state: &State, id: &ConversationId) -> Result<(), GatewayError> {
    let conversation = crate::conversations::uuid(id)?;
    let task = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query!(
            "SELECT c.title, c.title_source, c.model_route, c.owner_id, \
             (SELECT count(*) FROM events e WHERE e.conversation_id=c.id AND e.lifecycle='run.completed') AS \"completed!\" \
             FROM conversations c WHERE c.id=$1 AND c.state='active'",
            conversation
        )
        .fetch_optional(&state.pool)
        .await?)
    })
    .await?;
    // Only after the first completed run, and only a title nobody has set.
    let Some(task) = task.filter(|task| task.title_source == "message" && task.completed == 1)
    else {
        return Ok(());
    };
    let Some(route) = state
        .settings
        .current()
        .registry
        .get(&task.model_route)
        .cloned()
    else {
        return Ok(());
    };
    let reader = crate::secrets::reads::ReadContext {
        conversation: Some(conversation),
        user: Some(task.owner_id),
        run: None,
    };
    let secret = super::endpoint::credential(state, &route, &reader).await?;
    // Never queue behind the route's real traffic: a busy route leaves the title as it is.
    let limiter = state.relay.limiters.for_route(&route);
    let Ok(_permit) = limiter.try_acquire() else {
        return Ok(());
    };
    // On a one-slot route the person's next message cancels the call; one already waiting means no call at all.
    let one_slot = route.limits.concurrency == Some(1);
    let registered = one_slot.then(|| state.relay.titles.register(id));
    if one_slot && waiting(state, id).await? {
        return Ok(());
    }
    let cancelled = registered
        .as_ref()
        .map_or_else(CancellationToken::new, |call| call.token.clone());
    let mut asked = request(&task.title, &route);
    let idle = Duration::from_secs(state.config.relay.idle_timeout);
    // Any request waiting on the route, from any task, takes the slot: a title never delays real work.
    let waiter = async {
        while limiter.queued() == 0 {
            tokio::time::sleep(WAITER_CHECK).await;
        }
    };
    let answer = tokio::select! {
        () = cancelled.cancelled() => return Ok(()),
        () = waiter => return Ok(()),
        answer = answer(&state.relay.client, idle, &route, secret.as_ref(), &mut asked) => answer?,
    };
    let (answer, usage) = answer;
    record(state, conversation, &route.name, usage.as_ref()).await?;
    let Some(title) = usable(id, &route.name, &task.title, &answer) else {
        return Ok(());
    };
    let named = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query_scalar!(
            "UPDATE conversations SET title=$2,title_source='model',updated_at=CURRENT_TIMESTAMP \
             WHERE id=$1 AND title_source='message' RETURNING id",
            conversation,
            title
        )
        .fetch_optional(&state.pool)
        .await?)
    })
    .await?;
    if named.is_some() {
        state
            .events
            .control(id, ServerFrame::TaskTitled { title })?;
    }
    Ok(())
}

/// The answer as the task's title, or why not, at warn: a model that named nothing usable, or named it in another
/// writing system than the task's (GW-U-096), leaves the task its first words.
fn usable(id: &ConversationId, route: &str, task: &str, answer: &Completion) -> Option<String> {
    let Some(title) = clean(&answer.text) else {
        tracing::warn!(
            conversation_id = %id,
            route,
            finish = ?answer.finish_reason,
            "the task was not given a short title: the model's answer held no usable name"
        );
        return None;
    };
    if !fits(task, &title) {
        tracing::warn!(
            conversation_id = %id,
            route,
            "the task was not given a short title: the model answered in another writing system"
        );
        return None;
    }
    Some(title)
}

/// Whether the task has a message waiting to go to its runtime.
async fn waiting(state: &State, id: &ConversationId) -> Result<bool, GatewayError> {
    let conversation = crate::conversations::uuid(id)?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query_scalar!(
            "SELECT EXISTS(SELECT 1 FROM idempotency WHERE conversation_id=$1 AND route=$2 AND execution_status='queued') AS \"waiting!\"",
            conversation,
            crate::actions::held::route(id)
        )
        .fetch_one(&state.pool)
        .await?)
    })
    .await
}

/// The call's usage, as its own kind, on the task.
async fn record(
    state: &State,
    conversation: uuid::Uuid,
    route: &str,
    usage: Option<&tqa_llm::Usage>,
) -> Result<(), GatewayError> {
    let count = |value: u64| i64::try_from(value).ok();
    let (prompt, completion) = usage.map_or((None, None), |usage| {
        (count(usage.prompt_tokens), count(usage.completion_tokens))
    });
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        sqlx::query!(
            "INSERT INTO usage (id,conversation_id,user_id,route,kind,prompt_tokens,completion_tokens) VALUES ($1,$2,(SELECT owner_id FROM conversations WHERE id=$2),$3,'title',$4,$5)",
            uuid::Uuid::now_v7(),
            conversation,
            route,
            prompt,
            completion
        )
        .execute(&state.pool)
        .await?;
        Ok(())
    })
    .await
}

/// The title's answer and the usage of the calls it took: asked once more with room to think when the answer ran out at
/// its cap after reasoning with no text yet (a model that thinks although the route declares no reasoning, as Ollama's
/// qwen3 does on its defaults), as the route check does (GW-U-095). A plain model keeps its one cheap call, and a cut
/// without reasoning, or with no room beyond the cap, is the answer as it came (GW-U-099).
pub(super) async fn answer(
    client: &reqwest::Client,
    idle: Duration,
    route: &Route,
    secret: Option<&SecretString>,
    asked: &mut CompletionRequest,
) -> Result<(Completion, Option<tqa_llm::Usage>), GatewayError> {
    let first = complete(client, idle, route, secret, asked).await?;
    let room = route
        .limits
        .max_output
        .map_or(THINKING_ROOM, |most| THINKING_ROOM.min(most));
    if first.finish_reason != Some(FinishReason::Length)
        || first.reasoning.trim().is_empty()
        || !first.text.trim().is_empty()
        || asked.max_output.is_some_and(|cap| cap >= room)
    {
        let usage = first.usage.clone();
        return Ok((first, usage));
    }
    asked.max_output = Some(room);
    let second = complete(client, idle, route, secret, asked).await?;
    let usage = match (first.usage, second.usage.clone()) {
        (Some(one), Some(two)) => Some(tqa_llm::Usage {
            prompt_tokens: one.prompt_tokens + two.prompt_tokens,
            completion_tokens: one.completion_tokens + two.completion_tokens,
            cached_tokens: None,
            reasoning_tokens: None,
        }),
        (one, two) => two.or(one),
    };
    Ok((second, usage))
}

/// One request the relay's way, collected.
async fn complete(
    client: &reqwest::Client,
    idle: Duration,
    route: &Route,
    secret: Option<&SecretString>,
    asked: &CompletionRequest,
) -> Result<Completion, GatewayError> {
    let failed = |_| GatewayError::Io;
    let encoded =
        tqa_llm::encode_request(&super::probe::runtime_view(route), asked).map_err(failed)?;
    let mut body: Value = serde_json::from_slice(&encoded).map_err(|_| GatewayError::Io)?;
    tqa_llm::relay_shaping::shape_request(route, &mut body).map_err(|_| GatewayError::Io)?;
    let operation = match route.protocol {
        Protocol::OpenAiResponses => Operation::Responses,
        Protocol::AnthropicMessages => Operation::Messages,
        _ => Operation::Chat,
    };
    let response = request::build(client, route, operation, secret)
        .map_err(|_| GatewayError::Io)?
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(serde_json::to_vec(&body).map_err(|_| GatewayError::Io)?)
        .timeout(DEADLINE)
        .send()
        .await
        .map_err(|_| GatewayError::Io)?;
    if !response.status().is_success() {
        return Err(GatewayError::Io);
    }
    let mut http = HttpConfig::new(StreamLimits::RECOMMENDED);
    http.idle_timeout = idle;
    let stream = tqa_llm::decode_response(
        route.protocol,
        response,
        http,
        Arc::new(tqa_llm::TokioSleeper),
        CancellationToken::new(),
    );
    tqa_llm::collect(stream, StreamLimits::RECOMMENDED)
        .await
        .map_err(|_| GatewayError::Io)
}

#[cfg(test)]
#[path = "title_tests.rs"]
mod tests;
