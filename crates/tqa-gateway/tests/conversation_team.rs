//! GW-I-205 (ADR-0034, the slice S6 contract): a task's `team` on its get and in the list, for its person and for an
//! administrator (SEC-005: counts and ids only); `scope=top` leaving members out, but not from a search; `auth/me`'s
//! `features.agent_teams`; and, while agent teams are off, no trace: `team` null, `scope=top` filtering nothing.
#[path = "support/fixture.rs"]
mod support;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult};
use uuid::Uuid;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const ON: &str = "[agent_teams]\nenabled=true\n";

/// A person's lead task with one member `researcher`, and a task of theirs in no team.
struct Team {
    id: Uuid,
    channel: Uuid,
    lead: String,
    member: String,
    other: String,
    cookie: String,
    admin: String,
}

async fn team(f: &Fixture) -> Result<Team> {
    let (_, admin) = f.admin().await?;
    let (person, cookie) = f.member("owner@example.invalid").await?;
    let lead = f.conversation(person.id).await?.id.to_string();
    let member = f.conversation(person.id).await?.id.to_string();
    let other = f.conversation(person.id).await?.id.to_string();
    sqlx::query("UPDATE conversations SET title = 'Report on Q3' WHERE id = $1::uuid")
        .bind(&lead)
        .execute(&f.pool)
        .await?;
    sqlx::query("UPDATE conversations SET title = 'Q3 by region' WHERE id = $1::uuid")
        .bind(&member)
        .execute(&f.pool)
        .await?;
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
        other,
        cookie,
        admin,
    })
}

async fn json(f: &Fixture, path: &str, cookie: &str) -> Result<Value> {
    let response = f.get(path, cookie).await?;
    assert_eq!(response.status().as_u16(), 200, "{path}");
    Ok(response.json().await?)
}

/// The listed tasks' ids and teams, by id.
async fn listed(f: &Fixture, query: &str, cookie: &str) -> Result<Vec<(String, Value)>> {
    let page = json(f, &format!("/api/v1/conversations{query}"), cookie).await?;
    let mut items: Vec<(String, Value)> = page["items"]
        .as_array()
        .ok_or("no items")?
        .iter()
        .map(|item| {
            (
                item["id"].as_str().unwrap_or_default().to_owned(),
                item["team"].clone(),
            )
        })
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(items)
}

// TEST: GW-I-205
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_205_a_tasks_team_for_its_person_and_the_top_level_list(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, ON, async |f| {
        let t = team(f).await?;
        let (id, lead) = (t.id.to_string(), t.lead.clone());
        let lead_team = json!({"role": "lead", "id": id, "state": "running", "helpers": 1,
                               "attention": {"stuck": 0, "failed": 0}, "waiting_for_you": false});
        let member_team = json!({"role": "member", "id": id, "state": "running",
                                 "lead": {"id": lead, "title": "Report on Q3"},
                                 "name": "researcher", "helper_state": "working", "can_stop": true});
        assert_eq!(json(f, &format!("/api/v1/conversations/{}", t.lead), &t.cookie).await?["team"], lead_team);
        assert_eq!(json(f, &format!("/api/v1/conversations/{}", t.member), &t.cookie).await?["team"], member_team);
        assert_eq!(json(f, &format!("/api/v1/conversations/{}", t.other), &t.cookie).await?["team"], Value::Null);
        // The whole list carries them; the top-level list leaves the member out; a search still finds it.
        let mut all = vec![
            (t.lead.clone(), lead_team.clone()),
            (t.member.clone(), member_team.clone()),
            (t.other.clone(), Value::Null),
        ];
        all.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(listed(f, "", &t.cookie).await?, all);
        let top: Vec<_> = all.iter().filter(|(task, _)| *task != t.member).cloned().collect();
        assert_eq!(listed(f, "?scope=top", &t.cookie).await?, top);
        assert_eq!(
            listed(f, "?scope=top&q=region", &t.cookie).await?,
            [(t.member.clone(), member_team)]
        );
        let bad = f.get("/api/v1/conversations?scope=everything", &t.cookie).await?;
        assert_eq!(bad.status().as_u16(), 400);
        // A question to the person is open until it is answered.
        sqlx::query(
            "INSERT INTO channel_messages (channel_id, seq, author_kind, author_id, author_name, type, to_kind, body) \
             VALUES ($1, 1, 'conversation', $2, 'lead', 'person_question', 'person', '{\"type\":\"person_question\",\"text\":\"Which Q3?\"}')",
        )
        .bind(t.channel)
        .bind(&t.lead)
        .execute(&f.pool)
        .await?;
        let waiting = json(f, &format!("/api/v1/conversations/{}", t.lead), &t.cookie).await?;
        assert_eq!(waiting["team"]["waiting_for_you"], json!(true));
        // The person's switch, read at start.
        assert_eq!(json(f, "/api/v1/auth/me", &t.cookie).await?["features"], json!({"agent_teams": true}));
        Ok(())
    })
    .await
}

// TEST: GW-I-205
// TEST: SEC-005
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_205_an_administrator_sees_counts_and_ids_never_names_or_titles(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, ON, async |f| {
        let t = team(f).await?;
        let teams: Vec<(String, Value)> = listed(f, "?all=true", &t.admin).await?;
        let id = t.id.to_string();
        for (task, team) in teams {
            if task == t.lead {
                assert_eq!(
                    team,
                    json!({"role": "lead", "id": id, "state": "running", "helpers": 1,
                                        "attention": {"stuck": 0, "failed": 0}})
                );
            } else if task == t.member {
                assert_eq!(
                    team,
                    json!({"role": "member", "id": id, "state": "running", "lead": {"id": t.lead}})
                );
            }
        }
        Ok(())
    })
    .await
}

// TEST: GW-I-205
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_205_while_agent_teams_are_off_there_is_no_trace(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let t = team(f).await?;
        assert_eq!(
            json(f, &format!("/api/v1/conversations/{}", t.member), &t.cookie).await?["team"],
            Value::Null
        );
        let top = listed(f, "?scope=top", &t.cookie).await?;
        assert_eq!(top.len(), 3, "nothing filtered: {top:?}");
        assert!(top.iter().all(|(_, team)| team.is_null()));
        assert_eq!(
            json(f, "/api/v1/auth/me", &t.cookie).await?["features"],
            json!({"agent_teams": false})
        );
        Ok(())
    })
    .await
}
