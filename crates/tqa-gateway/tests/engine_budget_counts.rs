//! GW-I-126 (migration 0058; decided on 29 Sep 2026): when an engine's budget counts from. `total` counts from when
//! the budget was set or last changed, so saving a new budget starts a new count and saving the engine with the same
//! budget keeps it; `month` counts the calendar month in UTC whatever changed in it (raising the cap only raises the
//! ceiling). Every change of a budget is its own audit row, action `budget`, with the old and new budget, from the API
//! and from the seed. The administrators' list shows each budget's use over its current count.
//!
//! The engine's address never answers (`engine.invalid`), so a consultation the budget lets through is a 502 (sent,
//! failed, and counted like any consultation) and one it refuses is a 409 before anything is sent.
#[path = "support/fixture.rs"]
mod support;

use reqwest::Method;
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::net::Ipv4Addr;
use support::{Fixture, TestResult, call};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn engine(budget: &Value, timeout_ms: u32, version: Option<i64>) -> Value {
    let mut body = json!({"url": "https://engine.invalid", "auth": {"scheme": "none"},
        "timeout_ms": timeout_ms, "budget": budget});
    if let Some(version) = version {
        body["version"] = json!(version);
    }
    body
}
async fn put(f: &Fixture, cookie: &str, body: Value) -> Result<Value> {
    let (status, answer) = call(
        f,
        Method::PUT,
        "/api/v1/admin/decision/engines/router",
        cookie,
        (Some(body), None),
    )
    .await?;
    assert!(matches!(status, 200 | 201), "{status}: {answer}");
    Ok(answer)
}
/// The runtime token of a new conversation of `admin` whose profile names the engine.
async fn caller(f: &Fixture, admin: &tqa_gateway::User) -> Result<SecretString> {
    let conversation = f.conversation(admin.id).await?;
    let profile = json!({"name":"p","display_name":"P","skills":"all","mcp_servers":[],"connectors":[],"rules":[],
        "limits":{},"decision_engine":"router"});
    sqlx::query(
        "UPDATE conversations SET profile=$2, profile_id=$3, profile_version=1 WHERE id=$1::uuid",
    )
    .bind(conversation.id.as_str())
    .bind(&profile)
    .bind(uuid::Uuid::now_v7())
    .execute(&f.pool)
    .await?;
    Ok(f.gateway
        .runtime_tokens()
        .mint(
            &conversation.id,
            &format!("count-{}", uuid::Uuid::now_v7().simple()).parse()?,
            Ipv4Addr::LOCALHOST.into(),
        )
        .await?)
}
/// One consultation's status: 502 when the budget let it through, 409 when it refused it.
async fn consult(f: &Fixture, token: &SecretString) -> Result<u16> {
    let request = json!({
        "point": "before_step", "run_id": "r-1", "step": 1, "attempt": 1,
        "routes": [{"name":"local","context_window":null,"max_output":null,"native_tools":true}],
        "tools": [], "budget": {"steps_used":0,"max_steps":30,"output_tokens_used":0,"max_output_tokens":1000},
        "previous_calls": []
    });
    Ok(f.client
        .post(f.url("/relay/d/router/v1/before_step"))
        .bearer_auth(token.expose_secret())
        .json(&request)
        .send()
        .await?
        .status()
        .as_u16())
}
/// The engine as the administrators' list shows it.
async fn listed(f: &Fixture, cookie: &str) -> Result<Value> {
    let (_, list) = call(
        f,
        Method::GET,
        "/api/v1/admin/decision/engines",
        cookie,
        (None, None),
    )
    .await?;
    Ok(list["items"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["name"] == "router"))
        .cloned()
        .ok_or("the engine isn't listed")?)
}
/// The engine's `budget` audit rows, oldest first: actor kind and detail.
async fn budget_rows(f: &Fixture) -> Result<Vec<(String, Option<String>)>> {
    Ok(sqlx::query_as(
        "SELECT actor_kind, detail FROM admin_audit WHERE area='decision_engines' AND item='router' AND action='budget' \
         ORDER BY at",
    )
    .fetch_all(&f.pool)
    .await?)
}

