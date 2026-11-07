//! `/runtime/teams/messages` (ADR-0034, the slice S1 contract): a run posts to its agent team and reads what is
//! addressed to it, with its runtime token, from its sandbox's address. The role comes from the stored rows on every
//! call, and the author is always the run's conversation. Behind the install switch: off, every call answers
//! `teams_disabled`. A read may wait up to 30 s for a message, and answers early with a hint when another task needs
//! a slot or the lead's files.
use super::{
    roles::{self, Addressee, Route},
    store,
};
use crate::{GatewayError, app::State, conversations::idempotent::Keyed, error::InvalidReason};
use axum::{
    Json,
    body::Bytes,
    extract::{ConnectInfo, RawQuery, State as AppState},
    http::{HeaderMap, StatusCode},
};
use serde_json::{Value, json};
use std::{net::SocketAddr, sync::Arc};
use tqa_types::{MAX_TEAM_REQUEST_BYTES, TeamPost};
use uuid::Uuid;

/// The most messages a read returns, and its default.
const MAX_READ: u32 = 200;
const DEFAULT_READ: u32 = 50;
/// The longest wait a read may ask for, in seconds.
const MAX_WAIT_SECONDS: u32 = 30;

pub(crate) fn enabled(state: &State) -> Result<(), GatewayError> {
    if state.config.agent_teams.enabled {
        Ok(())
    } else {
        Err(GatewayError::Denied("teams_disabled"))
    }
}

/// The run's conversation, from its token and the sandbox's address.
pub(crate) async fn conversation(
    state: &State,
    headers: &HeaderMap,
    peer: SocketAddr,
) -> Result<Uuid, GatewayError> {
    let token = crate::auth::token::bearer(headers)?;
    let (conversation, _) = state
        .runtime_tokens
        .resolve(&token, peer.ip())
        .await?
        .ok_or(GatewayError::Unauthorized)?;
    Uuid::parse_str(conversation.as_str()).map_err(|_| GatewayError::Unauthorized)
}

/// The calling run, from `X-TQA-Run-ID`, which every route a tool calls takes (the S1 contract's Headers): a run id,
/// refused naming the header when missing or malformed. [`check_run`] then holds it to the conversation's current run.
pub(crate) fn run_header(headers: &HeaderMap) -> Result<String, GatewayError> {
    let invalid = |reason| GatewayError::Invalid {
        field: "X-TQA-Run-ID",
        reason,
    };
    let value = headers
        .get("x-tqa-run-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid(InvalidReason::ValueEmpty))?;
    value
        .parse::<tqa_types::RunId>()
        .map(|run| run.to_string())
        .map_err(|_| invalid(InvalidReason::ParameterInvalid))
}

/// `409 not_current_run` unless `run` is the conversation's current run (review H2), so whatever a call records for
/// its run is pruned when that run ends.
///
/// # Errors
/// `409 not_current_run`, storage failure.
pub(crate) async fn check_run(
    connection: &mut sqlx::PgConnection,
    conversation: Uuid,
    run: &str,
) -> Result<(), GatewayError> {
    if store::current_run(connection, conversation)
        .await?
        .as_deref()
        == Some(run)
    {
        Ok(())
    } else {
        Err(GatewayError::Conflict("not_current_run"))
    }
}

/// `team_ended` for a caller whose team has ended (ADR-0034 point 10). For the lead it carries what it hasn't read
/// yet, the end note among it, as a read's answer recorded under the call's key for its run: the read rule exactly, so
/// a repeat answers the same and the seq advances only when the call's `tool.result` is stored. A member's carries
/// nothing, as does the lead's once it has read everything. Any other result passes through.
pub(crate) async fn ended(
    state: &State,
    conversation: Uuid,
    (key, run): (&str, &str),
    result: Result<(StatusCode, Json<Value>), GatewayError>,
) -> Result<(StatusCode, Json<Value>), GatewayError> {
    if !matches!(result, Err(GatewayError::TeamEnded(None))) {
        return result;
    }
    let refusal = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        let Some(caller) = store::ended(&mut transaction, conversation).await? else {
            return Ok(GatewayError::Denied("not_a_member"));
        };
        if caller.role != roles::Role::Lead {
            return Ok(GatewayError::TeamEnded(None));
        }
        let answer = store::read(
            &mut transaction,
            &caller,
            (conversation, key, run),
            DEFAULT_READ,
        )
        .await?;
        // Nothing unread: the plain refusal, and nothing recorded (the transaction rolls back).
        if answer.messages.is_empty() {
            return Ok(GatewayError::TeamEnded(None));
        }
        // Indexed before the commit, as a read is (GW-I-195).
        if let Some(pending) = state.events.team_reads() {
            pending.insert(conversation, key, run);
        }
        transaction.commit().await?;
        sent(state, caller.lead, answer.sent).await;
        Ok(GatewayError::TeamEnded(Some(Box::new(json!({
            "messages": answer.messages,
            "last_seq": answer.last_seq,
            "more": answer.more,
        })))))
    })
    .await;
    Err(refusal.unwrap_or_else(|error| error))
}

