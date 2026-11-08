//! GW-I-319 (17.0's release candidate; a review of the team wake before 17.0): after a person stops a lead, the team's
//! messages to it wait unread and nothing said so. The person's team read now gives the lead's read place,
//! `lead.read_seq` (the place its wake reads from: what it has read, in a finished run or in one still going), and
//! `lead.unread`, the team's messages after that place that are for the lead (to it or to the whole team), never its
//! own.
#[path = "support/agent_team.rs"]
mod agent_team;
#[path = "support/fixture.rs"]
mod support;
use agent_team::*;
use serde_json::{Value, json};
use support::TestResult;

/// The lead's part of the team, as its person reads it.
async fn lead_of(f: &support::Fixture, t: &Team) -> Result<Value> {
    let response = f
        .get(
            &format!("/api/v1/conversations/{}/team", t.lead.0),
            &t.cookie,
        )
        .await?;
    assert_eq!(response.status().as_u16(), 200);
    let team: Value = response.json().await?;
    Ok(team["lead"].clone())
}

/// Stores a team message from `author` as the store would (the channel's next seq): for shapes no runtime post
/// makes, a helper's message to one helper or to the whole team.
async fn stored(
    f: &support::Fixture,
    t: &Team,
    author: &str,
    (to_kind, to): (&str, Option<&str>),
) -> Result<i64> {
    let channel: uuid::Uuid =
        sqlx::query_scalar("SELECT channel_id FROM agent_teams WHERE id = $1")
            .bind(t.id)
            .fetch_one(&f.pool)
            .await?;
    let seq: i64 = sqlx::query_scalar(
        "UPDATE channels SET last_seq = last_seq + 1 WHERE id = $1 RETURNING last_seq",
    )
    .bind(channel)
    .fetch_one(&f.pool)
    .await?;
    sqlx::query(
        "INSERT INTO channel_messages (channel_id, seq, author_kind, author_id, author_name, type, to_kind, \
         to_conversation, body) VALUES ($1, $2, 'conversation', $3, 'researcher', 'report', $4, $5::uuid, $6)",
    )
    .bind(channel)
    .bind(seq)
    .bind(author)
    .bind(to_kind)
    .bind(to)
    .bind(json!({"type": "report", "text": "Stored."}))
    .execute(&f.pool)
    .await?;
    Ok(seq)
}

// TEST: GW-I-319
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_319_the_team_read_gives_the_leads_read_place_and_what_waits_unread_for_it(
    pool: sqlx::PgPool,
) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        let lead = t.lead.0.as_str();
        assert_eq!(
            lead_of(f, &t).await?,
            json!({"id": lead, "read_seq": 0, "unread": 0}),
            "a new team: nothing read, nothing waiting"
        );
        // A helper's report and another's question to the lead wait for it.
        let (status, _) = send(
            f,
            &t.researcher.1,
            "r-2:c-1",
            &json!({"type": "report", "text": "North: 4.1 M."}),
        )
        .await?;
        assert_eq!(status, 201);
        let (status, _) = send(
            f,
            &t.reviewer.1,
            "r-3:c-1",
            &json!({"type": "ask", "text": "Which quarter?"}),
        )
        .await?;
        assert_eq!(status, 201);
        assert_eq!(lead_of(f, &t).await?["unread"], json!(2));
        // The lead's own message to one helper is no one's unread for it.
        let (status, _) = send(
            f,
            &t.lead.1,
            "r-1:c-1",
            &json!({"type": "assign", "to": "researcher", "text": "The south too."}),
        )
        .await?;
        assert_eq!(status, 201);
        assert_eq!(lead_of(f, &t).await?["unread"], json!(2));
        // A read in a run still going counts as read, as the wake counts it.
        let (status, answer) = read(f, &t.lead.1, ("r-1:c-2", "r-1")).await?;
        assert_eq!(status, 200, "{answer}");
        let last = seqs(&answer)
            .into_iter()
            .max()
            .ok_or("the lead read nothing")?;
        assert_eq!(
            lead_of(f, &t).await?,
            json!({"id": lead, "read_seq": last, "unread": 0})
        );
        // Its result stored, the read is the lead's place, and the run's end keeps it.
        f.gateway
            .events()
            .append(&t.lead.0, result(2, "r-1", "c-2")?)
            .await?;
        run_ends(f, &t.lead.0, 3, "r-1").await?;
        // A report after the lead's run (stopped by its person, say) waits unread, and wakes it.
        let (status, _) = send(
            f,
            &t.researcher.1,
            "r-2:c-2",
            &json!({"type": "report", "text": "South: 2.0 M."}),
        )
        .await?;
        assert_eq!(status, 201);
        assert_eq!(
            lead_of(f, &t).await?,
            json!({"id": lead, "read_seq": last, "unread": 1})
        );
        // Its wake reads from the same place.
        let keys = wakes_settle(f, &t.lead.0, 1).await?;
        assert!(
            keys.iter()
                .any(|key| key.starts_with(&format!("team-wake:{lead}:{last}"))),
            "the lead's wake reads from {last}: {keys:?}"
        );
        // A helper's message to another helper isn't the lead's; one to the whole team is.
        stored(
            f,
            &t,
            t.researcher.0.as_str(),
            ("member", Some(t.reviewer.0.as_str())),
        )
        .await?;
        assert_eq!(lead_of(f, &t).await?["unread"], json!(1));
        stored(f, &t, t.researcher.0.as_str(), ("team", None)).await?;
        assert_eq!(lead_of(f, &t).await?["unread"], json!(2));
        // A stopped team still says what was left unread.
        end(f, t.id).await?;
        assert_eq!(lead_of(f, &t).await?["unread"], json!(2));
        Ok(())
    })
    .await
}

/// The person's message to the lead task, while its run is active: held, and copied into the team's log (GW-I-307).
async fn to_lead(f: &support::Fixture, t: &Team, key: &str, text: &str) -> Result<u16> {
    let response = f
        .client
        .post(f.url(&format!("/api/v1/conversations/{}/messages", t.lead.0)))
        .header("origin", f.origin())
        .header("cookie", &t.cookie)
        .header("idempotency-key", key)
        .json(&json!({"text": text, "attachments": []}))
        .send()
        .await?;
    Ok(response.status().as_u16())
}

// TEST: GW-I-319
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_319_a_person_message_withdrawn_before_the_lead_read_it_is_not_unread(
    pool: sqlx::PgPool,
) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let t = team(f).await?;
        // Held behind the lead's active run, its copy waits for the lead's next check.
        assert_eq!(to_lead(f, &t, "p-1", "Drop DuckDB.").await?, 202);
        assert_eq!(lead_of(f, &t).await?["unread"], json!(1));
        // Withdrawn by its person, it will never reach the lead, as its wake counts it.
        let listed: Value = f
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
        let id = listed["items"][0]["id"]
            .as_str()
            .ok_or("the held message")?
            .to_owned();
        let cancel = f
            .client
            .delete(f.url(&format!(
                "/api/v1/conversations/{}/messages/queued/{id}",
                t.lead.0
            )))
            .header("origin", f.origin())
            .header("cookie", &t.cookie)
            .send()
            .await?;
        assert_eq!(cancel.status().as_u16(), 204);
        assert_eq!(lead_of(f, &t).await?["unread"], json!(0));
        // Another, still held, is unread.
        assert_eq!(to_lead(f, &t, "p-2", "Add MySQL.").await?, 202);
        assert_eq!(lead_of(f, &t).await?["unread"], json!(1));
        Ok(())
    })
    .await
}
