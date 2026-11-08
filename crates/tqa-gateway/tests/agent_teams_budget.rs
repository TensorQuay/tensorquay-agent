//! GW-I-107 and SEC-039 (ADR-0034 point 8): an agent team's budget is enforced by the gateway, at the relay. The
//! relay adds each answer's prompt and completion tokens to the team's spend, lead and members together; at 80 % the
//! lead gets one `budget_80` notice; at 100 % the team stops (`budget`); past it, or past the deadline, a request is
//! refused `team_budget` before anything is forwarded, whatever the runtime says. A team stopping for another reason
//! refuses its members `team_stopped` but lets its lead finish its answer; an ended team's tasks are ordinary tasks
//! again, charged nothing to the team. GW-I-261: the budget's stop is carried out within a second. GW-I-275: a model call
//! cut off before its server reported usage (a stop mid-reply) still adds its spend, the relay's estimate, to the
//! person's usage and to the team's budget, and both say it is an estimate.
#[path = "support/fixture.rs"]
mod support;
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::Arc;
use support::{Fixture, TestResult};
use tqa_testkit::{FakeModelServer, Script, Scripted};
use uuid::Uuid;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
/// A usage row's prompt, completion and reasoning tokens, and whether they are the relay's estimate.
type UsageCounts = (Option<i64>, Option<i64>, Option<i64>, bool);

/// The team's budget in tokens; each answer below costs 40 (30 in, 10 out).
const BUDGET: i64 = 100;

fn registry(server: &FakeModelServer) -> Result<tqa_llm::Registry> {
    Ok(tqa_llm::Registry::from_toml(&format!(
        "[[routes]]\nname='local'\nprotocol='openai_chat'\nbase_url='{}'\nmodel='pinned-model'\nauth={{kind='bearer'}}\nkey_ref='secrets/local'\nstream_usage=true",
        server.base_url()
    ))?)
}

/// A model server answering `answers` requests, each reporting 40 tokens.
async fn server(answers: usize) -> Result<FakeModelServer> {
    let script = Script::new(vec![
        Scripted::Text("done".into()),
        Scripted::Usage(tqa_llm::Usage {
            prompt_tokens: 30,
            completion_tokens: 10,
            cached_tokens: None,
            reasoning_tokens: None,
        }),
    ]);
    Ok(FakeModelServer::start(vec![script; answers], Arc::new(tqa_llm::TokioSleeper)).await?)
}

/// A lead task and a member task of one team (its log channel, its budget), and a task outside it.
struct Team {
    id: Uuid,
    /// The owner's (an administrator's) session cookie.
    cookie: String,
    channel: Uuid,
    lead: tqa_types::ConversationId,
    member: tqa_types::ConversationId,
    outsider: tqa_types::ConversationId,
}

async fn team(f: &Fixture) -> Result<Team> {
    team_with(f, BUDGET).await
}

/// A team whose budget is `budget` tokens, with one working member and a task outside it.
async fn team_with(f: &Fixture, budget: i64) -> Result<Team> {
    let (owner, cookie) = f.admin().await?;
    let lead = f.conversation(owner.id).await?.id;
    let member = f.conversation(owner.id).await?.id;
    let outsider = f.conversation(owner.id).await?.id;
    let (id, channel) = (Uuid::now_v7(), Uuid::now_v7());
    sqlx::query("INSERT INTO channels (id, owner_id, name, rule, kind) VALUES ($1, $2, 'Agent team', 'human', 'agent_team')")
        .bind(channel)
        .bind(owner.id)
        .execute(&f.pool)
        .await?;
    sqlx::query(
        "INSERT INTO agent_teams (id, channel_id, lead_conversation, owner_id, token_budget, deadline_at) \
         VALUES ($1, $2, $3, $4, $5, CURRENT_TIMESTAMP + interval '1 hour')",
    )
    .bind(id)
    .bind(channel)
    .bind(Uuid::parse_str(lead.as_str())?)
    .bind(owner.id)
    .bind(budget)
    .execute(&f.pool)
    .await?;
    sqlx::query(
        "INSERT INTO agent_team_members (conversation_id, team_id, name, role, workspace, state) \
         VALUES ($1, $2, 'researcher', 'researcher', 'own_copy', 'working')",
    )
    .bind(Uuid::parse_str(member.as_str())?)
    .bind(id)
    .execute(&f.pool)
    .await?;
    let secret = secrecy::SecretString::from(format!("synthetic-provider-{}", Uuid::now_v7()));
    f.gateway
        .secret_store()
        .put(&"secrets/local".parse()?, secret)
        .await?;
    Ok(Team {
        id,
        cookie,
        channel,
        lead,
        member,
        outsider,
    })
}