/// The person's messages to the lead a read delivered leave its page's list of waiting messages (GW-I-307).
async fn sent(state: &State, lead: Uuid, sent: Vec<String>) {
    if sent.is_empty() {
        return;
    }
    let Ok(id) = lead.to_string().parse() else {
        return;
    };
    if let Err(error) =
        crate::actions::held::publish(&state.pool, &state.events, &id, Some(sent)).await
    {
        tracing::warn!(conversation_id = %id, code = error.code(), "held messages notice not delivered");
    }
}

/// A strictly decoded body; a refusal names the field serde names (review M3): an unknown field, a missing one, or
/// `type` for an unknown message type. Anything else is the body as a whole.
pub(crate) fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, GatewayError> {
    serde_json::from_slice(bytes).map_err(|error| decode_error(&error.to_string()))
}

fn decode_error(message: &str) -> GatewayError {
    let named = |prefix: &str| {
        message
            .split_once(prefix)
            .and_then(|(_, rest)| rest.split_once('`'))
            .map(|(field, _)| field.to_owned())
    };
    if let Some(field) = named("unknown field `") {
        GatewayError::InvalidField {
            field,
            reason: InvalidReason::ParameterInvalid,
        }
    } else if let Some(field) = named("missing field `") {
        GatewayError::InvalidField {
            field,
            reason: InvalidReason::ValueEmpty,
        }
    } else if message.contains("unknown variant `") {
        GatewayError::Invalid {
            field: "type",
            reason: InvalidReason::ParameterInvalid,
        }
    } else {
        GatewayError::Invalid {
            field: "body",
            reason: InvalidReason::BodyInvalid,
        }
    }
}

