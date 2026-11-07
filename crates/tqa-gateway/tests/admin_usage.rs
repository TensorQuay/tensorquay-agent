//! GW-I-053 (090): `GET /api/v1/admin/usage` reports relayed-request usage by day, user or route, for administrators.

#[path = "support/fixture.rs"]
mod support;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// One usage row for `conversation` at `at` (RFC 3339), with the given token counts.
async fn usage(
    f: &Fixture,
    conversation: &str,
    route: &str,
    at: &str,
    tokens: [Option<i64>; 4],
) -> TestResult {
    let [prompt, completion, cached, reasoning] = tokens;
    sqlx::query(
        "INSERT INTO usage (id,conversation_id,route,prompt_tokens,completion_tokens,cached_tokens,reasoning_tokens,created_at) \
         VALUES ($1,$2::uuid,$3,$4,$5,$6,$7,$8::timestamptz)",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(conversation)
    .bind(route)
    .bind(prompt)
    .bind(completion)
    .bind(cached)
    .bind(reasoning)
    .bind(at)
    .execute(&f.pool)
    .await?;
    Ok(())
}

async fn report(f: &Fixture, cookie: &str, query: &str) -> Result<(u16, Value)> {
    let response = f
        .get(&format!("/api/v1/admin/usage?{query}"), cookie)
        .await?;
    Ok((response.status().as_u16(), response.json().await?))
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_053_days_are_contiguous_and_unreported_counts_stay_null(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (admin, cookie) = f.admin().await?;
        let id = f.conversation(admin.id).await?.id.to_string();
        usage(f, &id, "local", "2025-12-31T23:59:59Z", [Some(100), Some(100), None, None]).await?;
        usage(f, &id, "local", "2026-01-01T00:00:00Z", [Some(10), Some(5), Some(3), None]).await?;
        usage(f, &id, "local", "2026-01-01T12:00:00Z", [None, None, None, None]).await?;
        usage(f, &id, "local", "2026-01-03T23:59:59Z", [Some(1), Some(1), None, Some(2)]).await?;
        let (status, body) = report(f, &cookie, "from=2026-01-01&to=2026-01-03").await?;
        assert_eq!(status, 200, "{body}");
        let row = |key: &str, counts: [i64; 4], cached: Value, reasoning: Value| {
            json!({"key": key, "label": null, "requests": counts[0], "reported": counts[1], "estimated": 0,
                   "prompt_tokens": counts[2], "completion_tokens": counts[3],
                   "cached_tokens": cached, "reasoning_tokens": reasoning})
        };
        assert_eq!(
            body,
            json!({"from": "2026-01-01", "to": "2026-01-03", "group": "day", "truncated": false,
                "rows": [row("2026-01-01", [2, 1, 10, 5], json!(3), Value::Null),
                         row("2026-01-02", [0, 0, 0, 0], Value::Null, Value::Null),
                         row("2026-01-03", [1, 1, 1, 1], Value::Null, json!(2))],
                "totals": {"requests": 3, "reported": 2, "estimated": 0, "prompt_tokens": 11, "completion_tokens": 6,
                           "cached_tokens": 3, "reasoning_tokens": 2}})
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_053_users_and_routes_are_ranked_bounded_and_totalled(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (admin, cookie) = f.admin().await?;
        let (member, _) = f.member("member@example.invalid").await?;
        let own = f.conversation(admin.id).await?.id.to_string();
        let theirs = f.conversation(member.id).await?.id.to_string();
        usage(
            f,
            &own,
            "local",
            "2026-02-01T00:00:00Z",
            [Some(1), Some(1), None, None],
        )
        .await?;
        usage(
            f,
            &theirs,
            "second",
            "2026-02-01T00:00:00Z",
            [Some(50), Some(50), None, None],
        )
        .await?;
        sqlx::query("UPDATE conversations SET state='deleted' WHERE id=$1::uuid")
            .bind(&theirs)
            .execute(&f.pool)
            .await?;
        let (status, body) = report(f, &cookie, "from=2026-02-01&to=2026-02-01&group=user").await?;
        assert_eq!(status, 200, "{body}");
        let keys: Vec<(Value, Value)> = body["rows"]
            .as_array()
            .ok_or("rows")?
            .iter()
            .map(|row| (row["key"].clone(), row["label"].clone()))
            .collect();
        assert_eq!(
            keys,
            vec![
                (
                    json!(member.id.to_string()),
                    json!("member@example.invalid")
                ),
                (json!(admin.id.to_string()), json!("admin@example.invalid"))
            ]
        );
        for index in 0..201 {
            usage(
                f,
                &own,
                &format!("route-{index:03}"),
                "2026-02-02T00:00:00Z",
                [Some(index), Some(0), None, None],
            )
            .await?;
        }
        let (status, body) =
            report(f, &cookie, "from=2026-02-02&to=2026-02-02&group=route").await?;
        assert_eq!(status, 200, "{body}");
        let rows = body["rows"].as_array().ok_or("rows")?;
        assert_eq!((rows.len(), &body["truncated"]), (200, &json!(true)));
        assert_eq!(
            (&rows[0]["key"], &rows[199]["key"]),
            (&json!("route-200"), &json!("route-001"))
        );
        assert_eq!(body["totals"]["requests"], 201);
        assert_eq!(body["totals"]["prompt_tokens"], (0..201).sum::<i64>());
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_053_only_administrators_get_bounded_valid_ranges(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let (_, member) = f.member("member@example.invalid").await?;
        let (status, body) = report(f, &member, "").await?;
        assert_eq!((status, &body["error"]["code"]), (403, &json!("forbidden")));
        assert_eq!(
            f.client
                .get(f.url("/api/v1/admin/usage"))
                .send()
                .await?
                .status(),
            401
        );
        let (status, body) = report(f, &cookie, "").await?;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            body["rows"].as_array().ok_or("rows")?.len(),
            30,
            "the default is the 30 days ending today"
        );
        for (query, field, reason) in [
            ("from=2026-01-05&to=2026-01-04", "from", "range_reversed"),
            ("from=2026-01-01&to=2026-04-04", "to", "range_too_long"),
            ("to=2999-01-01", "to", "date_future"),
            ("from=2026-02-30", "from", "date_invalid"),
            ("group=week", "group", "group_invalid"),
            ("limit=5", "query", "parameter_invalid"),
            ("group=day&group=user", "query", "parameter_invalid"),
        ] {
            let (status, body) = report(f, &cookie, query).await?;
            assert_eq!(status, 400, "{query}");
            assert_eq!(body["error"]["code"], "invalid_request", "{query}");
            assert_eq!(
                body["error"]["details"],
                json!([{"field": field, "reason": reason}]),
                "{query}"
            );
        }
        let (status, body) = report(f, &cookie, "from=2026-01-01&to=2026-04-03").await?;
        assert_eq!(
            (status, body["rows"].as_array().map(Vec::len)),
            (200, Some(93)),
            "92 days apart is the longest range"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-118
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_118_routes_carry_their_names_and_compare_previous_adds_the_period_before(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (admin, cookie) = f.admin().await?;
        let id = f.conversation(admin.id).await?.id.to_string();
        sqlx::query("UPDATE model_routes SET display_name = 'Local model' WHERE name = 'local'")
            .execute(&f.pool)
            .await?;
        // The period (three days) and the three days before it; a row just outside both.
        usage(
            f,
            &id,
            "local",
            "2026-01-04T00:00:00Z",
            [Some(10), Some(5), None, None],
        )
        .await?;
        usage(
            f,
            &id,
            "gone",
            "2026-01-06T23:59:59Z",
            [Some(1), Some(1), None, None],
        )
        .await?;
        usage(
            f,
            &id,
            "local",
            "2026-01-01T00:00:00Z",
            [Some(7), Some(3), None, None],
        )
        .await?;
        usage(
            f,
            &id,
            "local",
            "2026-01-03T23:59:59Z",
            [Some(1), Some(1), None, None],
        )
        .await?;
        usage(
            f,
            &id,
            "local",
            "2025-12-31T23:59:59Z",
            [Some(100), Some(100), None, None],
        )
        .await?;
        let (status, body) = report(
            f,
            &cookie,
            "from=2026-01-04&to=2026-01-06&group=route&compare=previous",
        )
        .await?;
        assert_eq!(status, 200, "{body}");
        let labels: Vec<(Value, Value)> = body["rows"]
            .as_array()
            .ok_or("rows")?
            .iter()
            .map(|row| (row["key"].clone(), row["label"].clone()))
            .collect();
        assert_eq!(
            labels,
            [
                (json!("local"), json!("Local model")),
                (json!("gone"), Value::Null)
            ]
        );
        assert_eq!(
            (&body["previous_from"], &body["previous_to"]),
            (&json!("2026-01-01"), &json!("2026-01-03"))
        );
        assert_eq!(
            body["previous"],
            json!({"requests": 2, "reported": 2, "estimated": 0, "prompt_tokens": 8, "completion_tokens": 4,
                   "cached_tokens": null, "reasoning_tokens": null})
        );
        assert_eq!(body["totals"]["requests"], 2);
        // Without compare there is no previous period in the answer; an unknown value is refused.
        let (_, plain) = report(f, &cookie, "from=2026-01-04&to=2026-01-06&group=route").await?;
        assert!(
            plain.get("previous").is_none() && plain.get("previous_from").is_none(),
            "{plain}"
        );
        let (status, _) = report(
            f,
            &cookie,
            "from=2026-01-04&to=2026-01-06&compare=last_year",
        )
        .await?;
        assert_eq!(status, 400);
        Ok(())
    })
    .await
}
