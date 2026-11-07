//! GW-I-162, GW-I-163 and GW-I-170 (ADR-0037 §1, R6, T12, Amendment 1; the host-keys plan §1.2): shared keys and the
//! administrator's view. Only administrators make shared keys, for everyone or chosen people; a member sees the ones
//! they may use and only their own use. An administrator sees that members' own keys exist, never their names or
//! which task used them, and may delete them but never change them. A shared key never takes a member's own name,
//! and the refusal names no one.
#[path = "support/fixture.rs"]
mod support;

use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};
use uuid::Uuid;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// A generated test-only value; the tests only ever compare it, never print it.
const VALUE: &str = "tqa-test-shared-host-key-9c41";

fn key(name: &str, address: &str) -> Value {
    json!({"name": name, "address": address, "scheme": "bearer", "value": VALUE})
}

fn shared(name: &str, address: &str, users: &Value) -> Value {
    json!({"name": name, "address": address, "scheme": "bearer", "value": VALUE, "users": users})
}

fn reason(answer: &Value) -> &str {
    answer["error"]["details"][0]["reason"]
        .as_str()
        .or_else(|| answer["error"]["code"].as_str())
        .unwrap_or_default()
}

async fn post(
    f: &Fixture,
    cookie: &str,
    path: &str,
    body: Value,
    idempotency: &str,
) -> Result<(u16, Value)> {
    call(
        f,
        Method::POST,
        path,
        cookie,
        (Some(body), Some(idempotency)),
    )
    .await
}

/// A use of a key by `user` in `conversation`, as the proxy records it (ADR-0037 §8).
async fn used(f: &Fixture, id: &str, user: Uuid, conversation: &str) -> TestResult {
    sqlx::query(
        "INSERT INTO secret_reads (id, ref, consumer_kind, consumer, conversation_id, user_id, run_id, outcome, \
         first_at, last_at, count) SELECT $1, secret_ref, 'host', host || ':' || port, $3::uuid, $4, 'run-1', 'ok', \
         CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, 3 FROM host_keys WHERE id=$2::uuid",
    )
    .bind(Uuid::now_v7())
    .bind(id)
    .bind(conversation)
    .bind(user)
    .execute(&f.pool)
    .await?;
    Ok(())
}

