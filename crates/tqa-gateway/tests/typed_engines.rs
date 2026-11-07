//! GW-I-125 (ADR-0013, the adapter amendment): a typed-question engine, reached through the relay. The relay turns the
//! checked `decision/v1` request into one multiple-choice question to the engine's service, with the engine's key and
//! pinned model, the step's routes as the options (each with its hint) and nothing the exposure excludes, and answers
//! the runtime with the `call_model` it builds: the chosen route, or the conversation's own below `min_confidence`.
//! Each failure is typed: an unusable answer or refused question is `decision_engine_invalid` (the runtime's
//! `invalid`), a failed or key-refusing service `upstream_unavailable`, a late one `timeout`; a state over 16 KiB is
//! refused before sending, and not counted (no usage row), since the budget models what the service charges. The
//! service's tokens go on the usage row, and the runtime is never asked after a response.
#[path = "support/fixture.rs"]
mod support;

use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{net::Ipv4Addr, time::Duration};
use support::TestResult;
use tqa_testkit::{FakeQuestionServer, QuestionAnswer};

fn config(service: &FakeQuestionServer) -> String {
    let url = format!("{}/v1/evaluate", service.base_url());
    format!(
        "[[decision_engines]]\nname = \"router\"\nprotocol = \"typed_questions_v1\"\nurl = \"{url}\"\ninsecure_http = true\n\
         auth = {{ scheme = \"bearer\" }}\nkey_ref = \"secrets/questions\"\nmodel = \"pinned-model\"\ntimeout_ms = 300\n\
         route_hints = {{ cheap = \"routine steps: reading, looking something up\" }}\n\n\
         [[decision_engines]]\nname = \"wide\"\nprotocol = \"typed_questions_v1\"\nurl = \"{url}\"\ninsecure_http = true\n\
         model = \"pinned-model\"\nexposure = \"full\"\n"
    )
}
fn request(point: &str) -> Value {
    json!({
        "point": point, "run_id": "r-1", "step": 2, "attempt": 1,
        "routes": [{"name":"local","context_window":null,"max_output":null,"native_tools":true},
                   {"name":"cheap","context_window":null,"max_output":null,"native_tools":false}],
        "tools": [{"name":"read_file","description":"Read","parameters":{"type":"object"}}],
        "budget": {"steps_used":1,"max_steps":30,"output_tokens_used":120,"max_output_tokens":1000},
        "previous_calls": [{"name":"read_file","status":"ok"}]
    })
}

/// A conversation of `admin` whose profile names `engine`, its runtime token, and the engine's key in the store.
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
            &format!("typed-{}", uuid::Uuid::now_v7().simple()).parse()?,
            Ipv4Addr::LOCALHOST.into(),
        )
        .await?;
    f.gateway
        .secret_store()
        .put(
            &"secrets/questions".parse()?,
            SecretString::from("engine-key"),
        )
        .await?;
    Ok((conversation, token))
}
async fn consult(
    f: &support::Fixture,
    token: &SecretString,
    engine: &str,
    point: &str,
    body: &Value,
) -> Result<(u16, Value), Box<dyn std::error::Error>> {
    let response = f
        .client
        .post(f.url(&format!("/relay/d/{engine}/v1/{point}")))
        .bearer_auth(token.expose_secret())
        .json(body)
        .send()
        .await?;
    let status = response.status().as_u16();
    let text = response.text().await?;
    Ok((status, serde_json::from_str(&text).unwrap_or(Value::Null)))
}
fn choose(choice: &str, confidence: f64) -> QuestionAnswer {
    QuestionAnswer::Choose {
        choice: choice.into(),
        confidence,
        tokens: (640, 12),
    }
}

// TEST: GW-I-125
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_125_one_question_per_step_answered_as_a_call_model(pool: PgPool) -> TestResult {
    let service =
        FakeQuestionServer::start(vec![choose("cheap", 0.9), choose("cheap", 0.4)]).await?;
    support::run_configured(pool, None, &config(&service), async |f| {
        let admin = f.admin().await?;
        let (conversation, token) = caller(f, &admin, "router").await?;
        let (status, body) = consult(f, &token, "router", "before_step", &request("before_step")).await?;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body, json!({"decision":"call_model","route":"cheap","tools":["read_file"]}));
        // Below min_confidence (0.6 by default): the conversation's own route.
        let (_, low) = consult(f, &token, "router", "before_step", &request("before_step")).await?;
        assert_eq!(low["route"], "local", "{low}");
        let seen = service.requests();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].path, "/v1/evaluate");
        assert_eq!(seen[0].authorization.as_deref(), Some("Bearer engine-key"));
        let question = &seen[0].body;
        assert_eq!(question["model"], "pinned-model");
        assert_eq!(question["questions"]["route"]["type"], "choice");
        assert_eq!(
            question["questions"]["route"]["criteria"],
            json!({"local": "local", "cheap": "routine steps: reading, looking something up"})
        );
        assert_eq!(question["state"]["tools"], json!(["read_file"]), "tool names only");
        assert!(question["state"].get("transcript").is_none() && question["state"].get("response_text").is_none());
        // The service's tokens on the consultation's usage row (written as the call ends).
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let tokens: Option<(Option<i64>, Option<i64>)> = sqlx::query_as(
                    "SELECT prompt_tokens, completion_tokens FROM usage WHERE conversation_id=$1::uuid \
                     AND route='decision:router' AND prompt_tokens IS NOT NULL LIMIT 1",
                )
                .bind(conversation.id.as_str())
                .fetch_optional(&f.pool)
                .await?;
                if tokens == Some((Some(640), Some(12))) {
                    return Ok::<(), sqlx::Error>(());
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await??;
        // Never asked after a response: this kind answers before a step only.
        let (status, _) = consult(f, &token, "router", "after_response", &request("after_response")).await?;
        assert_eq!(status, 404);
        assert_eq!(service.requests().len(), 2, "nothing more was sent");
        Ok(())
    })
    .await?;
    service.stop().await;
    Ok(())
}

