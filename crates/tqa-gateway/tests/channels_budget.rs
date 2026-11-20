//! A channel's wake budget, set by the people who run it (ADR-0014's amendment, A5, decided on 20 Nov 2026, C-1): the
//! channel's owner, a person who leads it, or an administrator sets it, between 0 and the installation's ceiling; the
//! channel says who set it; a wake the budget held goes out once it is raised, without a new post (GW-I-376).
#[path = "support/channel_agent.rs"]
mod channel_agent;
#[path = "support/fixture.rs"]
mod support;
use channel_agent::{Agent, channel, notices, post, text};
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};

async fn set(
    f: &Fixture,
    cookie: &str,
    id: &str,
    body: Value,
) -> Result<(u16, Value), Box<dyn std::error::Error>> {
    call(
        f,
        Method::PATCH,
        &format!("/api/v1/channels/{id}"),
        cookie,
        (Some(body), None),
    )
    .await
}

// TEST: GW-I-376
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_376_a_channels_lead_sets_its_wake_budget(pool: PgPool) -> TestResult {
    Box::pin(support::run(pool, async |f| {
        let (ana, ana_cookie) = f.admin().await?;
        let (daniel, daniel_cookie) = f.member("daniel@example.invalid").await?;
        let (lee, lee_cookie) = f.member("lee@example.invalid").await?;
        let mut analyst = Agent::new(f, ana.id, "Pricing analyst").await?;
        let id = channel(f, &ana_cookie, "pricing", &[(&analyst, "member")]).await?;
        for (person, role) in [(&daniel, "member"), (&lee, "lead")] {
            let (status, body) = call(
                f,
                Method::PUT,
                &format!("/api/v1/channels/{id}/members/user/{}", person.id),
                &ana_cookie,
                (Some(json!({"role": role})), None),
            )
            .await?;
            assert_eq!(status, 200, "{body}");
        }
        // A plain member may not; a lead and the owner may, and the answer says the budget and the spend.
        let (status, body) = set(f, &daniel_cookie, &id, json!({"wake_budget": 500_000})).await?;
        assert_eq!((status, body["error"]["code"].clone()), (403, json!("not_lead")), "{body}");
        let (status, body) = set(f, &lee_cookie, &id, json!({"wake_budget": 500_000})).await?;
        assert_eq!(status, 200, "{body}");
        assert_eq!((body["wake_budget"].clone(), body["spent_tokens"].clone()), (json!(500_000), json!(0)));
        // Between 0 (waking off) and the installation's ceiling (10,000,000 by default), said in words.
        for bad in [json!(-1), json!("lots"), json!(10_000_001), json!(null)] {
            let (status, body) = set(f, &ana_cookie, &id, json!({"wake_budget": bad})).await?;
            assert_eq!(status, 400, "{bad}: {body}");
        }
        let (status, body) = set(f, &ana_cookie, &id, json!({"wake_budget": 10_000_001})).await?;
        assert!(body["error"]["message"].as_str().unwrap_or("").contains("10,000,000"), "{status} {body}");
        // The channel says who set it, each time.
        assert_eq!(notices(f, &id, "wake_budget_set").await?, 1);
        // A budget of 0 holds the wake; raising it sends the held wake, with no new post.
        let (status, _) = set(f, &ana_cookie, &id, json!({"wake_budget": 0})).await?;
        assert_eq!(status, 200);
        post(f, &ana_cookie, &id, text("@Pricing analyst churn by plan, please"), None).await?;
        analyst.quiet(f).await?;
        let (status, _) = set(f, &ana_cookie, &id, json!({"wake_budget": 1_000})).await?;
        assert_eq!(status, 200);
        assert!(analyst.next(f, channel_agent::WAKE).await?.is_some(), "the held wake after the raise");
        assert_eq!(notices(f, &id, "wake_budget_set").await?, 3);
        let said: String = sqlx::query_scalar(
            "SELECT body->>'text' FROM channel_messages WHERE channel_id = $1::uuid AND body->>'kind' = 'wake_budget_set' \
             ORDER BY seq DESC LIMIT 1",
        )
        .bind(&id)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(said, format!("{} set this channel's budget to 1,000 tokens.", ana.email));
        Ok(())
    }))
    .await
}
