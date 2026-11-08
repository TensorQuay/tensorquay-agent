//! The lead's routes on one helper (ADR-0034, the S1 contract's routes table, slice 4): `stop_member`, which stops it
//! and tells the lead, and `read_helper`, a read-only page of its transcript. The lead only, its own helpers only;
//! behind the install switch; the calling run in `X-TQA-Run-ID` like every tool route.
use super::{
    roles::{self, Route},
    runtime::{check_run, run_header},
    store,
};
use crate::{GatewayError, app::State, conversations::idempotent::Keyed, error::InvalidReason};
use axum::{
    Json,
    body::Bytes,
    extract::{ConnectInfo, Path, RawQuery, State as AppState},
    http::{HeaderMap, StatusCode},
};
use serde_json::{Value, json};
use sqlx::PgConnection;
use std::{net::SocketAddr, sync::Arc};
use time::OffsetDateTime;
use tqa_types::{ConversationId, Event, MAX_TEAM_REQUEST_BYTES, ServerFrame};
use uuid::Uuid;

/// The longest reason `stop_member` takes, in bytes.
const MAX_REASON_BYTES: usize = 1024;
/// The most events a transcript page returns, and its default.
const MAX_EVENTS: u32 = 200;
const DEFAULT_EVENTS: u32 = 50;
/// How many stored frames one look at the helper's journal reads.
const FRAMES_PER_LOOK: u32 = 500;

/// `stop_member`'s request.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct StopRequest {
    reason: String,
}

/// `POST /runtime/teams/members/{name}/stop` (`stop_member`): the helper is stopped and the lead gets a
/// `member_stopped` notice, in one transaction; its active run is then cancelled through the runtime link as a person's
/// cancel is, and a wake queued for it is dropped. Stopping a stopped helper answers the same.
///
/// # Errors
/// `403 teams_disabled`, `not_a_member` or `not_lead`; `404 unknown_member`; `400` naming the field or header; `413`;
/// `409 team_stopped`, `team_ended`, `member_gone` (failed) or `not_current_run`; storage failure.
pub(crate) async fn stop(
    AppState(state): AppState<Arc<State>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(name): Path<String>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<(StatusCode, Json<Value>), GatewayError> {
    let lead = super::runtime::conversation(&state, &headers, peer).await?;
    super::runtime::enabled(&state)?;
    if bytes.len() > MAX_TEAM_REQUEST_BYTES {
        return Err(GatewayError::BodyTooLarge);
    }
    let key = crate::actions::idempotency_key(&headers)?.to_owned();
    let run = run_header(&headers)?;
    let request: StopRequest = super::runtime::decode(&bytes)?;
    reason(&request.reason)?;
    let result = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        let (owner, _) = crate::channels::store::conversation_author(&mut transaction, lead)
            .await?
            .ok_or(GatewayError::Unauthorized)?;
        check_run(&mut transaction, lead, &run).await?;
        let keyed = Keyed::for_conversation(
            owner,
            lead,
            &headers,
            format!("POST /runtime/teams/members/{name}/stop as {lead}"),
            &request,
        )?;
        if let Some((status, body)) = keyed.begin(&state, &mut transaction).await? {
            return Ok(((status, Json(body)), None));
        }
        let Some(caller) = store::caller(&mut transaction, lead).await? else {
            if store::ended(&mut transaction, lead).await?.is_some() {
                return Err(GatewayError::TeamEnded(None));
            }
            return Err(GatewayError::Denied("not_a_member"));
        };
        roles::allowed(Some(caller.role), Route::StopMember)?;
        let stopped =
            store::stop_member(&mut transaction, &caller, (&name, store::Stopper::Lead)).await?;
        let body = json!({ "member": { "name": stopped.name, "state": "stopped" } });
        keyed
            .record(&mut transaction, StatusCode::OK, &body)
            .await?;
        transaction.commit().await?;
        if let Some(seq) = stopped.notice {
            state.channels.published(caller.channel, seq);
        }
        Ok(((StatusCode::OK, Json(body)), Some(stopped.conversation)))
    })
    .await;
    let (answer, member) = match result {
        Err(GatewayError::TeamEnded(None)) => {
            let refused = Err(GatewayError::TeamEnded(None));
            return super::runtime::ended(&state, lead, (&key, &run), refused).await;
        }
        other => other?,
    };
    if let Some(member) = member {
        super::stop::cancel_member(&state, member).await?;
    }
    Ok(answer)
}

