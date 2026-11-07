//! GW-I-206 (ADR-0034, the slice S6 contract): an agent team for its person, by the lead's task or a member's: the team
//! (state, budget, the open question, each helper with its state, tokens and last message, the version), its messages
//! in pages, and `team.changed` on the lead's and the members' streams when the team changes, from migration 0079's
//! NOTIFY; another person and an administrator get `404`, a task in no team `404 no_team`.
#[path = "support/fixture.rs"]
mod support;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult};
use tqa_types::ServerFrame;
use uuid::Uuid;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const ON: &str = "[agent_teams]\nenabled=true\n";

struct Team {
    id: Uuid,
    channel: Uuid,
    lead: tqa_gateway::Conversation,
    member: String,
    other: String,
    cookie: String,
    admin: String,
}

async fn team(f: &Fixture) -> Result<Team> {
    let (_, admin) = f.admin().await?;
    let (person, cookie) = f.member("owner@example.invalid").await?;
    let lead = f.conversation(person.id).await?;
    let member = f.conversation(person.id).await?.id.to_string();
    let other = f.conversation(person.id).await?.id.to_string();
    let (id, channel) = (Uuid::now_v7(), Uuid::now_v7());
    sqlx::query("INSERT INTO channels (id, owner_id, name, rule, kind) VALUES ($1, $2, 'Agent team', 'human', 'agent_team')")
        .bind(channel)
        .bind(person.id)
        .execute(&f.pool)
        .await?;
    sqlx::query(
        "INSERT INTO agent_teams (id, channel_id, lead_conversation, owner_id, token_budget, deadline_at) \
         VALUES ($1, $2, $3::uuid, $4, 2000000, CURRENT_TIMESTAMP + interval '1 hour')",
    )
    .bind(id)
    .bind(channel)
    .bind(lead.id.as_str())
    .bind(person.id)
    .execute(&f.pool)
    .await?;
    sqlx::query(
        "INSERT INTO agent_team_members (conversation_id, team_id, name, role, workspace, state) \
         VALUES ($1::uuid, $2, 'researcher', 'researcher', 'own_copy', 'working')",
    )
    .bind(&member)
    .bind(id)
    .execute(&f.pool)
    .await?;
    Ok(Team {
        id,
        channel,
        lead,
        member,
        other,
        cookie,
        admin,
    })
}

/// Stores a team message as the gateway's store would (the team's log, with the channel's seq).
async fn message(
    f: &Fixture,
    t: &Team,
    (author_kind, author_id, author_name): (&str, &str, &str),
    (kind, to): (&str, &str),
    body: Value,
) -> Result<i64> {
    let seq: i64 = sqlx::query_scalar(
        "UPDATE channels SET last_seq = last_seq + 1 WHERE id = $1 RETURNING last_seq",
    )
    .bind(t.channel)
    .fetch_one(&f.pool)
    .await?;
    sqlx::query(
        "INSERT INTO channel_messages (channel_id, seq, author_kind, author_id, author_name, type, to_kind, body) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(t.channel)
    .bind(seq)
    .bind(author_kind)
    .bind(author_id)
    .bind(author_name)
    .bind(kind)
    .bind(to)
    .bind(body)
    .execute(&f.pool)
    .await?;
    Ok(seq)
}

async fn get(f: &Fixture, path: &str, cookie: &str) -> Result<(u16, Value)> {
    let response = f.get(path, cookie).await?;
    let status = response.status().as_u16();
    Ok((status, response.json().await.unwrap_or(Value::Null)))
}

// TEST: GW-I-206
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_206_the_team_and_its_messages_for_its_person(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ON, async |f| {
        let t = team(f).await?;
        let long = "North: 4.1 M. ".repeat(40);
        let report = message(
            f,
            &t,
            ("conversation", &t.member, "researcher"),
            ("report", "lead"),
            json!({"type": "report", "text": long, "files": []}),
        )
        .await?;
        let question = message(
            f,
            &t,
            ("conversation", t.lead.id.as_str(), "lead"),
            ("person_question", "person"),
            json!({"type": "person_question", "text": "Which Q3?"}),
        )
        .await?;
        for task in [t.lead.id.to_string(), t.member.clone()] {
            let (status, team) =
                get(f, &format!("/api/v1/conversations/{task}/team"), &t.cookie).await?;
            assert_eq!(status, 200, "{team}");
            assert_eq!(
                (
                    team["id"].clone(),
                    team["state"].clone(),
                    team["stop_reason"].clone(),
                    team["lead"].clone()
                ),
                (
                    json!(t.id),
                    json!("running"),
                    Value::Null,
                    // GW-I-319: its read place, and the report it hasn't read (the question is to the person).
                    json!({"id": t.lead.id.as_str(), "read_seq": 0, "unread": 1})
                )
            );
            assert_eq!(team["budget"], json!({"tokens": 2_000_000, "spent": 0}));
            assert_eq!(team["waiting_for_you"]["seq"], json!(question));
            assert_eq!(team["waiting_for_you"]["text"], json!("Which Q3?"));
            let helper = &team["helpers"][0];
            assert_eq!(
                (
                    helper["conversation"].clone(),
                    helper["name"].clone(),
                    helper["state"].clone(),
                    helper["position"].clone(),
                    helper["tokens"].clone(),
                    helper["activity"].clone()
                ),
                (
                    json!(t.member),
                    json!("researcher"),
                    json!("working"),
                    Value::Null,
                    json!(0),
                    Value::Null
                )
            );
            assert_eq!(helper["last_message"]["seq"], json!(report));
            assert_eq!(
                helper["last_message"]["text"]
                    .as_str()
                    .map(|text| text.chars().count()),
                Some(300),
                "cut to 300 characters"
            );
            assert!(
                team["version"]
                    .as_u64()
                    .is_some_and(|version| version > 1_700_000_000_000)
            );
        }
        // Messages in pages, every one of them, in the read shape.
        let (status, page) = get(
            f,
            &format!("/api/v1/conversations/{}/team/messages?limit=1", t.lead.id),
            &t.cookie,
        )
        .await?;
        assert_eq!(
            (
                status,
                page["items"][0]["seq"].clone(),
                page["last_seq"].clone(),
                page["more"].clone()
            ),
            (200, json!(report), json!(report), json!(true))
        );
        let (_, rest) = get(
            f,
            &format!(
                "/api/v1/conversations/{}/team/messages?after={report}",
                t.lead.id
            ),
            &t.cookie,
        )
        .await?;
        assert_eq!(
            (rest["items"][0]["seq"].clone(), rest["more"].clone()),
            (json!(question), json!(false))
        );
        for bad in ["?limit=0", "?limit=201", "?after=x", "?page=2"] {
            let (status, _) = get(
                f,
                &format!("/api/v1/conversations/{}/team/messages{bad}", t.lead.id),
                &t.cookie,
            )
            .await?;
            assert_eq!(status, 400, "{bad}");
        }
        // A task in no team; an administrator (SEC-005) and another person get the task's own answer, 403.
        let (status, answer) = get(
            f,
            &format!("/api/v1/conversations/{}/team", t.other),
            &t.cookie,
        )
        .await?;
        assert_eq!(
            (status, answer["error"]["code"].clone()),
            (404, json!("no_team"))
        );
        assert_eq!(
            get(
                f,
                &format!("/api/v1/conversations/{}/team", t.lead.id),
                &t.admin
            )
            .await?
            .0,
            403
        );
        assert_eq!(
            get(
                f,
                &format!("/api/v1/conversations/{}/team/messages", t.lead.id),
                &t.admin
            )
            .await?
            .0,
            403
        );
        let (_, stranger) = f.member("stranger@example.invalid").await?;
        assert_eq!(
            get(
                f,
                &format!("/api/v1/conversations/{}/team", t.member),
                &stranger
            )
            .await?
            .0,
            403
        );
        Ok(())
    })
    .await
}