/// `POST /runtime/teams/messages`: the run posts as its conversation. The `Idempotency-Key` is required, so a retried
/// send answers with the first one's seq.
///
/// # Errors
/// `403 teams_disabled`, `not_a_member`, `not_lead` or `members_only`; `404 unknown_member`; `409 team_stopped`,
/// `team_ended`, `not_current_run` or `member_gone`; `413`; `400`/`422` naming the field; `429 channel_rate`; storage failure.
pub(crate) async fn post(
    AppState(state): AppState<Arc<State>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<(StatusCode, Json<Value>), GatewayError> {
    let conversation = conversation(&state, &headers, peer).await?;
    enabled(&state)?;
    if bytes.len() > MAX_TEAM_REQUEST_BYTES {
        return Err(GatewayError::BodyTooLarge);
    }
    let key = crate::actions::idempotency_key(&headers)?.to_owned();
    let run = run_header(&headers)?;
    let post: TeamPost = decode(&bytes)?;
    // A report's files are read from the member's workspace first, outside the transaction (slice 5).
    let collected = super::files::collect(&state, conversation, &post).await?;
    let result = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        let (owner, _) =
            crate::channels::store::conversation_author(&mut transaction, conversation)
                .await?
                .ok_or(GatewayError::Unauthorized)?;
        check_run(&mut transaction, conversation, &run).await?;
        // The key is scoped to the conversation, so another conversation of the same owner may send the same key.
        let keyed = Keyed::for_conversation(
            owner,
            conversation,
            &headers,
            format!("POST /runtime/teams/messages as {conversation}"),
            &post,
        )?;
        if let Some((status, body)) = keyed.begin(&state, &mut transaction).await? {
            return Ok((status, Json(body)));
        }
        let caller = store::caller(&mut transaction, conversation).await?;
        let Some(caller) = caller else {
            if store::ended(&mut transaction, conversation)
                .await?
                .is_some()
            {
                return Err(GatewayError::TeamEnded(None));
            }
            roles::allowed(None, Route::Post(post.message_type()))?;
            return Err(GatewayError::Denied("not_a_member"));
        };
        let seq = store::post(
            &mut transaction,
            &caller,
            conversation,
            (&post, collected.files()),
        )
        .await?;
        collected.keep(seq).await?;
        let mut body = json!({ "seq": seq });
        if roles::addressee(&post) == Addressee::Members {
            body["to"] = json!(store::live_members(&mut transaction, caller.team).await?);
        }
        keyed
            .record(&mut transaction, StatusCode::CREATED, &body)
            .await?;
        transaction.commit().await?;
        state.channels.published(caller.channel, seq);
        // Its readers whose runs aren't active are woken (the S1 contract's Delivery).
        super::wake::soon(&state, caller.team);
        Ok((StatusCode::CREATED, Json(body)))
    })
    .await;
    ended(&state, conversation, (&key, &run), result).await
}

/// `done`'s request: the summary, and what the acceptance check did when the lead's runtime ran one.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DoneRequest {
    summary: String,
    #[serde(default)]
    check: Option<tqa_types::CheckReport>,
}

/// What a retried `done` must repeat for its key: the summary and the check's command, not what the check printed,
/// which a rerun after a restart can change.
#[derive(serde::Serialize)]
struct DoneKey<'a> {
    summary: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    check: Option<&'a str>,
}

/// `POST /runtime/teams/done` (`done`, slice 3): the lead ends the team's work. When the administrator set an
/// acceptance check, the lead's runtime ran it first and `done` carries its report; a check that didn't pass refuses
/// `done` and nothing is stored (GW-I-198). Otherwise the summary is stored as a `done` message to every member and
/// the team set `stopping`; the stop carries it out and posts the end note (ADR-0034 point 10). The `Idempotency-Key` is required, so a retry answers as the first did.
///
/// # Errors
/// `403 teams_disabled`, `not_a_member` or `not_lead`; `400` naming the field or header; `413`; `409 team_stopped`,
/// `team_ended`, `not_current_run` or `acceptance_failed`; `429 channel_rate`; storage failure.
pub(crate) async fn done(
    AppState(state): AppState<Arc<State>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<(StatusCode, Json<Value>), GatewayError> {
    let conversation = conversation(&state, &headers, peer).await?;
    enabled(&state)?;
    if bytes.len() > MAX_TEAM_REQUEST_BYTES {
        return Err(GatewayError::BodyTooLarge);
    }
    let key = crate::actions::idempotency_key(&headers)?.to_owned();
    let run = run_header(&headers)?;
    let request: DoneRequest = decode(&bytes)?;
    super::validate::validate_done(&request.summary)?;
    let result = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        let (owner, _) =
            crate::channels::store::conversation_author(&mut transaction, conversation)
                .await?
                .ok_or(GatewayError::Unauthorized)?;
        check_run(&mut transaction, conversation, &run).await?;
        let keyed = Keyed::for_conversation(
            owner,
            conversation,
            &headers,
            format!("POST /runtime/teams/done as {conversation}"),
            &DoneKey {
                summary: &request.summary,
                check: request.check.as_ref().map(|check| check.command.as_str()),
            },
        )?;
        if let Some((status, body)) = keyed.begin(&state, &mut transaction).await? {
            return Ok((status, Json(body)));
        }
        let Some(caller) = store::caller(&mut transaction, conversation).await? else {
            if store::ended(&mut transaction, conversation)
                .await?
                .is_some()
            {
                return Err(GatewayError::TeamEnded(None));
            }
            return Err(GatewayError::Denied("not_a_member"));
        };
        let seq = store::done(&mut transaction, &caller, conversation, &request.summary).await?;
        // Held to the check set now, once `done` would end the team; a refusal rolls back what `done` wrote.
        if seq.is_some() {
            super::acceptance::check(&mut transaction, request.check.as_ref()).await?;
        }
        let body = json!({ "team": { "state": "stopping" } });
        keyed
            .record(&mut transaction, StatusCode::OK, &body)
            .await?;
        transaction.commit().await?;
        if let Some(seq) = seq {
            state.channels.published(caller.channel, seq);
        }
        // The stop is the reconciler's, now rather than at its next pass. It cancels the helpers' runs, not this one: the
        // lead's run goes on to its closing words to the person (GW-I-261).
        state.reconciler.wake();
        Ok((StatusCode::OK, Json(body)))
    })
    .await;
    ended(&state, conversation, (&key, &run), result).await
}