// TEST: GW-I-126
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_126_saving_a_new_total_budget_starts_a_new_count_and_the_same_one_keeps_it(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (admin, cookie) = f.admin().await?;
        let token = caller(f, &admin).await?;
        put(f, &cookie, engine(&json!({"calls": 2}), 500, None)).await?;
        assert_eq!(consult(f, &token).await?, 502, "within the budget: sent");
        assert_eq!(consult(f, &token).await?, 502);
        assert_eq!(consult(f, &token).await?, 409, "spent");
        let item = listed(f, &cookie).await?;
        assert_eq!(
            item["budget_used"],
            json!({"calls": 2, "tokens": 0}),
            "{item}"
        );
        assert!(item["budget_since"].is_string(), "{item}");
        // Another field changed, the same budget: the count goes on.
        put(f, &cookie, engine(&json!({"calls": 2}), 900, Some(1))).await?;
        assert_eq!(consult(f, &token).await?, 409, "still spent");
        // A new budget: a new count, from now.
        put(f, &cookie, engine(&json!({"calls": 3}), 900, Some(2))).await?;
        assert_eq!(
            consult(f, &token).await?,
            502,
            "counted from the new budget"
        );
        assert_eq!(
            listed(f, &cookie).await?["budget_used"],
            json!({"calls": 1, "tokens": 0})
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-126
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_126_a_change_in_the_month_keeps_the_months_count(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (admin, cookie) = f.admin().await?;
        let token = caller(f, &admin).await?;
        put(
            f,
            &cookie,
            engine(&json!({"calls": 2, "period": "month"}), 500, None),
        )
        .await?;
        assert_eq!(consult(f, &token).await?, 502);
        assert_eq!(consult(f, &token).await?, 502);
        assert_eq!(consult(f, &token).await?, 409);
        // Raising the cap in the month raises the ceiling only: two are used, one more passes.
        put(
            f,
            &cookie,
            engine(&json!({"calls": 3, "period": "month"}), 500, Some(1)),
        )
        .await?;
        assert_eq!(consult(f, &token).await?, 502);
        assert_eq!(consult(f, &token).await?, 409, "three used this month");
        assert_eq!(
            listed(f, &cookie).await?["budget_used"],
            json!({"calls": 3, "tokens": 0})
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-126
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_126_every_budget_change_is_its_own_audit_row(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        put(f, &cookie, engine(&json!({"calls": 2}), 500, None)).await?;
        put(f, &cookie, engine(&json!({"calls": 2}), 900, Some(1))).await?;
        put(
            f,
            &cookie,
            engine(&json!({"calls": 3000, "period": "month"}), 900, Some(2)),
        )
        .await?;
        let mut none = engine(&Value::Null, 900, Some(3));
        none.as_object_mut().ok_or("an object")?.remove("budget");
        put(f, &cookie, none).await?;
        assert_eq!(
            budget_rows(f).await?,
            vec![
                ("admin".to_owned(), Some("none->calls2_total".to_owned())),
                (
                    "admin".to_owned(),
                    Some("calls2_total->calls3000_month".to_owned())
                ),
                ("admin".to_owned(), Some("calls3000_month->none".to_owned())),
            ],
            "one row per change; none for a save with the same budget"
        );
        assert!(
            listed(f, &cookie).await?.get("budget_used").is_none(),
            "no budget, no use shown"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-126
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_126_a_seeded_budget_is_audited_as_the_seeds(pool: PgPool) -> TestResult {
    let seed = "[[decision_engines]]\nname = \"router\"\nurl = \"https://engine.invalid\"\n\
                budget = { calls = 5, period = \"month\" }\n";
    support::run_configured(pool, None, seed, async |f| {
        assert_eq!(
            budget_rows(f).await?,
            vec![("seed".to_owned(), Some("none->calls5_month".to_owned()))]
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-126
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_126_the_budget_count_reads_an_index_on_route_and_time(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        // Counted on every budgeted consultation: by route over a time range, never a scan of every model call.
        let definitions: Vec<String> =
            sqlx::query_scalar("SELECT indexdef FROM pg_indexes WHERE tablename='usage'")
                .fetch_all(&f.pool)
                .await?;
        assert!(
            definitions
                .iter()
                .any(|definition| definition.contains("(route, created_at)")),
            "{definitions:?}"
        );
        Ok(())
    })
    .await
}