// TEST: GW-I-162
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_162_shared_keys_are_for_everyone_or_chosen_people(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, admin) = f.admin().await?;
        let (alice, alice_cookie) = f.member("alice@example.invalid").await?;
        let (_, bob_cookie) = f.member("bob@example.invalid").await?;
        // Members can't make shared keys.
        let (status, _) = post(f, &alice_cookie, "/api/v1/admin/host-keys", shared("TEAM", "https://api.team.example", &json!("all")), "m").await?;
        assert_eq!(status, 403);
        let (status, everyone) = post(f, &admin, "/api/v1/admin/host-keys", shared("TEAM_API", "https://api.team.example", &json!("all")), "s1").await?;
        assert_eq!((status, &everyone["shared"], &everyone["users"]), (201, &json!(true), &json!("all")), "{everyone}");
        let (status, chosen) = post(f, &admin, "/api/v1/admin/host-keys", shared("OPS_API", "https://ops.team.example", &json!([alice.id])), "s2").await?;
        assert_eq!(status, 201, "{chosen}");
        assert_eq!(chosen["users"], json!([{"id": alice.id, "email": "alice@example.invalid"}]));
        // A member sees the shared keys they may use, without who may use them.
        let names = |listed: &Value| -> Vec<String> {
            listed["items"].as_array().into_iter().flatten().filter_map(|item| item["name"].as_str().map(str::to_owned)).collect()
        };
        let (_, alices) = call(f, Method::GET, "/api/v1/host-keys/shared", &alice_cookie, (None, None)).await?;
        assert_eq!(names(&alices), ["OPS_API", "TEAM_API"]);
        assert!(alices["items"].as_array().into_iter().flatten().all(|item| item["users"].is_null()), "{alices}");
        let (_, bobs) = call(f, Method::GET, "/api/v1/host-keys/shared", &bob_cookie, (None, None)).await?;
        assert_eq!(names(&bobs), ["TEAM_API"]);
        // A member's last use of a shared key is their own, never another's.
        let conversation = f.conversation(alice.id).await?;
        used(f, everyone["id"].as_str().ok_or("id")?, alice.id, conversation.id.as_str()).await?;
        let (_, bobs) = call(f, Method::GET, "/api/v1/host-keys/shared", &bob_cookie, (None, None)).await?;
        assert_eq!(bobs["items"][0]["last_use"], Value::Null, "{bobs}");
        let (_, alices) = call(f, Method::GET, "/api/v1/host-keys/shared", &alice_cookie, (None, None)).await?;
        let team = alices["items"].as_array().into_iter().flatten().find(|item| item["name"] == "TEAM_API").ok_or("team")?;
        assert_eq!((&team["last_use"]["count"], &team["last_use"]["outcome"]), (&json!(3), &json!("ok")), "{team}");
        // Reads of a shared key: a member sees only their own rows, never whose; an administrator sees every row and whose.
        let team_path = format!("/api/v1/host-keys/{}/reads", everyone["id"].as_str().ok_or("id")?);
        let (status, bobs_reads) = call(f, Method::GET, &team_path, &bob_cookie, (None, None)).await?;
        assert_eq!((status, &bobs_reads["items"]), (200, &json!([])), "{bobs_reads}");
        let (_, alices_reads) = call(f, Method::GET, &team_path, &alice_cookie, (None, None)).await?;
        assert_eq!(alices_reads["items"].as_array().map(Vec::len), Some(1), "{alices_reads}");
        assert!(alices_reads["items"][0].get("user_id").is_none());
        let admin_path = format!("/api/v1/admin/host-keys/{}/reads", everyone["id"].as_str().ok_or("id")?);
        let (status, all_reads) = call(f, Method::GET, &admin_path, &admin, (None, None)).await?;
        assert_eq!((status, &all_reads["items"][0]["user_id"]), (200, &json!(alice.id)), "{all_reads}");
        // A shared key a member may not use has no reads for them.
        let (status, _) = call(f, Method::GET, &format!("/api/v1/host-keys/{}/reads", chosen["id"].as_str().ok_or("id")?), &bob_cookie, (None, None)).await?;
        assert_eq!(status, 404);

        // The administrator's list: shared keys with who may use them; changing who may use one.
        let (_, listed) = call(f, Method::GET, "/api/v1/admin/host-keys", &admin, (None, None)).await?;
        assert_eq!(listed["items"].as_array().map(Vec::len), Some(2), "{listed}");
        let path = format!("/api/v1/admin/host-keys/{}", chosen["id"].as_str().ok_or("id")?);
        let widen = json!({"version": 1, "name": "OPS_API", "address": "https://ops.team.example", "scheme": "bearer", "users": "all"});
        let (status, widened) = call(f, Method::PUT, &path, &admin, (Some(widen), Some("w1"))).await?;
        assert_eq!((status, &widened["users"], &widened["version"]), (200, &json!("all"), &json!(2)), "{widened}");
        let (_, bobs) = call(f, Method::GET, "/api/v1/host-keys/shared", &bob_cookie, (None, None)).await?;
        assert_eq!(names(&bobs), ["OPS_API", "TEAM_API"]);
        let (status, _) = call(f, Method::DELETE, &format!("{path}?version=2"), &admin, (None, Some("d1"))).await?;
        assert_eq!(status, 204);
        Ok(())
    })
    .await
}

// TEST: GW-I-163
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_163_an_administrator_sees_members_own_keys_exist_and_nothing_more(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, admin) = f.admin().await?;
        let (alice, alice_cookie) = f.member("alice@example.invalid").await?;
        let (status, own) = post(f, &alice_cookie, "/api/v1/host-keys", key("ALICE_SECRET_NAME", "https://api.alice.example/repo"), "a1").await?;
        assert_eq!(status, 201, "{own}");
        let conversation = f.conversation(alice.id).await?;
        let id = own["id"].as_str().ok_or("id")?.to_owned();
        used(f, &id, alice.id, conversation.id.as_str()).await?;
        let (status, listed) = call(f, Method::GET, "/api/v1/admin/host-keys", &admin, (None, None)).await?;
        assert_eq!(status, 200, "{listed}");
        let row = &listed["personal"][0];
        let mut fields: Vec<&str> = row.as_object().ok_or("row")?.keys().map(String::as_str).collect();
        fields.sort_unstable();
        assert_eq!(fields, ["host", "id", "last_use", "owner", "path_prefix", "port", "set_at", "version"], "{row}");
        assert_eq!(
            (&row["host"], &row["path_prefix"], &row["owner"]),
            (&json!("api.alice.example"), &json!("/repo"), &json!({"id": alice.id, "email": "alice@example.invalid"}))
        );
        // Its last use is a time and nothing else: no task, no title, no outcome.
        let use_fields: Vec<&str> = row["last_use"].as_object().ok_or("last use")?.keys().map(String::as_str).collect();
        assert_eq!(use_fields, ["at"], "{row}");
        // The member's name for the key is nowhere in the administrator's answers.
        assert!(!listed.to_string().contains("ALICE_SECRET_NAME"), "{listed}");
        let path = format!("/api/v1/admin/host-keys/{id}");
        let change = json!({"version": 1, "name": "TAKEN_OVER", "address": "https://api.alice.example", "scheme": "bearer", "users": "all"});
        let (status, refused) = call(f, Method::PUT, &path, &admin, (Some(change), Some("take"))).await?;
        assert_eq!((status, reason(&refused)), (409, "personal_key"), "{refused}");
        assert!(!refused.to_string().contains("ALICE_SECRET_NAME"));
        let (status, _) = call(f, Method::DELETE, &format!("{path}?version=1"), &admin, (None, Some("del"))).await?;
        assert_eq!(status, 204);
        let (_, mine) = call(f, Method::GET, "/api/v1/host-keys", &alice_cookie, (None, None)).await?;
        assert_eq!(mine["items"], json!([]));
        Ok(())
    })
    .await
}