/// The reason's rules: no NUL, not empty, at most [`MAX_REASON_BYTES`].
fn reason(value: &str) -> Result<(), GatewayError> {
    let invalid = |reason| GatewayError::Invalid {
        field: "reason",
        reason,
    };
    if value.contains('\0') {
        return Err(invalid(InvalidReason::ValueNul));
    }
    if value.trim().is_empty() {
        return Err(invalid(InvalidReason::ValueEmpty));
    }
    if value.len() > MAX_REASON_BYTES {
        return Err(invalid(InvalidReason::ValueTooLong));
    }
    Ok(())
}

/// `GET /runtime/teams/members/{name}/transcript?since&limit` (`read_helper`): where the helper is now (`now`), then
/// its own messages, its text, tool calls and tool results after the event `since`, in order, at most `limit` (1–200,
/// 50 by default); the gateway's wake as `wake` with the helper's place, not as a message. Read-only; the tool marks
/// each event as untrusted. A stopped helper's transcript stays readable.
///
/// # Errors
/// `403 teams_disabled`, `not_a_member` or `not_lead`; `404 unknown_member`; `400` naming the header or parameter;
/// `409 team_ended` or `not_current_run`; storage failure.
pub(crate) async fn transcript(
    AppState(state): AppState<Arc<State>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(name): Path<String>,
    RawQuery(raw): RawQuery,
    headers: HeaderMap,
) -> Result<Json<Value>, GatewayError> {
    let lead = super::runtime::conversation(&state, &headers, peer).await?;
    super::runtime::enabled(&state)?;
    let run = run_header(&headers)?;
    let (since, limit) = page(raw.as_deref())?;
    let member = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        check_run(&mut connection, lead, &run).await?;
        let Some(caller) = store::caller(&mut connection, lead).await? else {
            if store::ended(&mut connection, lead).await?.is_some() {
                return Err(GatewayError::TeamEnded(None));
            }
            return Err(GatewayError::Denied("not_a_member"));
        };
        roles::allowed(Some(caller.role), Route::ReadHelper)?;
        let member = store::helper(&mut connection, caller.team, &name).await?;
        Ok((member, now(&mut connection, member).await?))
    })
    .await?;
    let (member, now) = member;
    let id: ConversationId = member
        .to_string()
        .parse()
        .map_err(|_| GatewayError::StoreUnavailable)?;
    let (events, last_seq, more) = events(&state, &id, since, limit).await?;
    Ok(Json(
        json!({ "now": now, "events": events, "last_seq": last_seq, "more": more }),
    ))
}

