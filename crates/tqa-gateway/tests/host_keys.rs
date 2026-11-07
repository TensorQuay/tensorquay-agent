//! GW-I-161 (ADR-0037 §1, Amendment 1; the host-keys plan §1.2): a member's own host keys through the API. Add, list,
//! change, replace the value and delete; metadata only in every answer, the value never back and never in a table;
//! the contract's refusals by field; another member sees and touches none of them.
#[path = "support/fixture.rs"]
mod support;

use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// A generated test-only value; the tests only ever compare it, never print it.
const VALUE: &str = "tqa-test-host-key-5f0c7e1a93";
const REPLACED: &str = "tqa-test-host-key-replaced-2b8d";

fn key(name: &str, address: &str) -> Value {
    json!({"name": name, "address": address, "scheme": "bearer", "value": VALUE})
}

async fn add(f: &Fixture, cookie: &str, body: Value, idempotency: &str) -> Result<(u16, Value)> {
    call(
        f,
        Method::POST,
        "/api/v1/host-keys",
        cookie,
        (Some(body), Some(idempotency)),
    )
    .await
}

/// The first field refusal's reason, or the error code.
fn reason(answer: &Value) -> &str {
    answer["error"]["details"][0]["reason"]
        .as_str()
        .or_else(|| answer["error"]["code"].as_str())
        .unwrap_or_default()
}

/// The caller's key `GITHUB_WORK` for api.github.com, added: its id and the answer.
async fn github(f: &Fixture, cookie: &str) -> Result<(String, Value)> {
    let (status, added) = add(
        f,
        cookie,
        key("GITHUB_WORK", "https://api.github.com"),
        "add-1",
    )
    .await?;
    assert_eq!(status, 201, "{added}");
    Ok((added["id"].as_str().ok_or("id")?.to_owned(), added))
}

// TEST: GW-I-161
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_161_a_key_is_added_and_listed_with_metadata_only(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let (_, added) = github(f, &cookie).await?;
        for (field, expected) in [
            ("name", json!("GITHUB_WORK")),
            ("host", json!("api.github.com")),
            ("port", json!(443)),
            ("path_prefix", Value::Null),
            ("scheme", json!("bearer")),
            ("user_name", Value::Null),
            ("header_name", Value::Null),
            ("shared", json!(false)),
            ("users", Value::Null),
            ("last_use", Value::Null),
            ("allowed", json!(false)),
            ("check_name", json!(false)),
            ("version", json!(1)),
        ] {
            assert_eq!(added[field], expected, "{field}: {added}");
        }
        assert!(
            added["set_at"].is_string() && added.get("value").is_none(),
            "{added}"
        );
        // The same Idempotency-Key answers the same, and adds nothing.
        assert_eq!(
            add(
                f,
                &cookie,
                key("GITHUB_WORK", "https://api.github.com"),
                "add-1"
            )
            .await?,
            (201, added.clone())
        );
        let (status, listed) =
            call(f, Method::GET, "/api/v1/host-keys", &cookie, (None, None)).await?;
        assert_eq!(
            (status, listed["items"].as_array().map(Vec::len)),
            (200, Some(1)),
            "{listed}"
        );
        assert_eq!(listed["items"][0], added);
        assert!(
            !listed.to_string().contains(VALUE),
            "a listing never carries the value"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-161
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_161_the_contracts_refusals_name_their_field_and_reason(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let (id, _) = github(f, &cookie).await?;
        for (body, expected) in [
            (key("X", "http://api.example"), "address_not_https"),
            (key("X", "https://user@api.example"), "address_not_plain"),
            (key("X", "https://*.example.com"), "host_invalid"),
            (key("path", "https://api.example"), "name_invalid"),
            (key("PATH", "https://api.example"), "name_reserved"),
            (key("TQA_X", "https://api.example"), "name_reserved"),
            (json!({"name": "X", "address": "https://api.example", "scheme": "header", "value": VALUE}), "scheme_not_supported"),
            (json!({"name": "X", "address": "https://api.example", "scheme": "bearer", "value": ""}), "value_empty"),
            (json!({"name": "X", "address": "https://api.example", "scheme": "bearer", "value": "a\nb"}), "value_invalid"),
            (json!({"name": "X", "address": "https://api.example", "scheme": "bearer"}), "body_invalid"),
        ] {
            let (status, answer) = add(f, &cookie, body.clone(), &format!("bad-{body}")).await?;
            assert_eq!((status, reason(&answer)), (400, expected), "{body}: {answer}");
        }
        // A name the member already uses: which key has it.
        let (status, taken) = add(f, &cookie, key("GITHUB_WORK", "https://gitlab.com"), "clash").await?;
        assert_eq!((status, reason(&taken)), (409, "name_taken"), "{taken}");
        assert_eq!(taken["error"]["details"][0]["taken_by"], json!({"id": id, "shared": false}));
        Ok(())
    })
    .await
}

