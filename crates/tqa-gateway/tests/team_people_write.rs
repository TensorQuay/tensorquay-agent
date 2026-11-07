//! GW-I-207 and GW-I-108's people's part (ADR-0034, the slice S6 contract): the person answers the lead's question
//! (stored as a `person_answer` to the lead, once; refused for a seq that isn't a question, a second time, once the
//! team stops) and stops the whole team from the lead's task or one helper from its own (through the one stop path, a
//! repeat answering the same); deleting a lead with helpers is refused without `with_helpers` and, with it, deletes
//! the helpers' tasks and stops the team (`lead_deleted`), with agent teams on or off; disabling the person stops
//! their teams (`person_disabled`). GW-I-210: what the person types to a helper reaches the lead as a
//! `person_message`, once.
#[path = "support/fixture.rs"]
mod support;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult};
use uuid::Uuid;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const ON: &str = "[agent_teams]\nenabled=true\n";
const OFF: &str = "[agent_teams]\nenabled=false\n";

struct Team {
    id: Uuid,
    channel: Uuid,
    lead: String,
    member: String,
    person: Uuid,
    cookie: String,
    admin: String,
}

async fn team(f: &Fixture) -> Result<Team> {
    let (_, admin) = f.admin().await?;
    let (person, cookie) = f.member("owner@example.invalid").await?;
    let lead = f.conversation(person.id).await?.id.to_string();
    let member = f.conversation(person.id).await?.id.to_string();
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
    .bind(&lead)
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
        person: person.id,
        cookie,
        admin,
    })
}

/// The lead's question to the person, stored as the store writes it; answers its seq.
async fn question(f: &Fixture, t: &Team) -> Result<i64> {
    let seq: i64 = sqlx::query_scalar(
        "UPDATE channels SET last_seq = last_seq + 1 WHERE id = $1 RETURNING last_seq",
    )
    .bind(t.channel)
    .fetch_one(&f.pool)
    .await?;
    sqlx::query(
        "INSERT INTO channel_messages (channel_id, seq, author_kind, author_id, author_name, type, to_kind, body) \
         VALUES ($1, $2, 'conversation', $3, 'lead', 'person_question', 'person', $4)",
    )
    .bind(t.channel)
    .bind(seq)
    .bind(&t.lead)
    .bind(json!({"type": "person_question", "text": "Which Q3?"}))
    .execute(&f.pool)
    .await?;
    Ok(seq)
}

async fn post(
    f: &Fixture,
    path: &str,
    cookie: &str,
    key: &str,
    body: &Value,
) -> Result<(u16, Value)> {
    let response = f
        .client
        .post(f.url(path))
        .header("origin", f.origin())
        .header("cookie", cookie)
        .header("idempotency-key", key)
        .json(body)
        .send()
        .await?;
    let status = response.status().as_u16();
    Ok((status, response.json().await.unwrap_or(Value::Null)))
}

fn code(answer: &Value) -> Value {
    answer["error"]["code"].clone()
}

async fn team_state(f: &Fixture, team: Uuid) -> Result<(String, Option<String>)> {
    Ok(
        sqlx::query_as("SELECT state, stop_reason FROM agent_teams WHERE id = $1")
            .bind(team)
            .fetch_one(&f.pool)
            .await?,
    )
}

// TEST: GW-I-207
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_207_the_person_answers_the_leads_question_once(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ON, async |f| {
        let t = team(f).await?;
        let asked = question(f, &t).await?;
        let path = format!("/api/v1/conversations/{}/team/answers", t.lead);
        let body = json!({"question_seq": asked, "text": "Calendar Q3, July to September."});
        let (status, answered) = post(f, &path, &t.cookie, "a1", &body).await?;
        assert_eq!(status, 201, "{answered}");
        let seq = answered["seq"].as_i64().ok_or("no seq")?;
        let stored: (String, String, Value) = sqlx::query_as(
            "SELECT type, to_kind, body FROM channel_messages WHERE channel_id = $1 AND seq = $2",
        )
        .bind(t.channel)
        .bind(seq)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(
            (stored.0.as_str(), stored.1.as_str()),
            ("person_answer", "lead")
        );
        assert_eq!(stored.2["question_seq"], json!(asked));
        // The same key answers as the first time; a second answer is refused.
        assert_eq!(
            post(f, &path, &t.cookie, "a1", &body).await?,
            (201, answered)
        );
        let (status, again) = post(f, &path, &t.cookie, "a2", &body).await?;
        assert_eq!((status, code(&again)), (409, json!("already_answered")));
        // A seq that isn't a question; a malformed body.
        let (status, unknown) = post(
            f,
            &path,
            &t.cookie,
            "a3",
            &json!({"question_seq": seq, "text": "x"}),
        )
        .await?;
        assert_eq!((status, code(&unknown)), (422, json!("unknown_reference")));
        for bad in [
            json!({"question_seq": asked, "text": ""}),
            json!({"question_seq": asked}),
            json!({"question_seq": asked, "text": "x", "to": "lead"}),
        ] {
            let (status, refused) = post(f, &path, &t.cookie, "a4", &bad).await?;
            assert_eq!(status, 400, "{bad}: {refused}");
        }
        // Once the team stops, no answer.
        let second = question(f, &t).await?;
        sqlx::query(
            "UPDATE agent_teams SET state = 'stopping', stop_reason = 'done' WHERE id = $1",
        )
        .bind(t.id)
        .execute(&f.pool)
        .await?;
        let (status, stopped) = post(
            f,
            &path,
            &t.cookie,
            "a5",
            &json!({"question_seq": second, "text": "Too late"}),
        )
        .await?;
        assert_eq!((status, code(&stopped)), (409, json!("team_stopped")));
        Ok(())
    })
    .await
}

