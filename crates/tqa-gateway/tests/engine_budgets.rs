//! GW-I-126 (ADR-0013, the adapter amendment): an engine's optional budget, an administrator's setting. It counts the
//! engine's usage rows (calls, or the service's tokens) over its period, the whole history or a calendar month in UTC,
//! in the relay, before any call leaves the gateway. Once spent, a consultation is refused with 409
//! `decision_engine_budget_spent` (the agent decides alone for that step) and the task's owner is told once. Parallel
//! consultations at the last unit overshoot by at most their number, and the count survives a restart.
#[path = "support/fixture.rs"]
mod support;

use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::net::Ipv4Addr;
use support::TestResult;
use tqa_testkit::{EngineAnswer, FakeEngineServer, FakeQuestionServer, QuestionAnswer};
use tqa_types::decision_v1::Response;

fn request() -> Value {
    json!({
        "point": "before_step", "run_id": "r-1", "step": 1, "attempt": 1,
        "routes": [{"name":"local","context_window":null,"max_output":null,"native_tools":true}],
        "tools": [{"name":"read_file","description":"Read","parameters":{"type":"object"}}],
        "budget": {"steps_used":0,"max_steps":30,"output_tokens_used":0,"max_output_tokens":1000},
        "previous_calls": []
    })
}
fn decide() -> EngineAnswer {
    EngineAnswer::Decide(Response::CallModel {
        route: "local".into(),
        tools: vec!["read_file".into()],
    })
}

/// A conversation of the admin whose profile names `engine`, and its runtime token.
async fn caller(
    f: &support::Fixture,
    admin: &(tqa_gateway::User, String),
    engine: &str,
) -> Result<(tqa_gateway::Conversation, SecretString), Box<dyn std::error::Error>> {
    let conversation = f.conversation(admin.0.id).await?;
    let profile = json!({"name":"p","display_name":"P","skills":"all","mcp_servers":[],"connectors":[],"rules":[],
        "limits":{},"decision_engine":engine});
    sqlx::query(
        "UPDATE conversations SET profile=$2, profile_id=$3, profile_version=1 WHERE id=$1::uuid",
    )
    .bind(conversation.id.as_str())
    .bind(&profile)
    .bind(uuid::Uuid::now_v7())
    .execute(&f.pool)
    .await?;
    let token = f
        .gateway
        .runtime_tokens()
        .mint(
            &conversation.id,
            &format!("budget-{}", uuid::Uuid::now_v7().simple()).parse()?,
            Ipv4Addr::LOCALHOST.into(),
        )
        .await?;
    Ok((conversation, token))
}
async fn consult(
    f: &support::Fixture,
    token: &SecretString,
    engine: &str,
) -> Result<(u16, Value), Box<dyn std::error::Error>> {
    let response = f
        .client
        .post(f.url(&format!("/relay/d/{engine}/v1/before_step")))
        .bearer_auth(token.expose_secret())
        .json(&request())
        .send()
        .await?;
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let text = response.text().await?;
    let body = serde_json::from_str(&text).unwrap_or(Value::Null);
    support::gateway_refusal(&headers, &body)?;
    Ok((status, body))
}
fn plain(engine: &FakeEngineServer, budget: &str) -> String {
    format!(
        "[[decision_engines]]\nname = \"router\"\nurl = \"{}\"\ninsecure_http = true\nbudget = {budget}\n",
        engine.base_url()
    )
}

// TEST: GW-I-126
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_126_a_spent_call_budget_refuses_before_any_call_and_tells_the_owner_once(
    pool: PgPool,
) -> TestResult {
    let engine = FakeEngineServer::start(vec![decide(), decide(), decide()]).await?;
    Box::pin(support::run_restarting_with(
        pool,
        None,
        &plain(&engine, "{ calls = 2 }"),
        async |f| {
            let admin = f.admin().await?;
            let (conversation, token) = caller(f, &admin, "router").await?;
            let mut stream = f.stream(&conversation.id, &admin.1, 0).await?;
            assert_eq!(consult(f, &token, "router").await?.0, 200);
            assert_eq!(consult(f, &token, "router").await?.0, 200);
            for _ in 0..2 {
                let (status, body) = consult(f, &token, "router").await?;
                assert_eq!(
                    (status, body["error"]["code"].as_str()),
                    (409, Some("decision_engine_budget_spent")),
                    "{body}"
                );
            }
            assert_eq!(
                engine.requests().len(),
                2,
                "nothing leaves the gateway once it is spent"
            );
            // One notice for the task, however many consultations are refused.
            let mut notices = 0;
            for _ in 0..8 {
                let frame =
                    tokio::time::timeout(std::time::Duration::from_millis(300), stream.next())
                        .await;
                let Ok(Ok((_, frame))) = frame else { break };
                if matches!(
                    frame,
                    tqa_types::ServerFrame::Error {
                        code: tqa_types::ErrorCode::DecisionEngineBudgetSpent,
                        retryable: false,
                        ..
                    }
                ) {
                    notices += 1;
                }
            }
            assert_eq!(notices, 1);
            // The count is the usage rows, so a restart changes nothing.
            f.restart().await?;
            let (status, _) = consult(f, &token, "router").await?;
            assert_eq!(status, 409, "still spent after a restart");
            Ok(())
        },
    ))
    .await?;
    engine.stop().await;
    Ok(())
}

