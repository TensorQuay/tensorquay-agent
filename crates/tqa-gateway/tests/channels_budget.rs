//! A channel's wake budget, set by the people who run it (ADR-0014's amendment, A5, decided on 20 Nov 2026, C-1): the
//! channel's owner, a person who leads it, or an administrator sets it, between 0 and the installation's ceiling; the
//! channel says who set it; a wake the budget held goes out once it is raised, without a new post (GW-I-376). The budget
//! is a daily allowance (decided on 21 Nov 2026, C-3): its window opens at the first charge and lasts a day, then spent
//! is 0 again and the held wakes go out (GW-I-385); an upgrade starts every channel at 0 (GW-I-386).
#[path = "support/channel_agent.rs"]
mod channel_agent;
#[path = "support/channel_spend.rs"]
mod channel_spend;
#[path = "support/fixture.rs"]
mod support;
use channel_agent::{Agent, WAKE, channel, notices, post, text};
use channel_spend::{model, model_call, registry, spent};
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

/// The channel as its owner reads it.
async fn read(f: &Fixture, cookie: &str, id: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let (status, body) = call(
        f,
        Method::GET,
        &format!("/api/v1/channels/{id}"),
        cookie,
        (None, None),
    )
    .await?;
    assert_eq!(status, 200, "{body}");
    Ok(body)
}

/// The text and `resets_at` of the channel's latest notice of `kind`.
async fn notice_of(
    f: &Fixture,
    id: &str,
    kind: &str,
) -> Result<(String, Value), Box<dyn std::error::Error>> {
    let body: Value = sqlx::query_scalar(
        "SELECT body FROM channel_messages WHERE channel_id = $1::uuid AND body->>'kind' = $2 ORDER BY seq DESC LIMIT 1",
    )
    .bind(id)
    .bind(kind)
    .fetch_one(&f.pool)
    .await?;
    Ok((
        body["text"].as_str().unwrap_or_default().to_owned(),
        body["resets_at"].clone(),
    ))
}

// TEST: GW-I-385
// C-3 and C-14 (decided on 21 Nov 2026): the budget is per day. The window opens at the first charge; the channel's
// read says when it renews (`budget_resets_at`, null while none is open); the 80 % and spent notices say "for today",
// and the spent one says when agents are woken again and who can raise it sooner, with the time as `resets_at`. When
// the window ends, spent is 0 again and the wake the budget held goes out without a new post.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_385_the_budget_renews_a_day_after_its_first_charge(pool: PgPool) -> TestResult {
    let server = model(5).await?;
    Box::pin(support::run_configured(
        pool,
        Some(registry(&server)?),
        "\n[channels]\nwake_budget = 100\nbudget_window_seconds = 12\n",
        async |f| {
            let (owner, cookie) = f.admin().await?;
            let mut analyst = Agent::new(f, owner.id, "Pricing analyst").await?;
            let id = channel(f, &cookie, "pricing", &[(&analyst, "member")]).await?;
            let secret = secrecy::SecretString::from(format!("synthetic-provider-{}", uuid::Uuid::now_v7()));
            f.gateway.secret_store().put(&"secrets/local".parse()?, secret).await?;
            let fresh = read(f, &cookie, &id).await?;
            assert_eq!((&fresh["spent_tokens"], &fresh["budget_resets_at"]), (&json!(0), &Value::Null), "{fresh}");
            // Three woken runs: 40 (the window opens), 80 (the 80 % notice), 120 (spent).
            let mut place = 0;
            let mut opened = None;
            for (round, total) in [(1, 40), (2, 80), (3, 120)] {
                post(f, &cookie, &id, text(&format!("@pricing analyst round {round}")), None).await?;
                analyst.woken(f, &id, place).await?;
                analyst.start(f).await?;
                place = *analyst.read(f, &id, place).await?.last().ok_or("read")?;
                let before = time::OffsetDateTime::now_utc();
                assert_eq!(model_call(f, &analyst).await?.0, 200);
                assert_eq!(spent(f, &id, total).await?, total);
                let view = read(f, &cookie, &id).await?;
                let resets = view["budget_resets_at"].as_str().ok_or("budget_resets_at")?.to_owned();
                if let Some(first) = &opened {
                    assert_eq!(&resets, first, "the window keeps its start");
                } else {
                    let at = time::OffsetDateTime::parse(&resets, &time::format_description::well_known::Rfc3339)?;
                    let after = at - before;
                    assert!(after > time::Duration::seconds(10) && after <= time::Duration::seconds(13), "{resets}");
                    opened = Some(resets);
                }
                analyst.end(f).await?;
            }
            let (said, _) = notice_of(f, &id, "wake_budget_80").await?;
            assert_eq!(said, "Woken agents have used 80 % of this channel's budget for today.");
            let (said, resets_at) = notice_of(f, &id, "wake_budget_spent").await?;
            assert!(said.starts_with("Woken agents have used this channel's budget for today: no agent is woken here until it renews at "), "{said}");
            assert!(said.contains(" UTC") && said.ends_with(", or until the channel's owner, a lead or an administrator raises it."), "{said}");
            assert_eq!(resets_at.as_str(), opened.as_deref(), "the notice carries the time the page shows");
            // Held: a post now wakes no one.
            post(f, &cookie, &id, text("@pricing analyst round 4"), None).await?;
            analyst.quiet(f).await?;
            // The window ends: the held wake goes out with no new post, and the channel starts again at 0.
            assert!(analyst.next(f, WAKE + WAKE).await?.is_some(), "the held wake after the reset");
            let renewed = read(f, &cookie, &id).await?;
            assert_eq!((&renewed["spent_tokens"], &renewed["budget_resets_at"]), (&json!(0), &Value::Null), "{renewed}");
            Ok(())
        },
    ))
    .await
}