// TEST: GW-I-170
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_170_a_shared_key_never_takes_a_members_own_name_and_names_no_one(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, admin) = f.admin().await?;
        let (alice, alice_cookie) = f.member("alice@example.invalid").await?;
        let (bob, bob_cookie) = f.member("bob@example.invalid").await?;
        let (status, _) = post(f, &alice_cookie, "/api/v1/host-keys", key("DEPLOY", "https://deploy.alice.example"), "a").await?;
        assert_eq!(status, 201);
        let nameless = |answer: &Value| {
            let text = answer.to_string();
            !text.contains("alice") && !text.contains(&alice.id.to_string()) && answer["error"]["details"][0].get("taken_by").is_none()
        };
        // For everyone: Alice may use it, and she already has DEPLOY.
        let (status, refused) = post(f, &admin, "/api/v1/admin/host-keys", shared("DEPLOY", "https://deploy.team.example", &json!("all")), "s1").await?;
        assert_eq!((status, reason(&refused)), (409, "name_taken_by_user"), "{refused}");
        assert_eq!(refused["error"]["code"], "name_taken_by_user");
        assert!(nameless(&refused), "{refused}");
        // For Bob only: fine, Bob has no DEPLOY.
        let (status, bobs) = post(f, &admin, "/api/v1/admin/host-keys", shared("DEPLOY", "https://deploy.team.example", &json!([bob.id])), "s2").await?;
        assert_eq!(status, 201, "{bobs}");
        // Taking Alice in: refused, naming no one.
        let path = format!("/api/v1/admin/host-keys/{}", bobs["id"].as_str().ok_or("id")?);
        let widen = json!({"version": 1, "name": "DEPLOY", "address": "https://deploy.team.example", "scheme": "bearer", "users": [bob.id, alice.id]});
        let (status, refused) = call(f, Method::PUT, &path, &admin, (Some(widen), Some("w"))).await?;
        assert_eq!((status, reason(&refused)), (409, "name_taken_by_user"), "{refused}");
        assert!(nameless(&refused), "{refused}");
        // Renaming another shared key, for everyone, to a name only Alice's own key has: refused too, naming no one. (To
        // a name another shared key has, it is `name_taken` naming that key, as for DEPLOY above.)
        let (status, _) = post(f, &alice_cookie, "/api/v1/host-keys", key("RELEASE", "https://release.alice.example"), "a2").await?;
        assert_eq!(status, 201);
        let (_, other) = post(f, &admin, "/api/v1/admin/host-keys", shared("OTHER", "https://other.team.example", &json!("all")), "s3").await?;
        let rename = json!({"version": 1, "name": "RELEASE", "address": "https://other.team.example", "scheme": "bearer", "users": "all"});
        let (status, refused) = call(f, Method::PUT, &format!("/api/v1/admin/host-keys/{}", other["id"].as_str().ok_or("id")?), &admin, (Some(rename), Some("r"))).await?;
        assert_eq!((status, reason(&refused)), (409, "name_taken_by_user"), "{refused}");
        assert!(nameless(&refused), "{refused}");
        // Bob adding his own DEPLOY now clashes with the shared one he may use: name_taken, which may name it.
        let (status, taken) = post(f, &bob_cookie, "/api/v1/host-keys", key("DEPLOY", "https://deploy.bob.example"), "b").await?;
        assert_eq!((status, reason(&taken)), (409, "name_taken"), "{taken}");
        assert_eq!(taken["error"]["details"][0]["taken_by"], json!({"id": bobs["id"], "shared": true}));
        Ok(())
    })
    .await
}
