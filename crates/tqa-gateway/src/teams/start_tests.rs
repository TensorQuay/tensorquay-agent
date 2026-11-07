//! GW-I-106 (store): a member joins its lead's team. The team comes with the first member, with the file's budget and
//! deadline; names are made unique; the cap counts live members and names its limit; depth 1 holds for a member of an
//! ended team too; a stopping team takes no one; concurrent starts take turns under the lead's lock.
use super::{Joining, join};
use crate::{GatewayError, config::AgentTeams, teams::cap::Cap};
use sqlx::PgPool;
use tqa_types::WorkspaceMode;
use uuid::Uuid;

type TestResult = Result<(), Box<dyn std::error::Error>>;

async fn conversation(pool: &PgPool, owner: Uuid) -> Result<Uuid, sqlx::Error> {
    let workspace = Uuid::now_v7();
    sqlx::query("INSERT INTO workspaces (id, owner_id, name) VALUES ($1, $2, 'w')")
        .bind(workspace)
        .bind(owner)
        .execute(pool)
        .await?;
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO conversations (id, owner_id, title, model_route, backend, template, workspace_id) \
         VALUES ($1, $2, 'task', 'local', 'docker', 'workstation', $3)",
    )
    .bind(id)
    .bind(owner)
    .bind(workspace)
    .execute(pool)
    .await?;
    Ok(id)
}

async fn person(pool: &PgPool) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO users (id, email, password_hash, role) VALUES ($1, $2::text::citext, 'x', 'member')")
        .bind(id)
        .bind(format!("{id}@example.invalid"))
        .execute(pool)
        .await?;
    Ok(id)
}

const CAP: Cap = Cap {
    limit: 2,
    by: "max_members",
};

/// A new member conversation of `owner` joins `lead`'s team as `role`, in a transaction of its own.
async fn joined(
    pool: &PgPool,
    owner: Uuid,
    lead: Uuid,
    role: &str,
    cap: Cap,
) -> Result<Result<super::Joined, GatewayError>, Box<dyn std::error::Error>> {
    let member = conversation(pool, owner).await?;
    let mut transaction = pool.begin().await?;
    let outcome = join(
        &mut transaction,
        &Joining {
            lead,
            member,
            role,
            workspace: WorkspaceMode::OwnCopy,
        },
        cap,
        &AgentTeams::default(),
    )
    .await;
    if outcome.is_ok() {
        transaction.commit().await?;
    }
    Ok(outcome)
}