/// `POST /runtime/teams/members` (`start_member`, slice 2): the run's conversation starts a member and leads from
/// then on. In one transaction: the member task (the lead's settings, or a profile narrowed to them), its workspace (an
/// own copy, or the lead's), the team with its first member, the member cap, and the brief stored as an `assign` to
/// the member. An own copy is copied once that commits. The `Idempotency-Key` is required, so a retried start answers
/// as the first did.
///
/// # Errors
/// `403 teams_disabled` or `not_lead`; `400` naming the field; `413`; `422 unknown_reference` (`profile`); `409
/// profile_unusable`, `team_stopped` or `member_limit`; the fork's own codes; `429 channel_rate`; storage failure.
pub(crate) async fn start(
    AppState(state): AppState<Arc<State>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<(StatusCode, Json<Value>), GatewayError> {
    let leader = conversation(&state, &headers, peer).await?;
    enabled(&state)?;
    if bytes.len() > MAX_TEAM_REQUEST_BYTES {
        return Err(GatewayError::BodyTooLarge);
    }
    let key = crate::actions::idempotency_key(&headers)?.to_owned();
    let run = run_header(&headers)?;
    let request: tqa_types::StartMember = decode(&bytes)?;
    super::validate::validate_start(&request)?;
    let started = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        let (owner, _) = crate::channels::store::conversation_author(&mut transaction, leader)
            .await?
            .ok_or(GatewayError::Unauthorized)?;
        check_run(&mut transaction, leader, &run).await?;
        let keyed = Keyed::for_conversation(
            owner,
            leader,
            &headers,
            format!("POST /runtime/teams/members as {leader}"),
            &request,
        )?;
        if let Some((status, body)) = keyed.begin(&state, &mut transaction).await? {
            return Ok(((status, Json(body)), None));
        }
        let started =
            super::start::start_member(&mut transaction, &state, (leader, owner), &request).await?;
        let body = json!({ "member": started.member, "seq": started.seq, "lead_tools": true });
        keyed
            .record(&mut transaction, StatusCode::CREATED, &body)
            .await?;
        transaction.commit().await?;
        state.channels.published(started.channel, started.seq);
        Ok(((StatusCode::CREATED, Json(body)), started.fork))
    })
    .await;
    let (answer, fork) = match started {
        Err(GatewayError::TeamEnded(None)) => {
            let refused = Err(GatewayError::TeamEnded(None));
            return ended(&state, leader, (&key, &run), refused).await;
        }
        other => other?,
    };
    // A member on the lead's files gets its first turn now; an own copy's comes when the copy ends.
    if let Some(member) = answer.1.0["member"]["conversation"]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .filter(|_| fork.is_none())
        && let Err(error) = super::first_turn::send(&state, member).await
    {
        tracing::warn!(member = %member, code = error.code(), "a helper's first turn wasn't sent");
    }
    if let Some((source, copy, quota)) = fork {
        // A Docker copy's directory is made here, as the fork route makes it before its transaction.
        if !state.config.uses_block_images() {
            state.storage.workspace(copy).await?;
        }
        crate::workspaces::fork::copy(&state, (source, copy), quota).await?;
    }
    Ok(answer)
}

