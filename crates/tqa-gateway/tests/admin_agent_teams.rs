//! GW-I-204 (ADR-0034, the slice S6 contract): the agent teams' two settings that aren't numbers, a helper's model
//! route and the acceptance check, as administrators read and write them: one versioned row, a stale version refused,
//! an unknown route refused, the check one line of at most 4,096 bytes, every write audited; administrators only, and
//! `403 teams_disabled` while agent teams are off.
#[path = "support/fixture.rs"]
mod support;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const ON: &str = "[agent_teams]\nenabled=true\n";

async fn get(f: &Fixture, cookie: &str) -> Result<(u16, Value)> {
    let response = f
        .client
        .get(f.url("/api/v1/admin/agent-teams"))
        .header("origin", f.origin())
        .header("cookie", cookie)
        .send()
        .await?;
    let status = response.status().as_u16();
    Ok((status, response.json().await?))
}

async fn put(f: &Fixture, cookie: &str, body: &Value) -> Result<(u16, Value)> {
    let response = f
        .client
        .put(f.url("/api/v1/admin/agent-teams"))
        .header("origin", f.origin())
        .header("cookie", cookie)
        .json(body)
        .send()
        .await?;
    let status = response.status().as_u16();
    Ok((status, response.json().await?))
}

fn code(answer: &Value) -> Value {
    answer["error"]["code"].clone()
}

// TEST: GW-I-204
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_204_administrators_read_and_write_the_two_settings_with_their_version(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, ON, async |f| {
        let (_, cookie) = f.admin().await?;
        assert_eq!(
            get(f, &cookie).await?,
            (200, json!({"member_route": null, "acceptance_check": null, "version": 0}))
        );
        let set = json!({"member_route": "local", "acceptance_check": "cargo test --workspace", "version": 0});
        let written = json!({"member_route": "local", "acceptance_check": "cargo test --workspace", "version": 1});
        assert_eq!(put(f, &cookie, &set).await?, (200, written.clone()));
        assert_eq!(get(f, &cookie).await?, (200, written));
        // A write from a stale read is refused, and nothing changes.
        let (status, answer) = put(f, &cookie, &json!({"member_route": null, "acceptance_check": null, "version": 0})).await?;
        assert_eq!((status, code(&answer)), (409, json!("conflict")));
        // A route the install doesn't have; a check of two lines, or over the bound.
        let (status, answer) = put(f, &cookie, &json!({"member_route": "nowhere", "acceptance_check": null, "version": 1})).await?;
        assert_eq!((status, code(&answer), answer["error"]["details"][0]["field"].clone()), (422, json!("unknown_reference"), json!("member_route")));
        for check in [json!("make check\nrm -rf /"), json!("x".repeat(4097)), json!("")] {
            let (status, answer) = put(f, &cookie, &json!({"member_route": null, "acceptance_check": check, "version": 1})).await?;
            assert_eq!((status, answer["error"]["details"][0]["field"].clone()), (400, json!("acceptance_check")), "{check}");
        }
        let (status, _) = put(f, &cookie, &json!({"member_route": null, "acceptance_check": null, "version": 1, "extra": 1})).await?;
        assert_eq!(status, 400, "a field it doesn't know");
        // Clearing both is a write like any other.
        assert_eq!(
            put(f, &cookie, &json!({"member_route": null, "acceptance_check": null, "version": 1})).await?,
            (200, json!({"member_route": null, "acceptance_check": null, "version": 2}))
        );
        let audited: i64 = sqlx::query_scalar("SELECT count(*) FROM admin_audit WHERE area = 'agent_teams' AND item = 'settings'")
            .fetch_one(&f.pool)
            .await?;
        assert_eq!(audited, 2, "each accepted write, never a refused one");
        // Not an administrator.
        let (_, member) = f.member("member@example.invalid").await?;
        assert_eq!(get(f, &member).await?.0, 403);
        assert_eq!(put(f, &member, &json!({"member_route": null, "acceptance_check": null, "version": 2})).await?.0, 403);
        Ok(())
    })
    .await
}

// TEST: GW-I-204
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_204_while_agent_teams_are_off_the_settings_answer_teams_disabled(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let (status, answer) = get(f, &cookie).await?;
        assert_eq!((status, code(&answer)), (403, json!("teams_disabled")));
        let (status, answer) = put(
            f,
            &cookie,
            &json!({"member_route": null, "acceptance_check": null, "version": 0}),
        )
        .await?;
        assert_eq!((status, code(&answer)), (403, json!("teams_disabled")));
        Ok(())
    })
    .await
}