fn code<T>(outcome: &Result<T, GatewayError>) -> Option<&'static str> {
    outcome.as_ref().err().map(GatewayError::code)
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_106_the_team_comes_with_its_first_member_and_names_are_unique(
    pool: PgPool,
) -> TestResult {
    let owner = person(&pool).await?;
    let lead = conversation(&pool, owner).await?;
    let first = joined(&pool, owner, lead, "Researcher", CAP).await??;
    assert!(first.first);
    assert_eq!(first.name, "Researcher");
    let second = joined(&pool, owner, lead, "researcher", CAP).await??;
    assert_eq!(
        (second.first, second.team, second.channel),
        (false, first.team, first.channel)
    );
    assert_eq!(second.name, "researcher-2", "unique in any letter case");
    let (kind, budget, minutes): (String, i64, f64) = sqlx::query_as(
        "SELECT c.kind, t.token_budget, extract(epoch FROM t.deadline_at - t.created_at)::float8 / 60 \
         FROM agent_teams t JOIN channels c ON c.id = t.channel_id WHERE t.id = $1",
    )
    .bind(first.team)
    .fetch_one(&pool)
    .await?;
    assert_eq!((kind.as_str(), budget), ("agent_team", 2_000_000));
    assert!(
        (minutes - 120.0).abs() < 1.0,
        "the deadline is the file's 120 minutes: {minutes}"
    );
    Ok(())
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_106_the_cap_counts_live_members_and_names_its_limit(pool: PgPool) -> TestResult {
    let owner = person(&pool).await?;
    let lead = conversation(&pool, owner).await?;
    joined(&pool, owner, lead, "a", CAP).await??;
    let b = joined(&pool, owner, lead, "b", CAP).await??;
    let refused = joined(&pool, owner, lead, "c", CAP).await?;
    assert!(matches!(
        refused,
        Err(GatewayError::MemberLimit {
            limit: 2,
            by: "max_members"
        })
    ));
    let response = axum::response::IntoResponse::into_response(GatewayError::MemberLimit {
        limit: 2,
        by: "max_members",
    });
    assert_eq!(response.status(), 409);
    let body = axum::body::to_bytes(response.into_body(), 4096).await?;
    let body: serde_json::Value = serde_json::from_slice(&body)?;
    assert_eq!(body["error"]["code"], "member_limit");
    assert_eq!(
        body["error"]["details"],
        serde_json::json!({"limit": 2, "by": "max_members"})
    );
    // A stopped member frees its place; its name stays taken.
    sqlx::query(
        "UPDATE agent_team_members SET state = 'stopped' WHERE team_id = $1 AND name = 'b'",
    )
    .bind(b.team)
    .execute(&pool)
    .await?;
    assert_eq!(joined(&pool, owner, lead, "b", CAP).await??.name, "b-2");
    // No room at all: one slot leaves none for a member.
    let other = conversation(&pool, owner).await?;
    let none = Cap {
        limit: 0,
        by: "slots",
    };
    assert_eq!(
        code(&joined(&pool, owner, other, "a", none).await?),
        Some("member_limit")
    );
    Ok(())
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_037_a_member_never_leads_and_a_lead_leads_one_team(pool: PgPool) -> TestResult {
    let owner = person(&pool).await?;
    let lead = conversation(&pool, owner).await?;
    let member = conversation(&pool, owner).await?;
    let mut transaction = pool.begin().await?;
    let team = join(
        &mut transaction,
        &Joining {
            lead,
            member,
            role: "helper",
            workspace: WorkspaceMode::MyFiles,
        },
        CAP,
        &AgentTeams::default(),
    )
    .await?;
    transaction.commit().await?;
    let mut transaction = pool.begin().await?;
    let started = join(
        &mut transaction,
        &Joining {
            lead: member,
            member: conversation(&pool, owner).await?,
            role: "x",
            workspace: WorkspaceMode::OwnCopy,
        },
        CAP,
        &AgentTeams::default(),
    )
    .await;
    assert_eq!(code(&started), Some("not_lead"));
    drop(transaction);
    sqlx::query(
        "UPDATE agent_teams SET state = 'ended', ended_at = CURRENT_TIMESTAMP WHERE id = $1",
    )
    .bind(team.team)
    .execute(&pool)
    .await?;
    assert_eq!(
        code(&joined(&pool, owner, member, "x", CAP).await?),
        Some("not_lead")
    );
    // One team per lead: the ended team's lead doesn't lead again, and no second team is made (ADR-0034 point 10).
    assert_eq!(
        code(&joined(&pool, owner, lead, "helper", CAP).await?),
        Some("team_ended")
    );
    let leader = lead;
    let teams: i64 =
        sqlx::query_scalar("SELECT count(*) FROM agent_teams WHERE lead_conversation = $1")
            .bind(leader)
            .fetch_one(&pool)
            .await?;
    assert_eq!(teams, 1);
    Ok(())
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_106_a_stopping_team_takes_no_one(pool: PgPool) -> TestResult {
    let owner = person(&pool).await?;
    let lead = conversation(&pool, owner).await?;
    let first = joined(&pool, owner, lead, "a", CAP).await??;
    sqlx::query("UPDATE agent_teams SET state = 'stopping' WHERE id = $1")
        .bind(first.team)
        .execute(&pool)
        .await?;
    assert_eq!(
        code(&joined(&pool, owner, lead, "b", CAP).await?),
        Some("team_stopped")
    );
    Ok(())
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_196_concurrent_starts_take_turns_under_the_cap(pool: PgPool) -> TestResult {
    let owner = person(&pool).await?;
    let leader = conversation(&pool, owner).await?;
    let starts = (0..6).map(|_| joined(&pool, owner, leader, "helper", CAP));
    let outcomes = futures::future::join_all(starts).await;
    let mut names = Vec::new();
    let mut refused = 0;
    for outcome in outcomes {
        match outcome? {
            Ok(joined) => names.push(joined.name),
            Err(GatewayError::MemberLimit { .. }) => refused += 1,
            Err(other) => return Err(other.into()),
        }
    }
    names.sort();
    assert_eq!(
        names,
        ["helper", "helper-2"],
        "two places, two distinct names"
    );
    assert_eq!(refused, 4);
    let teams: i64 =
        sqlx::query_scalar("SELECT count(*) FROM agent_teams WHERE lead_conversation = $1")
            .bind(leader)
            .fetch_one(&pool)
            .await?;
    assert_eq!(teams, 1, "one team, however many first starts raced");
    Ok(())
}