/// `GET /runtime/teams/role` (slice 3): which team tools the run gets, asked by the runtime at run start, as it asks
/// for its channels. `member` (its two tools), `lead` (the ten), `may_lead` (a task in no team: `start_member` alone),
/// `ended` (a lead whose team has ended and who hasn't read its end note: `read_reports` alone, which delivers it) or
/// `none` (agent teams are off, the conversation was ever a member, which never leads: depth 1, or it led a team that
/// has ended and has read its end: one team per lead). An older runtime never asks and gets no team tools. It takes
/// no `X-TQA-Run-ID`: the runtime asks before the run has an id.
///
/// # Errors
/// `401` without the run's token, storage failure.
pub(crate) async fn role(
    AppState(state): AppState<Arc<State>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Json<Value>, GatewayError> {
    let conversation = conversation(&state, &headers, peer).await?;
    if !state.config.agent_teams.enabled {
        return Ok(Json(json!({ "role": "none" })));
    }
    // A runtime below the wake's revision can't read a wake (revision 13): its task never becomes a lead, since a
    // lead's later wakes would reach it. The link refuses it once a team exists; this covers a task in none yet (GW-I-214).
    let id: tqa_types::ConversationId = conversation
        .to_string()
        .parse()
        .map_err(|_| GatewayError::Unauthorized)?;
    let wakeable = state
        .links
        .sandbox(&id)
        .is_some_and(|(_, revision)| revision >= tqa_types::TEAM_WAKE_REVISION);
    let role = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        Ok(match store::caller(&mut connection, conversation).await? {
            Some(caller) if caller.role == roles::Role::Lead => "lead",
            Some(_) => "member",
            // Depth 1: a member never leads.
            None if store::was_member(&mut connection, conversation).await? => "none",
            // One team per lead: once it has ended, `read_reports` alone while its end note is unread, then nothing.
            None => match store::ended(&mut connection, conversation).await? {
                Some(ended) if store::end_unread(&mut connection, ended.team).await? => "ended",
                None if wakeable => "may_lead",
                // An ended team whose note is read, or a runtime too old for a wake.
                Some(_) | None => "none",
            },
        })
    })
    .await?;
    Ok(Json(json!({ "role": role })))
}

/// `GET /runtime/teams/acceptance`: the administrator's acceptance check, which the lead's `done` runs first
/// (ADR-0034 point 10, GW-I-198), read fresh: `{"check": "<command>"}`, or `null` when none is set.
///
/// # Errors
/// `403 teams_disabled`, `not_a_member` or `not_lead`; storage failure.
pub(crate) async fn acceptance(
    AppState(state): AppState<Arc<State>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Json<Value>, GatewayError> {
    let conversation = conversation(&state, &headers, peer).await?;
    enabled(&state)?;
    let check = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        let caller = store::caller(&mut connection, conversation).await?;
        roles::allowed(caller.map(|caller| caller.role), Route::Done)?;
        crate::admin::agent_teams::acceptance_check(&mut connection).await
    })
    .await?;
    Ok(Json(json!({ "check": check })))
}

