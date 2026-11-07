//! GW-I-307 (c16k real-world B5, 17 Oct 2026): a person couldn't steer a running team. A message on a leading lead's
//! page waited "until the agent finishes", which for a lead is after its whole team. Now, while the lead's run is
//! active, the message is kept as before (held, cancellable) and also copied into the team's log for the lead, who
//! reads it at its next check of the team; read, the message is delivered and never starts a run of its own; cancelled
//! first, or sent as a run first (the lead's run ended before it read), the copy is never read: once, never twice.
#[path = "support/agent_team.rs"]
mod agent_team;
#[path = "support/fixture.rs"]
mod support;
use agent_team::*;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult};

/// The person posts `text` to the lead's task with idempotency key `key`: the status and the body.
async fn to_lead(f: &Fixture, t: &Team, key: &str, text: &str) -> Result<(u16, Value)> {
    let response = f
        .client
        .post(f.url(&format!("/api/v1/conversations/{}/messages", t.lead.0)))
        .header("origin", f.origin())
        .header("cookie", &t.cookie)
        .header("idempotency-key", key)
        .json(&json!({"text": text, "attachments": []}))
        .send()
        .await?;
    let status = response.status().as_u16();
    Ok((status, response.json().await.unwrap_or(Value::Null)))
}

/// The lead task's queued messages, as its page lists them: (id, state) in order.
async fn queued(f: &Fixture, t: &Team) -> Result<Vec<(String, String)>> {
    let answer: Value = f
        .client
        .get(f.url(&format!(
            "/api/v1/conversations/{}/messages/queued",
            t.lead.0
        )))
        .header("cookie", &t.cookie)
        .send()
        .await?
        .json()
        .await?;
    Ok(answer["items"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            (
                item["id"].as_str().unwrap_or_default().to_owned(),
                item["state"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect())
}

/// The person's messages copied to the lead (`person_message` with no member) in the team's log: (seq, text).
async fn copies(f: &Fixture, t: &Team) -> Result<Vec<(i64, String)>> {
    Ok(sqlx::query_as(
        "SELECT m.seq, m.body->>'text' FROM channel_messages m JOIN agent_teams a ON a.channel_id = m.channel_id \
         WHERE a.id = $1 AND m.type = 'person_message' AND m.body->>'member' = '' ORDER BY m.seq",
    )
    .bind(t.id)
    .fetch_all(&f.pool)
    .await?)
}

/// The texts of the person's messages in a read answer.
fn person_texts(answer: &Value) -> Vec<String> {
    answer["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|message| message["type"] == "person_message")
        .filter_map(|message| message["text"].as_str().map(str::to_owned))
        .collect()
}

const ON: &str = "[agent_teams]\nenabled=true\n";

// TEST: GW-I-307
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_307_a_message_to_a_leading_lead_is_kept_and_copied_for_its_next_check(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, ON, async |f| {
        let t = team(f).await?;
        // The lead's run r-1 is active (the fixture starts it): the message is held behind it, as before.
        let (status, sent) = to_lead(f, &t, "p-1", "Add MySQL as a fourth database.").await?;
        assert_eq!(status, 202, "{sent}");
        let listed = queued(f, &t).await?;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].1, "held", "still cancellable");
        let copied = copies(f, &t).await?;
        assert_eq!(copied.len(), 1, "copied once for the lead");
        assert_eq!(copied[0].1, "Add MySQL as a fourth database.");
        let link: Option<i64> = sqlx::query_scalar(
            "SELECT team_copy_seq FROM idempotency WHERE conversation_id = $1::uuid AND execution_status = 'queued'",
        )
        .bind(t.lead.0.as_str())
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(link, Some(copied[0].0), "the message knows its copy");
        // A replayed key copies nothing twice.
        assert_eq!(to_lead(f, &t, "p-1", "Add MySQL as a fourth database.").await?.0, 202);
        assert_eq!(copies(f, &t).await?.len(), 1);
        Ok(())
    })
    .await
}

// TEST: GW-I-307
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_307_read_by_the_lead_it_is_delivered_and_never_runs(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ON, async |f| {
        let t = team(f).await?;
        assert_eq!(to_lead(f, &t, "p-1", "Add MySQL.").await?.0, 202);
        let (status, answer) = read(f, &t.lead.1, ("r-1:c-1", "r-1")).await?;
        assert_eq!(status, 200, "{answer}");
        assert_eq!(person_texts(&answer), ["Add MySQL."], "the lead reads it at its next check");
        let listed = queued(f, &t).await?;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].1, "sent", "delivered: it starts no run of its own");
        let still_queued: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM idempotency WHERE conversation_id = $1::uuid AND execution_status = 'queued'",
        )
        .bind(t.lead.0.as_str())
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(still_queued, 0);
        // Read, it can't be cancelled any more.
        let cancel = f
            .client
            .delete(f.url(&format!(
                "/api/v1/conversations/{}/messages/queued/{}",
                t.lead.0, listed[0].0
            )))
            .header("origin", f.origin())
            .header("cookie", &t.cookie)
            .send()
            .await?;
        assert_eq!(cancel.status().as_u16(), 409);
        // Never twice: a second read doesn't bring it back.
        let (_, again) = read(f, &t.lead.1, ("r-1:c-2", "r-1")).await?;
        assert!(person_texts(&again).is_empty(), "{again}");
        Ok(())
    })
    .await
}

// TEST: GW-I-307
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_307_cancelled_or_sent_as_a_run_first_its_copy_is_never_read(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, ON, async |f| {
        let t = team(f).await?;
        // Cancelled before the lead read it.
        assert_eq!(to_lead(f, &t, "p-1", "Drop DuckDB.").await?.0, 202);
        let listed = queued(f, &t).await?;
        let cancel = f
            .client
            .delete(f.url(&format!(
                "/api/v1/conversations/{}/messages/queued/{}",
                t.lead.0, listed[0].0
            )))
            .header("origin", f.origin())
            .header("cookie", &t.cookie)
            .send()
            .await?;
        assert_eq!(cancel.status().as_u16(), 204);
        // Sent as a run first: the lead's run ended before it read, and the message went out as its next turn.
        assert_eq!(to_lead(f, &t, "p-2", "Add MySQL.").await?.0, 202);
        sqlx::query(
            "UPDATE idempotency SET execution_status = 'sending' WHERE conversation_id = $1::uuid \
             AND execution_status = 'queued'",
        )
        .bind(t.lead.0.as_str())
        .execute(&f.pool)
        .await?;
        assert_eq!(copies(f, &t).await?.len(), 2, "both were copied");
        let (status, answer) = read(f, &t.lead.1, ("r-1:c-1", "r-1")).await?;
        assert_eq!(status, 200, "{answer}");
        assert!(
            person_texts(&answer).is_empty(),
            "neither copy is read: {answer}"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-307
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_307_no_copy_without_an_active_lead_run(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ON, async |f| {
        let t = team(f).await?;
        run_ends(f, &t.lead.0, 2, "r-1").await?;
        assert_eq!(to_lead(f, &t, "p-1", "Add MySQL.").await?.0, 202);
        assert!(
            copies(f, &t).await?.is_empty(),
            "no run to wait on: an ordinary turn"
        );
        Ok(())
    })
    .await
}
