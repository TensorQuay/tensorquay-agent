//! Agent teams from a run, slice 2 (ADR-0034, the slice S1 contract): `POST /runtime/teams/members`.
//! - GW-I-106: the agent team comes with the first member; the member is a normal task of the same person with the
//!   lead's settings (the helpers' route when the file sets one), or a profile's narrowed to the lead's, the left-out
//!   ones listed; `own_copy` forks the lead's workspace and `my_files` binds it; the brief is stored as an `assign` to
//!   the member; the member cap; a retried start answers the first; agent teams are off unless switched on.
//! - SEC-037: a member's `start_member` is refused (depth 1); past the cap nothing is stored.
#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};
use tqa_types::ConversationId;
use uuid::Uuid;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const EXTRA: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";

/// Agent teams on, the docs server and the tracker connector on by default, the wiki connector off, and a second
/// environment.
fn config(teams: &str) -> String {
    format!(
        "[agent_teams]\nenabled = true\n{teams}\n\
         [[mcp.servers]]\nname='docs'\ncommand='/usr/local/bin/docs-mcp'\nenabled_by_default=true\n\
         [[connectors]]\nname='tracker'\nurl='https://mcp.tracker.example/mcp'\nenabled_by_default=true\n\
         [[connectors]]\nname='wiki'\nurl='https://mcp.wiki.example/mcp'\n\
         [[templates]]\nname='workstation'\ndisplay_name='W'\nimage='{EXTRA}'\ncapabilities=['shell']\ndefault=true\n\
         [[templates]]\nname='extra'\ndisplay_name='Extra'\nimage='{EXTRA}'\ncapabilities=['shell']\n"
    )
}

fn registry() -> Result<tqa_llm::Registry> {
    let route = |name: &str| {
        format!(
            "[[routes]]\nname='{name}'\nprotocol='openai_chat'\nbase_url='https://models.example/v1'\nmodel='{name}-model'\nauth={{kind='none'}}\n"
        )
    };
    Ok(tqa_llm::Registry::from_toml(&format!(
        "{}{}",
        route("local"),
        route("spare")
    ))?)
}

/// A person's task with its runtime connected, and the runtime's token.
struct Task {
    id: Uuid,
    token: String,
    _runtime: support::runtime::Runtime,
}

async fn task(f: &Fixture, owner: Uuid) -> Result<Task> {
    let conversation = f.conversation(owner).await?;
    connected(f, &conversation.id).await
}

async fn connected(f: &Fixture, id: &ConversationId) -> Result<Task> {
    let runtime = f.runtime_for(id).await?;
    // Its current run, `r-1`, which every team route but the role checks (the S1 contract's Headers).
    let next: i64 = sqlx::query_scalar(
        "SELECT COALESCE(max(seq), 0) + 1 FROM events WHERE conversation_id = $1",
    )
    .bind(Uuid::parse_str(id.as_str())?)
    .fetch_one(&f.pool)
    .await?;
    f.gateway
        .events()
        .append(id, support::started(u64::try_from(next)?, "r-1")?)
        .await?;
    let token = f
        .backend
        .token(id)
        .ok_or("token")?
        .expose_secret()
        .to_owned();
    Ok(Task {
        id: Uuid::parse_str(id.as_str())?,
        token,
        _runtime: runtime,
    })
}

fn brief(role: &str) -> Value {
    json!({
        "role": role,
        "brief": {"objective": "Find the Q3 revenue by region.", "output": "A table.",
                  "boundaries": "Don't edit the reports."},
    })
}

async fn start(f: &Fixture, token: &str, key: &str, body: &Value) -> Result<(u16, Value)> {
    let response = f
        .client
        .post(f.url("/runtime/teams/members"))
        .bearer_auth(token)
        .header("idempotency-key", key)
        .header("x-tqa-run-id", "r-1")
        .json(body)
        .send()
        .await?;
    let status = response.status().as_u16();
    Ok((status, response.json().await?))
}