/// `GET /runtime/teams/messages?limit&wait`: what is addressed to the run's conversation after its last-read seq.
/// The read is recorded under its call's `Idempotency-Key` for the run named by `X-TQA-Run-ID`; a repeat answers the
/// same, and the seq advances only when that call's `tool.result` is stored.
///
/// # Errors
/// `403 teams_disabled` or `not_a_member`, `400` naming a header or parameter, `409 not_current_run` or
/// `team_ended`, storage failure.
pub(crate) async fn read(
    AppState(state): AppState<Arc<State>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    RawQuery(raw): RawQuery,
    headers: HeaderMap,
) -> Result<Json<Value>, GatewayError> {
    let conversation = conversation(&state, &headers, peer).await?;
    enabled(&state)?;
    let key = crate::actions::idempotency_key(&headers)?.to_owned();
    let run = run_header(&headers)?;
    let (limit, wait) = read_query(raw.as_deref())?;
    // The wait comes before the read records its answer, so a repeat of this call never replays an early empty one.
    let hint = if wait > 0 {
        wait_for(&state, (conversation, &key), wait).await?
    } else {
        None
    };
    let result = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        check_run(&mut transaction, conversation, &run).await?;
        let Some(caller) = store::caller(&mut transaction, conversation).await? else {
            if store::ended(&mut transaction, conversation)
                .await?
                .is_some()
            {
                return Err(GatewayError::TeamEnded(None));
            }
            return Err(GatewayError::Denied("not_a_member"));
        };
        roles::allowed(Some(caller.role), Route::Read)?;
        let answer =
            store::read(&mut transaction, &caller, (conversation, &key, &run), limit).await?;
        // Indexed before the commit, so before the answer can leave the gateway: the read's tool.result, which can
        // only follow the answer, always finds it and advances the seq. A commit that fails leaves a stale entry,
        // which costs one DELETE that finds nothing. Were a result ever to miss the index, that read would be answered
        // again (at least once), never lost.
        if let Some(pending) = state.events.team_reads() {
            pending.insert(conversation, &key, &run);
        }
        transaction.commit().await?;
        sent(&state, caller.lead, answer.sent).await;
        let mut body = json!({
            "messages": answer.messages,
            "last_seq": answer.last_seq,
            "more": answer.more,
        });
        if let Some(hint) = hint {
            body["hint"] = hint;
        }
        Ok((StatusCode::OK, Json(body)))
    })
    .await;
    ended(&state, conversation, (&key, &run), result)
        .await
        .map(|(_, body)| body)
}

/// How often a waiting read looks again for a hint (a task queued for a slot or for the lead's files), which no
/// channel message announces.
const HINT_EVERY: std::time::Duration = std::time::Duration::from_secs(1);

/// Waits up to `wait` seconds, before the read records its answer, for something to answer: the answer already
/// recorded under the call's key (a repeat), a new message for the reader (the channel's hub wakes it; it subscribes before it looks, so a post committed in between is never
/// missed), or a hint to end the turn (slice 3). Answers the hint, if one ended the wait.
async fn wait_for(
    state: &State,
    (conversation, key): (Uuid, &str),
    wait: u32,
) -> Result<Option<Value>, GatewayError> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(u64::from(wait));
    let caller = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        store::caller(&mut connection, conversation).await
    })
    .await?;
    // Outside a team the read refuses at once; nothing to wait for.
    let Some(caller) = caller else {
        return Ok(None);
    };
    let mut woken = state.channels.subscribe(caller.channel);
    loop {
        let (new, hint) = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
            let mut connection = state.pool.acquire().await?;
            let new = store::has_new(&mut connection, &caller, (conversation, key)).await?;
            let hint = if new {
                None
            } else {
                store::hint(&mut connection, &caller, conversation).await?
            };
            Ok((new, hint))
        })
        .await?;
        if new || hint.is_some() {
            return Ok(hint);
        }
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return Ok(None);
        }
        tokio::select! {
            () = state.cancel.cancelled() => return Err(GatewayError::Cancelled),
            _ = woken.changed() => {}
            () = tokio::time::sleep((deadline - now).min(HINT_EVERY)) => {}
        }
    }
}