/// The next `team.changed` on `stream`: its version and last seq.
async fn changed(stream: &mut support::stream::Stream) -> Result<(u64, u64)> {
    for _ in 0..64 {
        if let (
            None,
            ServerFrame::TeamChanged {
                version, last_seq, ..
            },
        ) = stream.next().await?
        {
            return Ok((version, last_seq));
        }
    }
    Err("no team.changed".into())
}

/// The first `team.changed` frame whose version is past `before`: a frame for an earlier change may still arrive.
async fn changed_past(stream: &mut support::stream::Stream, before: u64) -> Result<(u64, u64)> {
    for _ in 0..8 {
        let (version, last_seq) = changed(stream).await?;
        if version > before {
            return Ok((version, last_seq));
        }
    }
    Err("no team.changed past the version read".into())
}

// TEST: GW-I-206
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_206_a_change_reaches_the_streams_as_team_changed(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ON, async |f| {
        let t = team(f).await?;
        let mut stream = f.stream(&t.lead.id, &t.cookie, 0).await?;
        // A helper's state changes: the version rises.
        let (_, before) = get(
            f,
            &format!("/api/v1/conversations/{}/team", t.lead.id),
            &t.cookie,
        )
        .await?;
        // On purpose, any change of state (the reconciler's move to stuck is GW-I-111's): it reaches the streams.
        sqlx::query(
            "UPDATE agent_team_members SET state = 'stuck' WHERE conversation_id = $1::uuid",
        )
        .bind(&t.member)
        .execute(&f.pool)
        .await?;
        // The team's own creation is flushed within the second after it, so its frame can reach the stream after
        // the read above, carrying `before` (the 7 Nov integration, on a loaded host): the change's frame is the first
        // one past that version.
        let before = before["version"].as_u64().unwrap_or(u64::MAX);
        let (version, _) = changed_past(&mut stream, before).await?;
        assert!(version > before, "{version} after {before}");
        let (_, after) = get(
            f,
            &format!("/api/v1/conversations/{}/team", t.lead.id),
            &t.cookie,
        )
        .await?;
        assert_eq!(after["version"], json!(version));
        // A message: the last seq moves, the version doesn't.
        let seq = message(
            f,
            &t,
            ("conversation", &t.member, "researcher"),
            ("ask", "lead"),
            json!({"type": "ask", "text": "Which regions?"}),
        )
        .await?;
        loop {
            let (again, last_seq) = changed(&mut stream).await?;
            if last_seq == u64::try_from(seq)? {
                assert_eq!(again, version, "a message alone leaves the version");
                break;
            }
        }
        // A read cursor is no change people see: nothing is sent for it.
        sqlx::query(
            "UPDATE agent_team_members SET last_read_seq = 1 WHERE conversation_id = $1::uuid",
        )
        .bind(&t.member)
        .execute(&f.pool)
        .await?;
        let quiet =
            tokio::time::timeout(std::time::Duration::from_millis(2500), changed(&mut stream))
                .await;
        assert!(quiet.is_err(), "a read cursor sent team.changed: {quiet:?}");
        Ok(())
    })
    .await
}