async fn profile(f: &Fixture, cookie: &str, body: Value) -> Result<()> {
    let (status, created) = call(
        f,
        Method::POST,
        "/api/v1/profiles",
        cookie,
        (Some(body), None),
    )
    .await?;
    assert_eq!(status, 201, "{created}");
    Ok(())
}

fn uuid(value: &Value) -> Result<Uuid> {
    Ok(Uuid::parse_str(value.as_str().ok_or("not a string")?)?)
}

async fn workspace_of(f: &Fixture, conversation: Uuid) -> Result<Uuid> {
    Ok(
        sqlx::query_scalar("SELECT workspace_id FROM conversations WHERE id = $1")
            .bind(conversation)
            .fetch_one(&f.pool)
            .await?,
    )
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_106_the_first_start_makes_the_team_and_a_member_task(pool: PgPool) -> TestResult {
    Box::pin(support::run_configured(pool, Some(registry()?), &config(""), async |f| {
        f.admin().await?;
        let (person, _) = f.member("owner@example.invalid").await?;
        let lead = task(f, person.id).await?;
        let lead_workspace = workspace_of(f, lead.id).await?;
        let (status, started) = start(f, &lead.token, "c-1", &brief("researcher")).await?;
        assert_eq!(status, 201, "{started}");
        let member = &started["member"];
        assert_eq!(
            (&member["name"], &member["workspace"], &member["state"], &started["lead_tools"]),
            (&json!("researcher"), &json!("own_copy"), &json!("starting"), &json!(true))
        );
        // The lead runs while it starts the member, so its copy is marked inconsistent.
        assert_eq!(member["consistent"], json!(false));
        assert!(member.get("left_out").is_none() && member.get("profile").is_none());
        let conversation = uuid(&member["conversation"])?;
        let workspace = uuid(&member["workspace_id"])?;
        assert_ne!(conversation, lead.id);
        // A normal task of the same person, with the lead's settings and its name as title.
        let (owner, title, route, template, servers, connectors): (Uuid, String, String, String, Vec<String>, Vec<String>) =
            sqlx::query_as(
                "SELECT owner_id, title, model_route, template, mcp_servers, connectors FROM conversations WHERE id = $1",
            )
            .bind(conversation)
            .fetch_one(&f.pool)
            .await?;
        assert_eq!(
            (owner, title.as_str(), route.as_str(), template.as_str()),
            (person.id, "researcher", "local", "workstation")
        );
        assert_eq!((servers, connectors), (vec!["docs".to_owned()], vec!["tracker".to_owned()]));
        // Its own copy: a fork of the lead's workspace, taken while the lead may be running.
        let (forked_from, owner_of_copy): (Option<Uuid>, Uuid) =
            sqlx::query_as("SELECT forked_from, owner_id FROM workspaces WHERE id = $1")
                .bind(workspace)
                .fetch_one(&f.pool)
                .await?;
        assert_eq!((workspace_of(f, conversation).await?, forked_from, owner_of_copy), (workspace, Some(lead_workspace), person.id));
        // The brief is the team's message at `seq`: an assign to the new member with the brief's parts.
        let seq = started["seq"].as_i64().ok_or("seq")?;
        let (kind, to_kind, to, body): (String, String, Uuid, Value) = sqlx::query_as(
            "SELECT m.type, m.to_kind, m.to_conversation, m.body FROM channel_messages m \
             JOIN agent_teams t ON t.channel_id = m.channel_id WHERE t.lead_conversation = $1 AND m.seq = $2",
        )
        .bind(lead.id)
        .bind(seq)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!((kind.as_str(), to_kind.as_str(), to), ("assign", "member", conversation));
        assert_eq!(body["brief"]["output"], "A table.");
        // My files: the lead's own workspace.
        let mine = json!({"role": "reviewer", "workspace": "my_files", "brief": brief("x")["brief"]});
        let (status, second) = start(f, &lead.token, "c-2", &mine).await?;
        assert_eq!(status, 201, "{second}");
        assert_eq!(uuid(&second["member"]["workspace_id"])?, lead_workspace);
        // It waits for the lead's sandbox to hand over the files, and the answer says so.
        assert_eq!(second["member"]["state"], "queued");
        assert!(second["member"].get("consistent").is_none(), "no copy, nothing to call consistent");
        assert!(second["seq"].as_i64() > Some(seq));
        Ok(())
    }))
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_106_a_member_without_a_profile_takes_the_helpers_route(pool: PgPool) -> TestResult {
    Box::pin(support::run_configured(
        pool,
        Some(registry()?),
        &config("member_route = 'spare'"),
        async |f| {
            f.admin().await?;
            let (person, _) = f.member("owner@example.invalid").await?;
            let lead = task(f, person.id).await?;
            let (status, started) = start(f, &lead.token, "c-1", &brief("helper")).await?;
            assert_eq!(status, 201, "{started}");
            let route: String =
                sqlx::query_scalar("SELECT model_route FROM conversations WHERE id = $1")
                    .bind(uuid(&started["member"]["conversation"])?)
                    .fetch_one(&f.pool)
                    .await?;
            assert_eq!(route, "spare");
            Ok(())
        },
    ))
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_106_a_profile_narrows_to_the_lead_and_lists_what_it_left_out(
    pool: PgPool,
) -> TestResult {
    Box::pin(support::run_configured(pool, Some(registry()?), &config(""), async |f| {
        let (_, admin) = f.admin().await?;
        let (person, cookie) = f.member("owner@example.invalid").await?;
        let (_, other) = f.member("other@example.invalid").await?;
        // The person's own: another route and environment, the docs server, and the wiki connector the lead lacks.
        profile(f, &cookie, json!({
            "name": "legal-reviewer", "display_name": "Legal reviewer", "model_route": "spare", "template": "extra",
            "mcp_servers": ["docs"], "connectors": ["wiki"], "instructions": "Review only.",
            "tools": ["read_file", "grep"],
        }))
        .await?;
        profile(f, &other, json!({"name": "theirs", "display_name": "Theirs"})).await?;
        profile(f, &admin, json!({"name": "shared-writer", "display_name": "Writer", "shared": true})).await?;
        let lead = task(f, person.id).await?;
        let mut body = brief("reviewer");
        body["profile"] = json!("legal-reviewer");
        let (status, started) = start(f, &lead.token, "c-1", &body).await?;
        assert_eq!(status, 201, "{started}");
        assert_eq!(started["member"]["profile"], "legal-reviewer");
        assert_eq!(started["member"]["left_out"], json!(["wiki"]));
        let (route, template, servers, connectors, name, tools): (String, String, Vec<String>, Vec<String>, String, Value) =
            sqlx::query_as(
                "SELECT model_route, template, mcp_servers, connectors, profile->>'name', profile->'tools' \
                 FROM conversations WHERE id = $1",
            )
            .bind(uuid(&started["member"]["conversation"])?)
            .fetch_one(&f.pool)
            .await?;
        assert_eq!((route.as_str(), template.as_str(), name.as_str()), ("spare", "extra", "legal-reviewer"));
        assert_eq!((servers, connectors), (vec!["docs".to_owned()], Vec::<String>::new()));
        assert_eq!(tools, json!(["read_file", "grep"]));
        // A shared profile may be used; another person's personal one and an unknown one are the same refusal.
        body["profile"] = json!("shared-writer");
        assert_eq!(start(f, &lead.token, "c-2", &body).await?.0, 201);
        for (key, name) in [("c-3", "theirs"), ("c-4", "nope")] {
            body["profile"] = json!(name);
            let (status, refused) = start(f, &lead.token, key, &body).await?;
            assert_eq!(
                (status, &refused["error"]["code"], &refused["error"]["details"][0]["field"]),
                (422, &json!("unknown_reference"), &json!("profile")),
                "{name}"
            );
        }
        Ok(())
    }))
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_037_a_member_cant_start_one_and_past_the_cap_nothing_is_stored(
    pool: PgPool,
) -> TestResult {
    Box::pin(support::run_configured(
        pool,
        Some(registry()?),
        &config("max_members = 2"),
        async |f| {
            f.admin().await?;
            let (person, _) = f.member("owner@example.invalid").await?;
            let lead = task(f, person.id).await?;
            let (_, first) = start(f, &lead.token, "c-1", &brief("a")).await?;
            start(f, &lead.token, "c-2", &brief("b")).await?;
            let member = connected(
                f,
                &first["member"]["conversation"]
                    .as_str()
                    .ok_or("id")?
                    .parse()?,
            )
            .await?;
            let (status, refused) = start(f, &member.token, "m-1", &brief("sub")).await?;
            assert_eq!(
                (status, &refused["error"]["code"]),
                (403, &json!("not_lead"))
            );
            let before: i64 = sqlx::query_scalar("SELECT count(*) FROM conversations")
                .fetch_one(&f.pool)
                .await?;
            let (status, refused) = start(f, &lead.token, "c-3", &brief("c")).await?;
            assert_eq!(
                (
                    status,
                    &refused["error"]["code"],
                    &refused["error"]["details"]
                ),
                (
                    409,
                    &json!("member_limit"),
                    &json!({"limit": 2, "by": "max_members"})
                )
            );
            let after: i64 = sqlx::query_scalar("SELECT count(*) FROM conversations")
                .fetch_one(&f.pool)
                .await?;
            assert_eq!(before, after, "a refused start leaves no task behind");
            Ok(())
        },
    ))
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_106_a_retried_start_answers_the_first_and_a_bad_one_names_its_field(
    pool: PgPool,
) -> TestResult {
    Box::pin(support::run_configured(
        pool,
        Some(registry()?),
        &config(""),
        async |f| {
            f.admin().await?;
            let (person, _) = f.member("owner@example.invalid").await?;
            let lead = task(f, person.id).await?;
            let (status, first) = start(f, &lead.token, "c-1", &brief("researcher")).await?;
            assert_eq!(status, 201, "{first}");
            let (status, again) = start(f, &lead.token, "c-1", &brief("researcher")).await?;
            assert_eq!((status, &again), (201, &first));
            let (status, reused) = start(f, &lead.token, "c-1", &brief("other")).await?;
            assert_eq!(
                (status, &reused["error"]["code"]),
                (422, &json!("idempotency_key_reused"))
            );
            let members: i64 = sqlx::query_scalar("SELECT count(*) FROM agent_team_members")
                .fetch_one(&f.pool)
                .await?;
            assert_eq!(members, 1);
            let mut vague = brief("x");
            vague["brief"]
                .as_object_mut()
                .ok_or("brief")?
                .remove("output");
            let (status, refused) = start(f, &lead.token, "c-2", &vague).await?;
            assert_eq!(
                (
                    status,
                    &refused["error"]["code"],
                    &refused["error"]["details"][0]["field"]
                ),
                (400, &json!("invalid_request"), &json!("output"))
            );
            Ok(())
        },
    ))
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_106_start_member_is_refused_while_agent_teams_are_off(pool: PgPool) -> TestResult {
    Box::pin(support::run_configured(
        pool,
        Some(registry()?),
        "",
        async |f| {
            f.admin().await?;
            let (person, _) = f.member("owner@example.invalid").await?;
            let lead = task(f, person.id).await?;
            let (status, refused) = start(f, &lead.token, "c-1", &brief("researcher")).await?;
            assert_eq!(
                (status, &refused["error"]["code"]),
                (403, &json!("teams_disabled"))
            );
            Ok(())
        },
    ))
    .await
}

async fn role(f: &Fixture, token: &str) -> Result<(u16, Value)> {
    let response = f
        .client
        .get(f.url("/runtime/teams/role"))
        .bearer_auth(token)
        .send()
        .await?;
    let status = response.status().as_u16();
    Ok((status, response.json().await?))
}

// TEST: GW-I-214
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_214_a_task_linked_below_13_is_never_offered_the_lead(pool: PgPool) -> TestResult {
    Box::pin(support::run_configured(
        pool,
        Some(registry()?),
        &config(""),
        async |f| {
            f.admin().await?;
            let (person, _) = f.member("owner@example.invalid").await?;
            // In no team yet, so the link takes it; but its later wakes couldn't reach it, so it never leads.
            for (revision, expected) in [
                (tqa_types::LEAD_BRIEF_REVISION, "none"),
                (tqa_types::TEAM_WAKE_REVISION, "may_lead"),
            ] {
                let conversation = f.conversation(person.id).await?;
                let _runtime = f.runtime_at(&conversation.id, revision).await?;
                let token = f
                    .backend
                    .token(&conversation.id)
                    .ok_or("token")?
                    .expose_secret()
                    .to_owned();
                assert_eq!(
                    role(f, &token).await?,
                    (200, json!({"role": expected})),
                    "revision {revision}"
                );
            }
            Ok(())
        },
    ))
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_202_a_runs_role_tells_which_team_tools_it_gets(pool: PgPool) -> TestResult {
    Box::pin(support::run_configured(
        pool,
        Some(registry()?),
        &config(""),
        async |f| {
            f.admin().await?;
            let (person, _) = f.member("owner@example.invalid").await?;
            let lead = task(f, person.id).await?;
            // A fresh top-level task may lead: it is offered start_member alone.
            assert_eq!(
                role(f, &lead.token).await?,
                (200, json!({"role": "may_lead"}))
            );
            let (_, started) = start(f, &lead.token, "c-1", &brief("researcher")).await?;
            assert_eq!(role(f, &lead.token).await?, (200, json!({"role": "lead"})));
            let member = connected(
                f,
                &started["member"]["conversation"]
                    .as_str()
                    .ok_or("id")?
                    .parse()?,
            )
            .await?;
            assert_eq!(
                role(f, &member.token).await?,
                (200, json!({"role": "member"}))
            );
            // A member of an ended team never leads (depth 1): no team tools at all.
            sqlx::query("UPDATE agent_teams SET state = 'ended', ended_at = CURRENT_TIMESTAMP")
                .execute(&f.pool)
                .await?;
            assert_eq!(
                role(f, &member.token).await?,
                (200, json!({"role": "none"}))
            );
            // One team per lead: the ended team's lead gets no team tools...
            assert_eq!(
                role(f, &lead.token).await?,
                (200, json!({"role": "none"}))
            );
            // ...except, while its end note is unread, read_reports alone, which delivers it (a team that ended
            // between its runs is still told once).
            let seq: i64 = sqlx::query_scalar(
                "WITH next AS (UPDATE channels c SET last_seq = last_seq + 1 FROM agent_teams t \
                 WHERE t.channel_id = c.id AND t.lead_conversation = $1 RETURNING c.id, c.last_seq) \
                 INSERT INTO channel_messages (channel_id, seq, author_kind, author_id, author_name, type, to_kind, body) \
                 SELECT id, last_seq, 'gateway', 'gateway', 'gateway', 'ended', 'lead', \
                 '{\"type\": \"ended\", \"text\": \"The team stopped: done.\"}' FROM next RETURNING seq",
            )
            .bind(lead.id)
            .fetch_one(&f.pool)
            .await?;
            assert_eq!(
                role(f, &lead.token).await?,
                (200, json!({"role": "ended"}))
            );
            sqlx::query("UPDATE agent_teams SET lead_last_read_seq = $1 WHERE lead_conversation = $2")
                .bind(seq)
                .bind(lead.id)
                .execute(&f.pool)
                .await?;
            assert_eq!(
                role(f, &lead.token).await?,
                (200, json!({"role": "none"})),
                "once read, nothing"
            );
            // Without a runtime token, nothing.
            let (status, _) = role(f, "not-a-token").await?;
            assert_eq!(status, 401);
            Ok(())
        },
    ))
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_202_with_agent_teams_off_every_role_is_none(pool: PgPool) -> TestResult {
    Box::pin(support::run_configured(
        pool,
        Some(registry()?),
        "",
        async |f| {
            f.admin().await?;
            let (person, _) = f.member("owner@example.invalid").await?;
            let lead = task(f, person.id).await?;
            assert_eq!(role(f, &lead.token).await?, (200, json!({"role": "none"})));
            Ok(())
        },
    ))
    .await
}

/// An administrator's write of a limit (ADR-0028), at the version the listing gives.
async fn set_limit(f: &Fixture, cookie: &str, key: &str, value: u64) -> Result<()> {
    let listing: Value = f.get("/api/v1/admin/limits", cookie).await?.json().await?;
    let version = listing["items"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["key"] == key))
        .and_then(|item| item["version"].as_i64())
        .ok_or_else(|| format!("no {key} in the listing"))?;
    let response = f
        .client
        .put(f.url(&format!("/api/v1/admin/limits/{key}")))
        .header("origin", f.origin())
        .header("cookie", cookie)
        .json(&json!({"value": value, "version": version}))
        .send()
        .await?;
    assert_eq!(response.status().as_u16(), 200, "{key}");
    Ok(())
}

// TEST: GW-I-203
// TEST: GW-I-204
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_203_the_administrators_settings_apply_to_the_next_helper_and_team(
    pool: PgPool,
) -> TestResult {
    Box::pin(support::run_configured(pool, Some(registry()?), &config(""), async |f| {
        let (_, admin) = f.admin().await?;
        // The helper's route, set by the administrator (no file value).
        let put = f
            .client
            .put(f.url("/api/v1/admin/agent-teams"))
            .header("origin", f.origin())
            .header("cookie", &admin)
            .json(&json!({"member_route": "spare", "acceptance_check": null, "version": 0}))
            .send()
            .await?;
        assert_eq!(put.status().as_u16(), 200);
        // A team's budget and deadline, and the cap on helpers, from the next team and helper on.
        set_limit(f, &admin, "agent_teams.token_budget", 50_000).await?;
        set_limit(f, &admin, "agent_teams.deadline_minutes", 30).await?;
        set_limit(f, &admin, "agent_teams.max_members", 1).await?;
        let (person, _) = f.member("owner@example.invalid").await?;
        let lead = task(f, person.id).await?;
        let (status, started) = start(f, &lead.token, "s-1", &brief("helper")).await?;
        assert_eq!(status, 201, "{started}");
        let route: String = sqlx::query_scalar("SELECT model_route FROM conversations WHERE id = $1")
            .bind(uuid(&started["member"]["conversation"])?)
            .fetch_one(&f.pool)
            .await?;
        assert_eq!(route, "spare", "the administrator's route");
        let (budget, minutes): (i64, f64) = sqlx::query_as(
            "SELECT token_budget, EXTRACT(EPOCH FROM deadline_at - created_at)::float8 / 60 FROM agent_teams \
             WHERE lead_conversation = $1",
        )
        .bind(lead.id)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(budget, 50_000);
        assert!((minutes - 30.0).abs() < 1.0, "a 30-minute deadline: {minutes}");
        let (status, refused) = start(f, &lead.token, "s-2", &brief("second")).await?;
        assert_eq!((status, refused["error"]["code"].clone()), (409, json!("member_limit")), "the cap of 1");
        Ok(())
    }))
    .await
}