// TEST: GW-I-161
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_161_a_change_keeps_or_replaces_the_value_and_a_stale_one_conflicts(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let (id, added) = github(f, &cookie).await?;
        // Change without a value: the name, address and scheme; the value stays, and so does set_at.
        let path = format!("/api/v1/host-keys/{id}");
        let change = json!({"version": 1, "name": "GITHUB_ACME", "address": "https://github.com/acme/", "scheme": "basic", "user_name": "x-access-token"});
        let (status, changed) = call(f, Method::PUT, &path, &cookie, (Some(change.clone()), Some("change-1"))).await?;
        assert_eq!(status, 200, "{changed}");
        assert_eq!(
            (&changed["name"], &changed["host"], &changed["path_prefix"], &changed["scheme"], &changed["user_name"], &changed["version"], &changed["set_at"]),
            (&json!("GITHUB_ACME"), &json!("github.com"), &json!("/acme"), &json!("basic"), &json!("x-access-token"), &json!(2), &added["set_at"])
        );
        let (status, stale) = call(f, Method::PUT, &path, &cookie, (Some(change), Some("change-stale"))).await?;
        assert_eq!((status, reason(&stale)), (409, "conflict"), "{stale}");
        // Replacing the value moves set_at.
        let replace = json!({"version": 2, "name": "GITHUB_ACME", "address": "https://github.com/acme", "scheme": "basic", "user_name": "x-access-token", "value": REPLACED});
        let (status, replaced) = call(f, Method::PUT, &path, &cookie, (Some(replace), Some("replace-1"))).await?;
        assert_eq!((status, &replaced["version"]), (200, &json!(3)), "{replaced}");
        assert_ne!(replaced["set_at"], added["set_at"]);
        // The values are nowhere but the store's sealed table, and in no answer.
        let rows: String = sqlx::query_scalar("SELECT coalesce(string_agg(row_to_json(h)::text, ''), '') FROM host_keys h")
            .fetch_one(&f.pool)
            .await?;
        for value in [VALUE, REPLACED] {
            assert!(!rows.contains(value) && !changed.to_string().contains(value) && !replaced.to_string().contains(value));
        }
        Ok(())
    })
    .await
}

// TEST: GW-I-161
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_161_another_member_sees_and_touches_none_and_the_owner_reads_its_uses(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let (_, other) = f.member("b@example.invalid").await?;
        let (id, _) = github(f, &cookie).await?;
        let path = format!("/api/v1/host-keys/{id}");
        let (_, theirs) = call(f, Method::GET, "/api/v1/host-keys", &other, (None, None)).await?;
        assert_eq!(theirs["items"], json!([]));
        assert_eq!(call(f, Method::DELETE, &format!("{path}?version=1"), &other, (None, Some("steal"))).await?.0, 404);
        // Reads: the owner's own key's uses, never whose (they're all the owner's); another member gets 404.
        sqlx::query(
            "INSERT INTO secret_reads (id, ref, consumer_kind, consumer, conversation_id, user_id, run_id, outcome, \
             first_at, last_at, count) SELECT gen_random_uuid(), secret_ref, 'host', 'api.github.com:443', NULL, owner_id, \
             'run-1', 'wrong_position', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, 2 FROM host_keys",
        )
        .execute(&f.pool)
        .await?;
        let (status, reads) = call(f, Method::GET, &format!("{path}/reads"), &cookie, (None, None)).await?;
        assert_eq!(status, 200, "{reads}");
        let read = &reads["items"][0];
        assert_eq!(
            (&read["consumer_kind"], &read["consumer"], &read["outcome"], &read["count"], read.get("user_id")),
            (&json!("host"), &json!("api.github.com:443"), &json!("wrong_position"), &json!(2), None),
            "{reads}"
        );
        assert_eq!(call(f, Method::GET, &format!("{path}/reads"), &other, (None, None)).await?.0, 404);
        Ok(())
    })
    .await
}

// TEST: GW-I-161
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_161_deleting_a_key_deletes_its_sealed_value(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let (id, _) = github(f, &cookie).await?;
        let reference: String = sqlx::query_scalar("SELECT secret_ref FROM host_keys")
            .fetch_one(&f.pool)
            .await?;
        let (status, _) = call(
            f,
            Method::DELETE,
            &format!("/api/v1/host-keys/{id}?version=1"),
            &cookie,
            (None, Some("delete-1")),
        )
        .await?;
        assert_eq!(status, 204);
        let (_, after) = call(f, Method::GET, "/api/v1/host-keys", &cookie, (None, None)).await?;
        assert_eq!(after["items"], json!([]));
        let sealed: i64 = sqlx::query_scalar("SELECT count(*) FROM secrets WHERE ref=$1")
            .bind(&reference)
            .fetch_one(&f.pool)
            .await?;
        assert_eq!(sealed, 0, "the sealed value is deleted with the key");
        Ok(())
    })
    .await
}