/// Where the helper is now, ahead of its events, so a lead can tell slow from stuck (night 2, 12 Oct: a lead read a
/// helper three times, saw only its brief, and stopped it 3.7 min into its first step's thinking). Its state (the
/// team view's word) and, while a step runs, the step, the tool running if any, since when and for how many seconds,
/// and about how many tokens of thinking and of text the step has streamed so far.
async fn now(connection: &mut PgConnection, member: Uuid) -> Result<Value, GatewayError> {
    let stored = sqlx::query_scalar!(
        "SELECT state FROM agent_team_members WHERE conversation_id = $1",
        member
    )
    .fetch_one(&mut *connection)
    .await?;
    let state = super::place_shape::helper_state(&stored);
    let Some(activity) = super::people_read::activity(connection, member).await? else {
        return Ok(json!({ "state": state }));
    };
    let streamed = sqlx::query!(
        "SELECT COALESCE(sum(octet_length(frame->'event'->>'text')) \
         FILTER (WHERE frame->'event'->>'type' = 'reasoning.delta'), 0)::bigint AS \"thinking!\", \
         COALESCE(sum(octet_length(frame->'event'->>'text')) \
         FILTER (WHERE frame->'event'->>'type' = 'text.delta'), 0)::bigint AS \"text!\", \
         COALESCE(bool_or(frame->'event'->>'type' = 'step.completed'), false) AS \"completed!\" \
         FROM events WHERE conversation_id = $1 AND seq > $2 AND run_id = $3 \
         AND frame->'event'->>'type' IN ('reasoning.delta', 'text.delta', 'step.completed')",
        member,
        activity.from_seq,
        activity.run
    )
    .fetch_one(&mut *connection)
    .await?;
    // Between steps: the last one has completed and the next hasn't started, so no step's time is running.
    if streamed.completed {
        return Ok(json!({ "state": state }));
    }
    let seconds = (OffsetDateTime::now_utc() - activity.began)
        .whole_seconds()
        .max(0);
    Ok(json!({
        "state": state,
        "step": activity.step,
        "tool": activity.tool,
        "since": activity.since,
        "seconds": seconds,
        "thinking_tokens": estimated_tokens(streamed.thinking),
        "text_tokens": estimated_tokens(streamed.text),
    }))
}

/// About how many tokens `bytes` of streamed UTF-8 are, by the estimate the runtime and the relay use for a step with no
/// reported usage (`tqa_context::estimate_bytes`).
fn estimated_tokens(bytes: i64) -> u32 {
    tqa_context::estimate_bytes(usize::try_from(bytes).unwrap_or(0)).tokens
}

/// `since` (a seq, 0 by default) and `limit` (1–200, 50 by default).
fn page(raw: Option<&str>) -> Result<(u64, u32), GatewayError> {
    let (mut since, mut limit) = (0_u64, DEFAULT_EVENTS);
    for (name, value) in url::form_urlencoded::parse(raw.unwrap_or_default().as_bytes()) {
        match name.as_ref() {
            "since" => {
                since = value.parse().map_err(|_| GatewayError::Invalid {
                    field: "since",
                    reason: InvalidReason::ParameterInvalid,
                })?;
            }
            "limit" => {
                limit = value
                    .parse::<u32>()
                    .ok()
                    .filter(|value| (1..=MAX_EVENTS).contains(value))
                    .ok_or(GatewayError::Invalid {
                        field: "limit",
                        reason: InvalidReason::LimitOutOfRange,
                    })?;
            }
            // Named as the people's message page names it.
            _ => {
                return Err(GatewayError::Invalid {
                    field: "query",
                    reason: InvalidReason::ParameterInvalid,
                });
            }
        }
    }
    Ok((since, limit))
}

fn seq_of(frame: &ServerFrame) -> u64 {
    crate::wire::sequence(frame)
        .and_then(|seq| serde_json::to_value(seq).ok())
        .and_then(|seq| seq.as_u64())
        .unwrap_or(0)
}

/// What one stored frame adds to a transcript page.
enum Step {
    /// Nothing: lifecycle, usage, output chunks and the like.
    Skip,
    /// More of the text being written.
    Text(String),
    /// An event of its own.
    Event(Value),
}

fn step(seq: u64, frame: ServerFrame) -> Step {
    let ServerFrame::Event { event, .. } = frame else {
        return Step::Skip;
    };
    match event {
        // The gateway's wake (revision 13) is its own kind, never a message to the helper.
        Event::MessageUser {
            team_wake: Some(wake),
            ..
        } => Step::Event(json!({"seq": seq, "type": "wake", "after": wake.after})),
        Event::MessageUser { text, .. } => {
            Step::Event(json!({"seq": seq, "type": "message", "text": text}))
        }
        Event::TextDelta { text } => Step::Text(text),
        Event::ToolCall {
            name, arguments, ..
        } => Step::Event(
            json!({"seq": seq, "type": "tool_call", "name": name, "arguments": arguments}),
        ),
        Event::ToolResult {
            status, content, ..
        } => Step::Event(
            json!({"seq": seq, "type": "tool_result", "status": status, "content": content}),
        ),
        _ => Step::Skip,
    }
}