/// The migrations before `version`.
fn before(version: i64) -> sqlx::migrate::Migrator {
    sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            tqa_gateway::MIGRATOR
                .migrations
                .iter()
                .filter(|migration| migration.version < version)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    }
}

// TEST: GW-I-386
// C-3 (decided on 21 Nov 2026): migration 0111 starts every channel at 0 with no window open, so one the old lifetime
// budget had stopped can be woken again; a budget nobody set follows the new default, and one someone set keeps its
// number.
#[sqlx::test(migrations = false)]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_386_an_upgrade_starts_every_channels_budget_at_zero(pool: PgPool) -> TestResult {
    support::require_database()?;
    before(111).run(&pool).await?;
    let (old_owner, unset, set) = (
        uuid::Uuid::now_v7(),
        uuid::Uuid::now_v7(),
        uuid::Uuid::now_v7(),
    );
    sqlx::query("INSERT INTO users (id, email, password_hash, role) VALUES ($1, 'old-owner@example.invalid', 'x', 'member')")
        .bind(old_owner)
        .execute(&pool)
        .await?;
    for (channel, budget, spent) in [
        (unset, None, 300_000_i64),
        (set, Some(500_000_i64), 640_000),
    ] {
        sqlx::query("INSERT INTO channels (id, owner_id, name, rule, budget, spent_tokens) VALUES ($1, $2, 'old', 'human', $3, $4)")
            .bind(channel)
            .bind(old_owner)
            .bind(budget)
            .bind(spent)
            .execute(&pool)
            .await?;
    }
    tqa_gateway::MIGRATOR.run(&pool).await?;
    // Both start at 0 with no window open; the budget nobody set stays unset (the installation's default applies), the
    // one someone set keeps its number.
    let rows: Vec<(uuid::Uuid, Option<i64>, i64, Option<time::OffsetDateTime>)> =
        sqlx::query_as("SELECT id, budget, spent_tokens, budget_window_start FROM channels")
            .fetch_all(&pool)
            .await?;
    let of = |id| {
        rows.iter()
            .find(|row| row.0 == id)
            .map(|row| (row.1, row.2, row.3))
    };
    assert_eq!(of(unset), Some((None, 0, None)), "{rows:?}");
    assert_eq!(of(set), Some((Some(500_000), 0, None)), "{rows:?}");
    Ok(())
}