/// `POST /runtime/teams/files` (`take_files`, slice 5): the lead places a report's files, all or those named, in its
/// own `uploads/` as `<member>-<path>`. The `Idempotency-Key` is required, so a retried call answers as the first did.
///
/// # Errors
/// `403 teams_disabled`, `not_a_member` or `not_lead`; `409 team_stopped` or `not_current_run`; `422
/// unknown_reference`; `413`; `400` naming the field (`X-TQA-Run-ID` included); per file `not_found`, `upload_too_large`, `uploads_full` or `files_unavailable` naming it;
/// storage failure.
pub(crate) async fn take_files(
    AppState(state): AppState<Arc<State>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<Json<Value>, GatewayError> {
    let lead = conversation(&state, &headers, peer).await?;
    enabled(&state)?;
    if bytes.len() > MAX_TEAM_REQUEST_BYTES {
        return Err(GatewayError::BodyTooLarge);
    }
    crate::actions::idempotency_key(&headers)?;
    let run = run_header(&headers)?;
    let request: tqa_types::TakeFiles = decode(&bytes)?;
    Ok(Json(
        super::files::take(&state, (lead, &run), &headers, &request).await?,
    ))
}

/// `limit` (1–200, 50 by default) and `wait` (0–30 seconds, 0 by default).
fn read_query(raw: Option<&str>) -> Result<(u32, u32), GatewayError> {
    let (mut limit, mut wait) = (DEFAULT_READ, 0);
    for (name, value) in url::form_urlencoded::parse(raw.unwrap_or_default().as_bytes()) {
        let (field, slot, max, min) = match name.as_ref() {
            "limit" => ("limit", &mut limit, MAX_READ, 1),
            "wait" => ("wait", &mut wait, MAX_WAIT_SECONDS, 0),
            _ => return Err(GatewayError::BadRequest("unknown query parameter")),
        };
        *slot = value
            .parse::<u32>()
            .ok()
            .filter(|value| (min..=max).contains(value))
            .ok_or(GatewayError::Invalid {
                field,
                reason: InvalidReason::LimitOutOfRange,
            })?;
    }
    Ok((limit, wait))
}

#[cfg(test)]
mod tests {
    use super::{decode_error, read_query};
    use crate::{GatewayError, error::InvalidReason};

    #[test]
    fn gw_u_125_a_decode_error_names_its_field() {
        let field = |message: &str| match decode_error(message) {
            GatewayError::InvalidField { field, reason } => Some((field, reason)),
            GatewayError::Invalid { field, reason } => Some((field.to_owned(), reason)),
            _ => None,
        };
        assert_eq!(
            field("unknown field `tone`, expected one of `type`, `text` at line 1 column 30"),
            Some(("tone".to_owned(), InvalidReason::ParameterInvalid))
        );
        assert_eq!(
            field("missing field `question_seq` at line 1 column 28"),
            Some(("question_seq".to_owned(), InvalidReason::ValueEmpty))
        );
        assert_eq!(
            field("unknown variant `notice`, expected one of `assign`"),
            Some(("type".to_owned(), InvalidReason::ParameterInvalid))
        );
        assert_eq!(
            field("expected value at line 1 column 1"),
            Some(("body".to_owned(), InvalidReason::BodyInvalid))
        );
    }

    #[test]
    fn gw_u_125_a_reads_limit_and_wait_are_bounded() {
        assert_eq!(read_query(None).ok(), Some((50, 0)));
        assert_eq!(read_query(Some("limit=200&wait=30")).ok(), Some((200, 30)));
        for raw in [
            "limit=0",
            "limit=201",
            "wait=31",
            "wait=-1",
            "limit=x",
            "since=3",
        ] {
            assert!(read_query(Some(raw)).is_err(), "{raw}");
        }
    }
}