// TEST: GW-I-126
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_126_a_token_budget_counts_what_the_service_reports(pool: PgPool) -> TestResult {
    let call = || QuestionAnswer::Choose {
        choice: "local".into(),
        confidence: 0.9,
        tokens: (640, 12),
    };
    let service = FakeQuestionServer::start(vec![call(), call(), call()]).await?;
    let config = format!(
        "[[decision_engines]]\nname = \"router\"\nprotocol = \"typed_questions_v1\"\nurl = \"{}/v1/evaluate\"\n\
         insecure_http = true\nmodel = \"pinned-model\"\nbudget = {{ tokens = 1000 }}\n",
        service.base_url()
    );
    support::run_configured(pool, None, &config, async |f| {
        let admin = f.admin().await?;
        let (conversation, token) = caller(f, &admin, "router").await?;
        assert_eq!(consult(f, &token, "router").await?.0, 200);
        // The first call's 652 tokens land on its usage row as it ends; the second is still under 1000.
        wait_for_tokens(f, &conversation, 652).await?;
        assert_eq!(consult(f, &token, "router").await?.0, 200);
        wait_for_tokens(f, &conversation, 1304).await?;
        let (status, body) = consult(f, &token, "router").await?;
        assert_eq!(
            (status, body["error"]["code"].as_str()),
            (409, Some("decision_engine_budget_spent"))
        );
        assert_eq!(service.requests().len(), 2);
        Ok(())
    })
    .await?;
    service.stop().await;
    Ok(())
}

async fn wait_for_tokens(
    f: &support::Fixture,
    conversation: &tqa_gateway::Conversation,
    total: i64,
) -> TestResult {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let sum: Option<i64> = sqlx::query_scalar(
                "SELECT sum(COALESCE(prompt_tokens,0)+COALESCE(completion_tokens,0))::bigint FROM usage \
                 WHERE conversation_id=$1::uuid AND route='decision:router'",
            )
            .bind(conversation.id.as_str())
            .fetch_one(&f.pool)
            .await?;
            if sum == Some(total) {
                return Ok::<(), sqlx::Error>(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await??;
    Ok(())
}

// TEST: GW-I-126
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_126_a_monthly_budget_counts_this_calendar_month_in_utc(pool: PgPool) -> TestResult {
    let engine = FakeEngineServer::start(vec![decide()]).await?;
    support::run_configured(pool, None, &plain(&engine, "{ calls = 2, period = \"month\" }"), async |f| {
        let admin = f.admin().await?;
        let (conversation, token) = caller(f, &admin, "router").await?;
        // Two consultations last month (UTC) don't count; one this month does.
        let now = time::OffsetDateTime::now_utc();
        let month = now.replace_day(1)?.replace_time(time::Time::MIDNIGHT);
        for created in [
            month - time::Duration::hours(1),
            month - time::Duration::days(20),
            now,
        ] {
            sqlx::query(
                "INSERT INTO usage (id, conversation_id, route, created_at) VALUES ($1, $2::uuid, 'decision:router', $3)",
            )
            .bind(uuid::Uuid::now_v7())
            .bind(conversation.id.as_str())
            .bind(created)
            .execute(&f.pool)
            .await?;
        }
        assert_eq!(consult(f, &token, "router").await?.0, 200, "one this month, under two");
        assert_eq!(consult(f, &token, "router").await?.0, 409, "two this month");
        Ok(())
    })
    .await?;
    engine.stop().await;
    Ok(())
}

// TEST: GW-I-126
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_126_parallel_consultations_at_the_last_unit_overshoot_by_at_most_their_number(
    pool: PgPool,
) -> TestResult {
    let engine =
        FakeEngineServer::start(vec![decide(), decide(), decide(), decide(), decide()]).await?;
    support::run_configured(pool, None, &plain(&engine, "{ calls = 2 }"), async |f| {
        let admin = f.admin().await?;
        let (_, token) = caller(f, &admin, "router").await?;
        assert_eq!(
            consult(f, &token, "router").await?.0,
            200,
            "one of two used"
        );
        // Four at once at the last unit: at least one passes, and at most all four (the rule: the overshoot is at
        // most the consultations in flight when it runs out).
        let results = futures::future::join_all((0..4).map(|_| consult(f, &token, "router"))).await;
        let passed = results
            .iter()
            .filter(|result| matches!(result, Ok((200, _))))
            .count();
        assert!((1..=4).contains(&passed), "{passed} passed");
        assert!(
            results
                .iter()
                .all(|result| matches!(result, Ok((200 | 409, _)))),
            "every other one is refused as spent"
        );
        assert_eq!(
            consult(f, &token, "router").await?.0,
            409,
            "spent after the burst"
        );
        assert!(engine.requests().len() <= 1 + 4);
        Ok(())
    })
    .await?;
    engine.stop().await;
    Ok(())
}