// TEST: GW-I-125
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_125_each_failure_is_typed_and_a_big_state_is_never_sent(pool: PgPool) -> TestResult {
    let service = FakeQuestionServer::start(vec![
        choose("elsewhere", 0.9),
        QuestionAnswer::Raw {
            status: 200,
            body: b"{\"answers\":{}}".to_vec(),
        },
        QuestionAnswer::Raw {
            status: 422,
            body: b"{}".to_vec(),
        },
        QuestionAnswer::Raw {
            status: 401,
            body: b"{}".to_vec(),
        },
        QuestionAnswer::Raw {
            status: 529,
            body: b"{}".to_vec(),
        },
        QuestionAnswer::Delay(Duration::from_secs(2), Box::new(choose("cheap", 0.9))),
    ])
    .await?;
    support::run_configured(pool, None, &config(&service), async |f| {
        let admin = f.admin().await?;
        let (_, token) = caller(f, &admin, "router").await?;
        for (expected_status, expected_code, what) in [
            (
                502,
                "decision_engine_invalid",
                "a choice outside the options",
            ),
            (
                502,
                "decision_engine_invalid",
                "an answer without the question",
            ),
            (
                502,
                "decision_engine_invalid",
                "a question the service refused as invalid",
            ),
            (502, "upstream_unavailable", "the key refused"),
            (502, "upstream_unavailable", "the service overloaded"),
            (504, "timeout", "past timeout_ms"),
        ] {
            let (status, body) =
                consult(f, &token, "router", "before_step", &request("before_step")).await?;
            assert_eq!(
                (status, body["error"]["code"].as_str()),
                (expected_status, Some(expected_code)),
                "{what}: {body}"
            );
        }
        // A state over 16 KiB (a long transcript, which only a full-exposure engine may see) is never sent.
        let (_, wide_token) = caller(f, &admin, "wide").await?;
        let mut big = request("before_step");
        big["transcript"] = json!([{"role":"user","text":"x".repeat(17 * 1024)}]);
        let sent = service.requests().len();
        let (status, body) = consult(f, &wide_token, "wide", "before_step", &big).await?;
        assert_eq!(
            (status, body["error"]["code"].as_str()),
            (502, Some("decision_engine_invalid")),
            "{body}"
        );
        assert_eq!(service.requests().len(), sent, "nothing was sent");
        // Refused before anything was sent, so it costs nothing and is not counted against the budget.
        let counted: i64 =
            sqlx::query_scalar("SELECT count(*) FROM usage WHERE route='decision:wide'")
                .fetch_one(&f.pool)
                .await?;
        assert_eq!(
            counted, 0,
            "a question refused locally is not a consultation"
        );
        Ok(())
    })
    .await?;
    service.stop().await;
    Ok(())
}

// TEST: GW-I-125
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_125_the_runtime_is_told_to_ask_before_steps_only(pool: PgPool) -> TestResult {
    let service = FakeQuestionServer::start(vec![]).await?;
    support::run_configured(pool, None, &config(&service), async |f| {
        let (user, _) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let profile = json!({"name":"p","display_name":"P","skills":"all","mcp_servers":[],"connectors":[],
            "rules":[],"limits":{},"decision_engine":"router"});
        sqlx::query("UPDATE conversations SET profile=$2, profile_id=$3, profile_version=1 WHERE id=$1::uuid")
            .bind(conversation.id.as_str())
            .bind(&profile)
            .bind(uuid::Uuid::now_v7())
            .execute(&f.pool)
            .await?;
        let runtime = f.runtime(&conversation).await?;
        assert_eq!(runtime.config["decision_engine"]["points"], json!(["before_step"]));
        Ok(())
    })
    .await?;
    service.stop().await;
    Ok(())
}