/// The most of one event a page carries (its text, a tool's content or a call's arguments), and of a whole page, so one
/// large tool result can't fill the lead's model (GW-I-197).
const EVENT_BYTES: usize = 8 * 1024;
const PAGE_BYTES: usize = 64 * 1024;

/// A page being filled: its events and their size.
#[derive(Default)]
struct Page {
    events: Vec<Value>,
    bytes: usize,
}

/// `text` cut to at most `max` bytes at a character boundary.
fn cut(text: &str, max: usize) -> &str {
    let mut end = max.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Marks `event` as cut, with the full size of what was cut.
fn truncated(event: &mut Value, field: &str, full: &str) {
    event[field] = json!(cut(full, EVENT_BYTES));
    event["truncated"] = json!(true);
    event["bytes"] = json!(full.len());
}

/// `event` within [`EVENT_BYTES`]: an over-long text, content or argument list is cut and marked.
fn capped(mut event: Value) -> Value {
    for field in ["text", "content"] {
        if let Some(full) = event[field]
            .as_str()
            .filter(|full| full.len() > EVENT_BYTES)
            .map(str::to_owned)
        {
            truncated(&mut event, field, &full);
        }
    }
    let arguments = event["arguments"].to_string();
    if event.get("arguments").is_some() && arguments.len() > EVENT_BYTES {
        truncated(&mut event, "arguments", &arguments);
    }
    event
}

/// Adds what `frame` brings to `page`: more text to the text being written (up to [`EVENT_BYTES`]), or a new event while
/// the page has room for it by count and by size. Answers false, adding nothing, when a new event finds the page full.
fn take(page: &mut Page, seq: u64, frame: ServerFrame, limit: usize) -> bool {
    let next = match step(seq, frame) {
        Step::Skip => return true,
        Step::Event(event) => capped(event),
        Step::Text(text) => {
            if let Some(open) = page.events.last_mut().filter(|open| open["type"] == "text") {
                let held = open["text"].as_str().unwrap_or_default().to_owned();
                let size = |bytes: usize| u64::try_from(bytes).unwrap_or(u64::MAX);
                if held.len() >= EVENT_BYTES {
                    let full = open["bytes"].as_u64().unwrap_or_else(|| size(held.len()));
                    open["truncated"] = json!(true);
                    open["bytes"] = json!(full.saturating_add(size(text.len())));
                    return true;
                }
                let joined = format!("{held}{text}");
                let grown = joined.len().min(EVENT_BYTES).saturating_sub(held.len());
                *open = capped(json!({"seq": open["seq"].clone(), "type": "text", "text": joined}));
                page.bytes += grown;
                return true;
            }
            capped(json!({"seq": seq, "type": "text", "text": text}))
        }
    };
    let size = next.to_string().len();
    if page.events.len() == limit || (!page.events.is_empty() && page.bytes + size > PAGE_BYTES) {
        return false;
    }
    page.bytes += size;
    page.events.push(next);
    true
}

/// The helper's events after `since`, at most `limit`: its messages, its text (each run of deltas one event), its tool
/// calls and their results; the rest of its journal is left out. Answers the events, the seq of the last frame they
/// cover (the next page's `since`) and whether more may follow.
async fn events(
    state: &State,
    id: &ConversationId,
    since: u64,
    limit: u32,
) -> Result<(Vec<Value>, u64, bool), GatewayError> {
    let limit = usize::try_from(limit).unwrap_or(usize::MAX);
    let (mut page, mut last) = (Page::default(), since);
    loop {
        let frames = state.events.read(id, last.into(), FRAMES_PER_LOOK).await?;
        let count = frames.len();
        for frame in frames {
            let seq = seq_of(&frame);
            if !take(&mut page, seq, frame, limit) {
                return Ok((page.events, last, true));
            }
            last = seq;
        }
        if count < usize::try_from(FRAMES_PER_LOOK).unwrap_or(usize::MAX) {
            return Ok((page.events, last, false));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{EVENT_BYTES, PAGE_BYTES, Page, capped, cut, page, reason, seq_of, take};
    use serde_json::json;
    use tqa_types::{Event, ServerFrame};

    fn frame(seq: u64, event: Event) -> ServerFrame {
        ServerFrame::Event {
            seq: seq.into(),
            run_id: None,
            step: None,
            ts: "2026-01-01T00:00:00Z".into(),
            event,
        }
    }

    #[test]
    fn gw_u_134_a_transcript_event_and_page_are_capped_by_size() {
        // One event: cut at a character boundary, marked, with its full size.
        assert_eq!(cut("aé", 2), "a");
        let event = capped(
            json!({"seq": 4, "type": "tool_result", "status": "ok", "content": "x".repeat(1 << 20)}),
        );
        assert_eq!(event["content"].as_str().map(str::len), Some(EVENT_BYTES));
        assert_eq!(
            (&event["truncated"], &event["bytes"]),
            (&json!(true), &json!(1 << 20))
        );
        let small = capped(json!({"seq": 5, "type": "text", "text": "short"}));
        assert!(small.get("truncated").is_none());
        // A long run of deltas stops growing at the cap and counts what it left out.
        let mut page = Page::default();
        for seq in 1..=100 {
            assert!(take(
                &mut page,
                seq,
                frame(
                    seq,
                    Event::TextDelta {
                        text: "y".repeat(1000)
                    }
                ),
                50
            ));
        }
        assert_eq!(page.events.len(), 1);
        assert_eq!(
            page.events[0]["text"].as_str().map(str::len),
            Some(EVENT_BYTES)
        );
        assert_eq!(page.events[0]["bytes"], json!(100_000));
        // A page fills by size before its count: seven capped events of about 8 KiB, and the next waits for the next page.
        let mut page = Page::default();
        let mut taken = 0;
        for seq in 1..=20 {
            let call = Event::MessageUser {
                text: "z".repeat(20_000),
                attachments: Vec::new(),
                lead_brief: None,
                team_wake: None,
            };
            if !take(&mut page, seq, frame(seq, call), 50) {
                break;
            }
            taken += 1;
        }
        assert_eq!(taken, 7);
    }

    #[test]
    fn gw_u_134_an_event_that_fills_the_page_exactly_is_taken() {
        let message = |seq| {
            frame(
                seq,
                Event::MessageUser {
                    text: "m".into(),
                    attachments: Vec::new(),
                    lead_brief: None,
                    team_wake: None,
                },
            )
        };
        // The size the page counts for the event: what it adds to an empty page.
        let mut alone = Page::default();
        assert!(take(&mut alone, 2, message(2), 50));
        let size = alone.bytes;
        // After a first event, room for exactly that size takes it; a byte less leaves it for the next page.
        for (room, taken) in [(size, true), (size - 1, false)] {
            let mut page = Page::default();
            assert!(take(&mut page, 1, message(1), 50));
            page.bytes = PAGE_BYTES - room;
            assert_eq!(
                take(&mut page, 2, message(2), 50),
                taken,
                "{room} bytes of room for {size}"
            );
        }
    }

    // TEST: GW-U-145
    #[test]
    fn gw_u_145_a_helpers_wake_reads_as_the_gateways_in_its_transcript() {
        let mut page = Page::default();
        let wake = tqa_types::TeamWake { after: 6 };
        for (seq, team_wake, text) in [
            (
                1,
                Some(wake),
                "New messages from your lead since message 6.",
            ),
            (2, None, "a person's message"),
        ] {
            assert!(take(
                &mut page,
                seq,
                frame(
                    seq,
                    Event::MessageUser {
                        text: text.into(),
                        attachments: Vec::new(),
                        lead_brief: None,
                        team_wake,
                    }
                ),
                50
            ));
        }
        // The lead reads the gateway's wake as one, never as a message the helper was sent by a person.
        assert_eq!(
            page.events,
            [
                json!({"seq": 1, "type": "wake", "after": 6}),
                json!({"seq": 2, "type": "message", "text": "a person's message"}),
            ]
        );
    }

    #[test]
    fn gw_u_134_a_transcript_skips_what_isnt_work_caps_arguments_and_counts_a_run_cut_at_its_cap()
    -> Result<(), Box<dyn std::error::Error>> {
        // An event's seq is its own; a frame without one counts as 0.
        assert_eq!(seq_of(&frame(7, Event::TextDelta { text: "x".into() })), 7);
        assert_eq!(
            seq_of(&ServerFrame::Heartbeat {
                run_id: "r-1".parse()?
            }),
            0
        );
        // A frame that isn't an event (a heartbeat) adds nothing.
        let mut page = Page::default();
        assert!(take(
            &mut page,
            1,
            ServerFrame::Heartbeat {
                run_id: "r-1".parse()?
            },
            50
        ));
        assert!(page.events.is_empty());
        // A call's arguments past the cap are cut as text, marked, with their full size.
        let call = capped(json!({"seq": 2, "type": "tool_call", "name": "write_file",
                                 "arguments": {"content": "a".repeat(10_000)}}));
        assert_eq!(call["arguments"].as_str().map(str::len), Some(EVENT_BYTES));
        assert_eq!(call["truncated"], json!(true));
        assert!(call["bytes"].as_u64().is_some_and(|bytes| bytes > 10_000));
        // A run that reaches the cap exactly isn't cut yet; the next delta marks it and counts both.
        let mut page = Page::default();
        for seq in 1..=2 {
            assert!(take(
                &mut page,
                seq,
                frame(
                    seq,
                    Event::TextDelta {
                        text: "y".repeat(EVENT_BYTES / 2)
                    }
                ),
                50
            ));
        }
        assert!(page.events[0].get("truncated").is_none());
        assert!(take(
            &mut page,
            3,
            frame(3, Event::TextDelta { text: "zz".into() }),
            50
        ));
        assert_eq!(page.events[0]["truncated"], json!(true));
        assert_eq!(page.events[0]["bytes"], json!(EVENT_BYTES + 2));
        Ok(())
    }

    #[test]
    fn gw_u_134_a_calls_arguments_stay_whole_up_to_the_cap_exactly() {
        // Small arguments are left as they are, unmarked.
        let call = capped(json!({"seq": 3, "type": "tool_call", "name": "read_file",
                                 "arguments": {"path": "a.txt"}}));
        assert_eq!(call["arguments"], json!({"path": "a.txt"}));
        assert!(call.get("truncated").is_none(), "{call}");
        // Arguments whose JSON is exactly the cap long are kept whole; one byte more is cut and marked.
        let sized = |extra: usize| {
            let text = "a".repeat(EVENT_BYTES - r#"{"c":""}"#.len() + extra);
            capped(
                json!({"seq": 4, "type": "tool_call", "name": "write_file", "arguments": {"c": text}}),
            )
        };
        let exact = sized(0);
        assert_eq!(exact["arguments"].to_string().len(), EVENT_BYTES);
        assert!(exact.get("truncated").is_none());
        let over = sized(1);
        assert_eq!(over["truncated"], json!(true));
        assert_eq!(over["bytes"], json!(EVENT_BYTES + 1));
    }

    #[test]
    fn gw_u_134_stop_members_reason_and_read_helpers_page_are_bounded() {
        assert!(reason("Off track.").is_ok());
        for bad in ["", "   ", "a\0b"] {
            assert!(reason(bad).is_err(), "{bad:?}");
        }
        assert!(reason(&"x".repeat(1024)).is_ok());
        assert!(reason(&"x".repeat(1025)).is_err());
        assert_eq!(page(None).ok(), Some((0, 50)));
        assert_eq!(page(Some("since=7&limit=200")).ok(), Some((7, 200)));
        for raw in ["limit=0", "limit=201", "since=-1", "since=x", "wait=3"] {
            assert!(page(Some(raw)).is_err(), "{raw}");
        }
        assert!(
            matches!(
                page(Some("wait=3")),
                Err(crate::GatewayError::Invalid { field: "query", .. })
            ),
            "an unknown parameter is named, as on the people's message page"
        );
    }
}