// TEST: GW-I-207
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_207_the_person_stops_a_helper_then_the_team(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ON, async |f| {
        let t = team(f).await?;
        let helper = format!("/api/v1/conversations/{}/team/stop", t.member);
        let stopped = (202, json!({"member": {"name": "researcher", "state": "stopped"}}));
        assert_eq!(post(f, &helper, &t.cookie, "s1", &json!({})).await?, stopped);
        assert_eq!(post(f, &helper, &t.cookie, "s2", &json!({})).await?, stopped, "a repeat answers the same");
        let state: String = sqlx::query_scalar("SELECT state FROM agent_team_members WHERE conversation_id = $1::uuid")
            .bind(&t.member)
            .fetch_one(&f.pool)
            .await?;
        assert_eq!(state, "stopped");
        let notices: Vec<String> = sqlx::query_scalar(
            "SELECT body->>'text' FROM channel_messages WHERE channel_id = $1 AND body->>'kind' = 'member_stopped'",
        )
        .bind(t.channel)
        .fetch_all(&f.pool)
        .await?;
        assert_eq!(
            notices,
            ["researcher was stopped by its person. Its task stays. Don't start another helper for its part unless your person asks."],
            "the lead is told once that its person stopped it, and not to hand its part to a new helper"
        );
        // The whole team, from the lead's task: the one stop path carries it to ended.
        let lead = format!("/api/v1/conversations/{}/team/stop", t.lead);
        assert_eq!(post(f, &lead, &t.cookie, "s3", &json!({})).await?, (202, json!({"team": {"state": "stopping"}})));
        tokio::time::timeout(support::DEADLINE, async {
            while team_state(f, t.id).await?.0 != "ended" {
                tokio::task::yield_now().await;
            }
            Ok::<_, Box<dyn std::error::Error>>(())
        })
        .await??;
        assert_eq!(team_state(f, t.id).await?.1.as_deref(), Some("stopped"));
        let (status, ended) = post(f, &lead, &t.cookie, "s4", &json!({})).await?;
        assert_eq!((status, code(&ended)), (409, json!("team_stopped")));
        Ok(())
    })
    .await
}

// TEST: GW-I-108
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_108_deleting_a_lead_with_helpers_needs_with_helpers(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ON, deleting_a_lead_with_helpers).await
}

// TEST: GW-I-108
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_108_a_team_made_before_agent_teams_went_off_is_deleted_whole(
    pool: PgPool,
) -> TestResult {
    // The switch stops new team activity, not what a delete removes.
    support::run_configured(pool, None, OFF, deleting_a_lead_with_helpers).await
}

async fn deleting_a_lead_with_helpers(f: &Fixture) -> TestResult {
    let t = team(f).await?;
    let delete = |query: &str| {
        f.client
            .delete(f.url(&format!("/api/v1/conversations/{}{query}", t.lead)))
            .header("origin", f.origin())
            .header("cookie", &t.cookie)
            .send()
    };
    let refused = delete("").await?;
    assert_eq!(refused.status().as_u16(), 409);
    assert_eq!(code(&refused.json().await?), json!("has_helpers"));
    assert_eq!(
        team_state(f, t.id).await?.0,
        "running",
        "a refused delete stops nothing"
    );
    assert_eq!(delete("?with_helpers=true").await?.status().as_u16(), 204);
    let states: Vec<String> = sqlx::query_scalar(
        "SELECT state FROM conversations WHERE id = ANY($1::uuid[]) ORDER BY id",
    )
    .bind(vec![t.lead.clone(), t.member.clone()])
    .fetch_all(&f.pool)
    .await?;
    assert_eq!(states, ["deleted", "deleted"], "the lead and its helper");
    let (state, reason) = team_state(f, t.id).await?;
    assert_eq!(
        (state.as_str(), reason.as_deref()),
        ("ended", Some("lead_deleted"))
    );
    Ok(())
}