/// One model request from `conversation`'s runtime: its status and, when refused, the error's code.
async fn call(f: &Fixture, conversation: &tqa_types::ConversationId) -> Result<(u16, Value)> {
    let token = f
        .gateway
        .runtime_tokens()
        .mint(
            conversation,
            &format!("sandbox-{}", Uuid::now_v7().simple()).parse()?,
            std::net::Ipv4Addr::LOCALHOST.into(),
        )
        .await?;
    let response = f
        .client
        .post(f.url("/relay/r/local/chat/completions"))
        .bearer_auth(token.expose_secret())
        .json(&json!({"model":"m","messages":[{"role":"user","content":"hi"}],"stream":true}))
        .send()
        .await?;
    let status = response.status().as_u16();
    let text = response.text().await?;
    let code = serde_json::from_str::<Value>(&text)
        .map_or(Value::Null, |body| body["error"]["code"].clone());
    Ok((status, code))
}

/// The team's spend once it reaches `tokens` (the meter writes after the answer ends).
async fn spent(f: &Fixture, team: Uuid, tokens: i64) -> Result<i64> {
    tokio::time::timeout(support::DEADLINE, async {
        loop {
            let spent: i64 =
                sqlx::query_scalar("SELECT spent_tokens FROM agent_teams WHERE id = $1")
                    .bind(team)
                    .fetch_one(&f.pool)
                    .await?;
            if spent >= tokens {
                return Ok::<i64, Box<dyn std::error::Error>>(spent);
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .map_err(|_| format!("the team's spend never reached {tokens}"))?
}

/// The `budget_80` notices in the team's log.
async fn notices(f: &Fixture, channel: Uuid) -> Result<i64> {
    Ok(sqlx::query_scalar(
        "SELECT count(*) FROM channel_messages WHERE channel_id = $1 AND type = 'notice' \
         AND body->>'kind' = 'budget_80' AND to_kind = 'lead'",
    )
    .bind(channel)
    .fetch_one(&f.pool)
    .await?)
}

const ENABLED: &str = "[agent_teams]\nenabled=true\n";

// TEST: GW-I-107
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_107_the_team_budget_counts_lead_and_members_notices_at_80_and_stops_at_100(
    pool: PgPool,
) -> TestResult {
    let model = server(3).await?;
    let result = support::run_configured(pool, Some(registry(&model)?), ENABLED, async |f| {
        // The budget's stop nudges the reconciler (GW-I-261): held, the team stays `stopping` to look at.
        f.gateway.agent_teams_hold(true);
        let t = team(f).await?;
        assert_eq!(call(f, &t.lead).await?.0, 200);
        assert_eq!(spent(f, t.id, 40).await?, 40);
        assert_eq!(notices(f, t.channel).await?, 0, "under 80 %: no notice");
        // The member's answer is the team's too: 80 % is crossed, the lead is told once.
        assert_eq!(call(f, &t.member).await?.0, 200);
        assert_eq!(spent(f, t.id, 80).await?, 80);
        assert_eq!(notices(f, t.channel).await?, 1);
        // Still under the budget, so forwarded; its answer takes the team past it, and the team stops.
        assert_eq!(call(f, &t.lead).await?.0, 200);
        assert_eq!(
            spent(f, t.id, 120).await?,
            120,
            "overshoot: at most the requests in flight"
        );
        let (state, reason): (String, Option<String>) =
            sqlx::query_as("SELECT state, stop_reason FROM agent_teams WHERE id = $1")
                .bind(t.id)
                .fetch_one(&f.pool)
                .await?;
        assert_eq!(
            (state.as_str(), reason.as_deref()),
            ("stopping", Some("budget"))
        );
        // Past the budget: refused before anything is forwarded, final (409, never retried).
        assert_eq!(call(f, &t.member).await?, (409, json!("team_budget")));
        assert_eq!(call(f, &t.lead).await?, (409, json!("team_budget")));
        assert_eq!(
            model.requests().len(),
            3,
            "nothing past the budget reached the model"
        );
        assert_eq!(
            notices(f, t.channel).await?,
            1,
            "the 80 % notice is sent once"
        );
        Ok(())
    })
    .await;
    model.shutdown().await?;
    result
}

// TEST: SEC-039
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_039_the_budget_and_deadline_cannot_be_bypassed_and_an_ended_team_frees_its_tasks(
    pool: PgPool,
) -> TestResult {
    let model = server(2).await?;
    let result = support::run_configured(pool, Some(registry(&model)?), ENABLED, async |f| {
        let t = team(f).await?;
        // Past the deadline with nothing spent: refused as spent, whatever the runtime asks.
        sqlx::query("UPDATE agent_teams SET deadline_at = CURRENT_TIMESTAMP - interval '1 minute' WHERE id = $1")
            .bind(t.id)
            .execute(&f.pool)
            .await?;
        assert_eq!(call(f, &t.member).await?, (409, json!("team_budget")));
        // A task outside the team is untouched.
        assert_eq!(call(f, &t.outsider).await?.0, 200);
        // Stopping for another reason (the lead's done): a member is refused as stopped.
        sqlx::query(
            "UPDATE agent_teams SET deadline_at = CURRENT_TIMESTAMP + interval '1 hour', state = 'stopping', \
             stop_reason = 'done' WHERE id = $1",
        )
        .bind(t.id)
        .execute(&f.pool)
        .await?;
        assert_eq!(call(f, &t.member).await?, (409, json!("team_stopped")));
        // Once the team has ended its tasks are ordinary tasks: forwarded, and no longer counted.
        sqlx::query("UPDATE agent_teams SET state = 'ended', ended_at = CURRENT_TIMESTAMP WHERE id = $1")
            .bind(t.id)
            .execute(&f.pool)
            .await?;
        assert_eq!(call(f, &t.member).await?.0, 200);
        assert_eq!(model.requests().len(), 2, "only the outsider's and the freed task's requests went");
        Ok(())
    })
    .await;
    model.shutdown().await?;
    result
}

/// Puts the team in `state` for `reason`, with time left on its deadline.
async fn stop(f: &Fixture, team: Uuid, state: &str, reason: &str) -> TestResult {
    sqlx::query(
        "UPDATE agent_teams SET state = $2, stop_reason = $3, deadline_at = CURRENT_TIMESTAMP + interval '1 hour', \
         ended_at = CASE WHEN $2 = 'ended' THEN CURRENT_TIMESTAMP END WHERE id = $1",
    )
    .bind(team)
    .bind(state)
    .bind(reason)
    .execute(&f.pool)
    .await?;
    Ok(())
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_107_a_stopping_team_lets_its_lead_finish_and_an_ended_one_charges_no_one(
    pool: PgPool,
) -> TestResult {
    // Two while running, five closing answers while stopping, one past the budget, two after the end.
    let model = server(10).await?;
    let result = support::run_configured(pool, Some(registry(&model)?), ENABLED, async |f| {
        let t = team(f).await?;
        // Running: both spend; once spent, neither.
        for who in [&t.lead, &t.member] {
            assert_eq!(call(f, who).await?.0, 200);
        }
        let set_spent = async |tokens: i64| -> TestResult {
            sqlx::query("UPDATE agent_teams SET spent_tokens = $2 WHERE id = $1")
                .bind(t.id)
                .bind(tokens)
                .execute(&f.pool)
                .await?;
            Ok(())
        };
        // Each answer is metered after it ends: wait for both before setting the spend.
        assert_eq!(spent(f, t.id, 80).await?, 80);
        set_spent(BUDGET).await?;
        for who in [&t.lead, &t.member] {
            assert_eq!(
                call(f, who).await?,
                (409, json!("team_budget")),
                "running, spent"
            );
        }
        set_spent(0).await?;
        // Stopping for anything but its budget or deadline: the lead finishes its answer, its members are refused.
        for reason in ["done", "stopped", "idle", "lead_deleted", "person_disabled"] {
            stop(f, t.id, "stopping", reason).await?;
            assert_eq!(
                call(f, &t.lead).await?.0,
                200,
                "{reason}: the lead's closing answer"
            );
            assert_eq!(
                call(f, &t.member).await?,
                (409, json!("team_stopped")),
                "{reason}: a member"
            );
        }
        assert_eq!(model.requests().len(), 7);
        // A stopping team's answers still count: the five closing answers, from nothing.
        assert_eq!(spent(f, t.id, 200).await?, 200);
        // The closing answer passes even when the budget was spent meanwhile, and is charged, as every lead call is
        // until the stop finishes.
        stop(f, t.id, "stopping", "done").await?;
        set_spent(BUDGET).await?;
        assert_eq!(call(f, &t.lead).await?.0, 200);
        assert_eq!(call(f, &t.member).await?, (409, json!("team_stopped")));
        assert_eq!(
            spent(f, t.id, BUDGET + 40).await?,
            BUDGET + 40,
            "the closing answer is charged"
        );
        // Stopping for its budget or deadline: no one spends any more.
        for reason in ["budget", "deadline"] {
            stop(f, t.id, "stopping", reason).await?;
            for who in [&t.lead, &t.member] {
                assert_eq!(call(f, who).await?, (409, json!("team_budget")), "{reason}");
            }
        }
        assert_eq!(
            model.requests().len(),
            8,
            "nothing forwarded once the budget stops it"
        );
        // Ended, even by its budget: lead and member are ordinary tasks, and the team is charged nothing more.
        stop(f, t.id, "ended", "budget").await?;
        let before: i64 = sqlx::query_scalar("SELECT spent_tokens FROM agent_teams WHERE id = $1")
            .bind(t.id)
            .fetch_one(&f.pool)
            .await?;
        for who in [&t.lead, &t.member] {
            assert_eq!(call(f, who).await?.0, 200);
        }
        assert_eq!(model.requests().len(), 10);
        // The meter writes after each answer ends; give it the time one takes.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let after: i64 = sqlx::query_scalar("SELECT spent_tokens FROM agent_teams WHERE id = $1")
            .bind(t.id)
            .fetch_one(&f.pool)
            .await?;
        assert_eq!(after, before, "an ended team is charged nothing");
        Ok(())
    })
    .await;
    model.shutdown().await?;
    result
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_107_with_agent_teams_off_no_team_row_refuses_a_call(pool: PgPool) -> TestResult {
    let model = server(1).await?;
    // Agent teams off, with a team stopping as an older setting left it: the relay never looks.
    let result = support::run_configured(pool, Some(registry(&model)?), "", async |f| {
        let t = team(f).await?;
        stop(f, t.id, "stopping", "done").await?;
        assert_eq!(call(f, &t.member).await?.0, 200);
        Ok(())
    })
    .await;
    model.shutdown().await?;
    result
}

// TEST: GW-I-261
/// Watch 2: the answer that reaches the budget sets the team stopping in the meter's transaction, and the reconciler,
/// nudged after that commit, ends the team within seconds (about one here) rather than at its next 15 s pass. The
/// bound is 3 s, scaled for slow runs but kept under 12 s, so the 15 s pass can never meet it.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_261_the_budget_s_stop_ends_the_team_at_once_not_at_the_next_pass(
    pool: PgPool,
) -> TestResult {
    let model = server(3).await?;
    let result = support::run_configured(pool, Some(registry(&model)?), ENABLED, async |f| {
        let t = team(f).await?;
        for conversation in [&t.lead, &t.member, &t.lead] {
            assert_eq!(call(f, conversation).await?.0, 200);
        }
        // The spend that reached the budget is stored with the stop, in one transaction.
        spent(f, t.id, 120).await?;
        let bound = tqa_testkit::deadline::scaled(std::time::Duration::from_secs(3))
            .min(std::time::Duration::from_secs(12));
        let ended = tokio::time::timeout(bound, async {
            loop {
                let state: String =
                    sqlx::query_scalar("SELECT state FROM agent_teams WHERE id = $1")
                        .bind(t.id)
                        .fetch_one(&f.pool)
                        .await?;
                if state == "ended" {
                    return Ok::<(), Box<dyn std::error::Error>>(());
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await;
        ended.map_err(|_| format!("the team didn't end within {bound:?}"))??;
        Ok(())
    })
    .await;
    model.shutdown().await?;
    result
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_275_a_call_cut_off_before_its_usage_still_charges_the_person_and_the_team(
    pool: PgPool,
) -> TestResult {
    // The helper's step: 3500 bytes of thinking, then a long wait in which the step is stopped, so the server's usage
    // (its last record) never comes.
    let server = FakeModelServer::start(
        vec![Script::new(vec![
            Scripted::Reasoning("t".repeat(3_500)),
            Scripted::Delay(std::time::Duration::from_secs(30)),
            Scripted::Usage(tqa_llm::Usage {
                prompt_tokens: 30,
                completion_tokens: 10,
                cached_tokens: None,
                reasoning_tokens: None,
            }),
        ])],
        Arc::new(tqa_llm::TokioSleeper),
    )
    .await?;
    let result = support::run_configured(pool, Some(registry(&server)?), ENABLED, async |f| {
        let team = team(f).await?;
        let token = f
            .gateway
            .runtime_tokens()
            .mint(
                &team.member,
                &format!("sandbox-{}", Uuid::now_v7().simple()).parse()?,
                std::net::Ipv4Addr::LOCALHOST.into(),
            )
            .await?;
        let mut response = f
            .client
            .post(f.url("/relay/r/local/chat/completions"))
            .bearer_auth(token.expose_secret())
            .json(&json!({"model":"m","messages":[{"role":"user","content":"hi"}],"stream":true}))
            .send()
            .await?;
        assert_eq!(response.status(), 200);
        // Read until the whole thinking has passed the relay, then stop the step.
        let mut received = 0;
        while received < 3_500 {
            let chunk = response.chunk().await?.ok_or("the stream ended early")?;
            received += String::from_utf8_lossy(&chunk).matches('t').count();
        }
        drop(response);
        // The usage row: the relay's estimate, marked; 3500 bytes are 1000 tokens by the runtime's byte rule.
        let row = tokio::time::timeout(support::DEADLINE, async {
            loop {
                let row: Option<UsageCounts> = sqlx::query_as(
                    "SELECT prompt_tokens, completion_tokens, reasoning_tokens, estimated FROM usage \
                     WHERE conversation_id = $1 AND completion_tokens IS NOT NULL",
                )
                .bind(Uuid::parse_str(team.member.as_str())?)
                .fetch_optional(&f.pool)
                .await?;
                if let Some(row) = row {
                    return Ok::<_, Box<dyn std::error::Error>>(row);
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .map_err(|_| "the cut-off call's usage was never written")??;
        let (prompt, completion, reasoning, estimated) = row;
        assert!(estimated, "an estimate, not the server's report");
        assert_eq!((Some(1_000), Some(1_000)), (completion, reasoning));
        let prompt = prompt.ok_or("no prompt estimate")?;
        assert!(prompt > 0, "the request's own estimate");
        // The team's budget grew by it, and the team view says its helper's tokens include an estimate.
        assert_eq!(prompt + 1_000, spent(f, team.id, prompt + 1_000).await?);
        let view: Value = f
            .get(&format!("/api/v1/conversations/{}/team", team.lead), &team.cookie)
            .await?
            .json()
            .await?;
        let helper = &view["helpers"][0];
        assert_eq!((helper["tokens"].as_i64(), helper["tokens_estimated"].as_bool()), (Some(prompt + 1_000), Some(true)));
        // The person's usage: the request counted, as an estimate, not as unreported.
        let usage: Value = f
            .get("/api/v1/admin/usage?group=user", &team.cookie)
            .await?
            .json()
            .await?;
        let totals = &usage["totals"];
        assert_eq!(
            (totals["requests"].as_i64(), totals["reported"].as_i64(), totals["estimated"].as_i64()),
            (Some(1), Some(0), Some(1))
        );
        assert_eq!(totals["completion_tokens"].as_i64(), Some(1_000));
        Ok(())
    })
    .await;
    server.shutdown().await?;
    result
}

/// The texts of the gateway's notices of `kind` to `to_kind` in the team's log, oldest first.
async fn notice_texts(
    f: &Fixture,
    channel: Uuid,
    kind: &str,
    to_kind: &str,
) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT body->>'text' FROM channel_messages WHERE channel_id = $1 AND type = 'notice' \
         AND body->>'kind' = $2 AND to_kind = $3 ORDER BY seq",
    )
    .bind(channel)
    .bind(kind)
    .bind(to_kind)
    .fetch_all(&f.pool)
    .await?)
}

// TEST: GW-I-304
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_304_at_80_percent_everyone_is_asked_for_the_work_and_the_last_10_is_the_leads(
    pool: PgPool,
) -> TestResult {
    // Real-world use before 17.0 (17 Oct 2026): two helpers spent the budget and nothing came back. At 80 % the lead
    // and each working helper are asked for the work; at 90 % the helpers stop and the lead has the rest to write up;
    // 100 % is still the end for everyone.
    let model = server(4).await?;
    let result = support::run_configured(pool, Some(registry(&model)?), ENABLED, async |f| {
        f.gateway.agent_teams_hold(true);
        // 44 tokens: one 40-token answer crosses 80 % (35.2) and 90 % (39.6) together.
        let t = team_with(f, 44).await?;
        assert_eq!(call(f, &t.member).await?.0, 200);
        assert_eq!(spent(f, t.id, 40).await?, 40);
        assert_eq!(
            notice_texts(f, t.channel, "budget_80", "lead").await?,
            ["The team has used 80 % of its budget. Each helper was asked to report what it has now. As their reports come, take their files, then finish with what you have."]
        );
        // This spend crossed the helpers' 90 % too: they are refused at their next call, so none is asked for a report
        // it can't send (the 80 % ask alone: the waiting-lead case below).
        assert!(notice_texts(f, t.channel, "budget_80", "member").await?.is_empty());
        assert_eq!(
            notice_texts(f, t.channel, "budget_helpers_stopped", "lead").await?,
            ["Your helpers stop at their next step: the team is at 90 % of its budget. Write up what you have; the team stops at 100 %."]
        );
        // 90 %: a helper is refused before anything is forwarded, with its own code (its page says why, B9b-3); the
        // lead goes on until 100 %.
        assert_eq!(call(f, &t.member).await?, (409, json!("team_budget_helpers")));
        assert_eq!(call(f, &t.lead).await?.0, 200);
        assert_eq!(spent(f, t.id, 80).await?, 80);
        assert_eq!(call(f, &t.lead).await?, (409, json!("team_budget")), "100 %: the lead too");
        assert_eq!(model.requests().len(), 2, "only the helper's first and the lead's call reached the model");
        Ok(())
    })
    .await;
    model.shutdown().await?;
    result
}

// TEST: GW-I-304
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_304_the_helpers_share_is_a_setting(pool: PgPool) -> TestResult {
    let model = server(3).await?;
    let config = "[agent_teams]\nenabled=true\nhelpers_stop_percent=50\n";
    let result = support::run_configured(pool, Some(registry(&model)?), config, async |f| {
        f.gateway.agent_teams_hold(true);
        let t = team(f).await?;
        assert_eq!(call(f, &t.member).await?.0, 200, "40 % of 100: under the helpers' 50 %");
        assert_eq!(spent(f, t.id, 40).await?, 40);
        assert_eq!(call(f, &t.member).await?.0, 200);
        assert_eq!(spent(f, t.id, 80).await?, 80);
        assert_eq!(call(f, &t.member).await?, (409, json!("team_budget_helpers")), "past 50 %: helpers stop");
        assert_eq!(call(f, &t.lead).await?.0, 200, "the lead goes on");
        // The 80 % line came after the helpers' 50 %: they are refused already, so none is asked to report.
        assert!(
            notice_texts(f, t.channel, "budget_80", "member").await?.is_empty(),
            "no report asked of a helper past its share"
        );
        assert_eq!(notice_texts(f, t.channel, "budget_80", "lead").await?.len(), 1);
        assert_eq!(
            notice_texts(f, t.channel, "budget_helpers_stopped", "lead").await?,
            ["Your helpers stop at their next step: the team is at 50 % of its budget. Write up what you have; the team stops at 100 %."]
        );
        Ok(())
    })
    .await;
    model.shutdown().await?;
    result
}

/// `reader`'s team wakes, their keys oldest first.
async fn wakes(f: &Fixture, reader: &tqa_types::ConversationId) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT key FROM idempotency WHERE conversation_id = $1 AND key LIKE 'team-wake:%' ORDER BY ordinal",
    )
    .bind(Uuid::parse_str(reader.as_str())?)
    .fetch_all(&f.pool)
    .await?)
}

/// A report from `member`'s run `run` to its lead, as its runtime posts it: the status.
async fn report(f: &Fixture, member: &tqa_types::ConversationId, run: &str) -> Result<u16> {
    let token = f
        .gateway
        .runtime_tokens()
        .mint(
            member,
            &format!("sandbox-{}", Uuid::now_v7().simple()).parse()?,
            std::net::Ipv4Addr::LOCALHOST.into(),
        )
        .await?;
    let response = f
        .client
        .post(f.url("/runtime/teams/messages"))
        .bearer_auth(token.expose_secret())
        .header("idempotency-key", format!("{run}:report"))
        .header("x-tqa-run-id", run)
        .json(&json!({"type": "report", "text": "What I have: notes.md."}))
        .send()
        .await?;
    Ok(response.status().as_u16())
}

// TEST: GW-I-304
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_304_a_lead_waiting_on_a_working_helper_reads_the_80_notice_with_its_report(
    pool: PgPool,
) -> TestResult {
    // A review before 17.0 found this class of fault: the 80 % notice woke a lead that was waiting on its helpers,
    // before the reports it asks for, and told it to finish; `done` would then stop the helpers before they reported.
    // While a helper works, the notice doesn't wake the lead: the helper's report does, and the lead's next turn reads
    // both.
    let model = server(1).await?;
    let result = support::run_configured(pool, Some(registry(&model)?), ENABLED, async |f| {
        f.gateway.agent_teams_hold(true);
        // 48 tokens: one 40-token answer crosses 80 % (38.4) and not the helpers' 90 % (43.2).
        let t = team_with(f, 48).await?;
        // The helper's run is going; the lead has no run: it waits on its helper.
        f.gateway
            .events()
            .append(&t.member, support::started(1, "r-2")?)
            .await?;
        assert_eq!(call(f, &t.member).await?.0, 200);
        assert_eq!(spent(f, t.id, 40).await?, 40);
        assert_eq!(
            notice_texts(f, t.channel, "budget_80", "lead").await?.len(),
            1
        );
        assert_eq!(
            notice_texts(f, t.channel, "budget_80", "member").await?,
            ["The team has used 80 % of its budget. Report to your lead now with what you have and any files, then carry on."],
            "one per working helper, to that helper"
        );
        f.gateway.reconcile_agent_teams_now().await?;
        assert!(
            wakes(f, &t.lead).await?.is_empty(),
            "the 80 % notice doesn't wake a lead while its helper works"
        );
        assert_eq!(report(f, &t.member, "r-2").await?, 201);
        let lead = Uuid::parse_str(t.lead.as_str())?;
        let mut woken = Vec::new();
        for _ in 0..50 {
            woken = wakes(f, &t.lead).await?;
            if !woken.is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        // One wake, from the start of the log: its turn reads the notice and the report together.
        assert_eq!(woken, [format!("team-wake:{lead}:0")]);
        let unread: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT type, body->>'kind' FROM channel_messages WHERE channel_id = $1 AND seq > 0 \
             AND to_kind IN ('lead', 'team') ORDER BY seq",
        )
        .bind(t.channel)
        .fetch_all(&f.pool)
        .await?;
        assert_eq!(
            unread,
            [
                ("notice".to_owned(), Some("budget_80".to_owned())),
                ("report".to_owned(), None)
            ]
        );
        Ok(())
    })
    .await;
    model.shutdown().await?;
    result
}

// TEST: GW-I-304
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_304_with_no_helper_working_the_80_notice_wakes_the_lead(pool: PgPool) -> TestResult {
    let model = server(1).await?;
    let result = support::run_configured(pool, Some(registry(&model)?), ENABLED, async |f| {
        f.gateway.agent_teams_hold(true);
        let t = team_with(f, 48).await?;
        // The helper has finished with what it was given; nothing will come from it.
        sqlx::query("UPDATE agent_team_members SET state = 'done' WHERE team_id = $1")
            .bind(t.id)
            .execute(&f.pool)
            .await?;
        assert_eq!(call(f, &t.lead).await?.0, 200);
        assert_eq!(spent(f, t.id, 40).await?, 40);
        f.gateway.reconcile_agent_teams_now().await?;
        let lead = Uuid::parse_str(t.lead.as_str())?;
        assert_eq!(wakes(f, &t.lead).await?, [format!("team-wake:{lead}:0")]);
        Ok(())
    })
    .await;
    model.shutdown().await?;
    result
}