// TEST: GW-I-108
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_108_disabling_the_person_stops_their_teams(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ON, async |f| {
        let t = team(f).await?;
        let disabled = f
            .client
            .patch(f.url(&format!("/api/v1/users/{}", t.person)))
            .header("origin", f.origin())
            .header("cookie", &t.admin)
            .json(&json!({"disabled": true}))
            .send()
            .await?;
        assert_eq!(disabled.status().as_u16(), 200);
        let (state, reason) = team_state(f, t.id).await?;
        assert_eq!(
            (state.as_str(), reason.as_deref()),
            ("ended", Some("person_disabled"))
        );
        let notes: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM channel_messages WHERE channel_id = $1 AND type = 'ended'",
        )
        .bind(t.channel)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(notes, 1, "one end note to the lead");
        let helper: String = sqlx::query_scalar(
            "SELECT state FROM agent_team_members WHERE conversation_id = $1::uuid",
        )
        .bind(&t.member)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(helper, "stopped");
        Ok(())
    })
    .await
}

/// The `person_message`s to the lead in the team's log: their bodies.
async fn copies(f: &Fixture, t: &Team) -> Result<Vec<Value>> {
    Ok(sqlx::query_scalar(
        "SELECT body FROM channel_messages WHERE channel_id = $1 AND type = 'person_message' AND to_kind = 'lead' \
         AND author_kind = 'user' ORDER BY seq",
    )
    .bind(t.channel)
    .fetch_all(&f.pool)
    .await?)
}

// TEST: GW-I-210
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_210_what_the_person_types_to_a_helper_reaches_the_lead_once(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, ON, async |f| {
        let t = team(f).await?;
        let text = json!({"text": "Use the July figures, not June's.", "attachments": []});
        let to_helper = format!("/api/v1/conversations/{}/messages", t.member);
        let (status, sent) = post(f, &to_helper, &t.cookie, "m1", &text).await?;
        assert_eq!(status, 202, "{sent}");
        let expected = json!({
            "type": "person_message",
            "text": "Use the July figures, not June's.",
            "member": "researcher",
            "by": "your person",
        });
        assert_eq!(copies(f, &t).await?, std::slice::from_ref(&expected));
        // A replayed key copies nothing twice; the lead's own task is no helper's.
        assert_eq!(post(f, &to_helper, &t.cookie, "m1", &text).await?.0, 202);
        let to_lead = format!("/api/v1/conversations/{}/messages", t.lead);
        assert_eq!(post(f, &to_lead, &t.cookie, "m2", &text).await?.0, 202);
        assert_eq!(copies(f, &t).await?, [expected]);
        // Once the team stops, a helper's messages are its own.
        sqlx::query(
            "UPDATE agent_teams SET state = 'stopping', stop_reason = 'done' WHERE id = $1",
        )
        .bind(t.id)
        .execute(&f.pool)
        .await?;
        assert_eq!(post(f, &to_helper, &t.cookie, "m3", &text).await?.0, 202);
        assert_eq!(copies(f, &t).await?.len(), 1);
        Ok(())
    })
    .await
}

// TEST: GW-I-210
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_210_the_lead_is_woken_for_the_copy(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ON, async |f| {
        let t = team(f).await?;
        let text = json!({"text": "Use the July figures.", "attachments": []});
        let to_helper = format!("/api/v1/conversations/{}/messages", t.member);
        assert_eq!(post(f, &to_helper, &t.cookie, "m1", &text).await?.0, 202);
        assert_eq!(copies(f, &t).await?.len(), 1);
        // The lead has no active run, so the copy queues its wake at once. The reconciler wakes too, but only on its
        // next pass, 15 s apart: a wake within 3 s is the message route's own.
        let woken = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                let queued: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM idempotency WHERE conversation_id = $1::uuid AND key LIKE $2",
                )
                .bind(&t.lead)
                .bind(format!("team-wake:{}:%", t.lead))
                .fetch_one(&f.pool)
                .await?;
                if queued > 0 {
                    return Ok::<_, Box<dyn std::error::Error>>(queued);
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        })
        .await
        .map_err(|_| "the lead wasn't woken for the copy")??;
        assert_eq!(woken, 1);
        Ok(())
    })
    .await
}

// TEST: GW-I-210
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_210_with_the_channels_day_full_the_message_goes_and_the_copy_is_skipped(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, ON, async |f| {
        let t = team(f).await?;
        // The team's log at its daily limit (5,000 messages in 24 hours).
        sqlx::query(
            "INSERT INTO channel_messages (channel_id, seq, author_kind, author_id, author_name, type, to_kind, body) \
             SELECT $1, n, 'conversation', $2, 'lead', 'decision', 'members', \
                    jsonb_build_object('type', 'decision', 'text', 'Go on.') \
             FROM generate_series(1, 5000) AS n",
        )
        .bind(t.channel)
        .bind(&t.lead)
        .execute(&f.pool)
        .await?;
        sqlx::query("UPDATE channels SET last_seq = 5000 WHERE id = $1")
            .bind(t.channel)
            .execute(&f.pool)
            .await?;
        let text = json!({"text": "Use the July figures.", "attachments": []});
        let to_helper = format!("/api/v1/conversations/{}/messages", t.member);
        let (status, sent) = post(f, &to_helper, &t.cookie, "m1", &text).await?;
        assert_eq!(status, 202, "the person's message is never refused for the copy: {sent}");
        assert_eq!(copies(f, &t).await?.len(), 0, "the copy is skipped");
        Ok(())
    })
    .await
}
