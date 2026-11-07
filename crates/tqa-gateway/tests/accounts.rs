//! GW-I-002, GW-I-023 and GW-I-046: invitations, user administration and personal API tokens (task 063).
#[path = "support/fixture.rs"]
mod support;

use reqwest::{Method, Response};
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const MEMBER: &str = "member@example.invalid";
const MEMBER_PASSWORD: &str = "member-password-correct-horse";

/// A cookie-session request with the exact Origin, as the web sends it.
fn session(f: &Fixture, method: Method, path: &str, cookie: &str) -> reqwest::RequestBuilder {
    f.client
        .request(method, f.url(path))
        .header("origin", f.origin())
        .header("cookie", cookie)
}

/// A program's request: Bearer only, no Origin, no cookie.
fn bearer(f: &Fixture, method: Method, path: &str, token: &str) -> reqwest::RequestBuilder {
    f.client.request(method, f.url(path)).bearer_auth(token)
}

async fn error(response: Response) -> Result<(u16, String)> {
    let status = response.status().as_u16();
    let body: Value = response.json().await?;
    Ok((
        status,
        body["error"]["code"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
    ))
}

async fn invite(f: &Fixture, cookie: &str, email: &str, role: &str) -> Result<Value> {
    let response = session(f, Method::POST, "/api/v1/invitations", cookie)
        .json(&json!({"email":email,"role":role}))
        .send()
        .await?;
    assert_eq!(response.status(), 201);
    Ok(response.json().await?)
}

async fn accept(f: &Fixture, token: &str) -> Result<Response> {
    Ok(f.post(
        "/api/v1/invitations/accept",
        &json!({"token":token,"password":MEMBER_PASSWORD}),
    )
    .await?)
}

fn cookie_of(response: &Response) -> Result<String> {
    let value = response
        .headers()
        .get("set-cookie")
        .ok_or("no session cookie")?
        .to_str()?;
    Ok(value.split(';').next().ok_or("cookie")?.to_owned())
}

/// A member invited and signed in through the public accept flow; returns the member's session cookie.
async fn member(f: &Fixture, admin: &str) -> Result<String> {
    let invitation = invite(f, admin, MEMBER, "member").await?;
    let accepted = accept(f, invitation["token"].as_str().ok_or("token")?).await?;
    assert_eq!(accepted.status(), 201);
    cookie_of(&accepted)
}

async fn create_token(f: &Fixture, cookie: &str, name: &str, days: u32) -> Result<(String, Value)> {
    let response = session(f, Method::POST, "/api/v1/tokens", cookie)
        .json(&json!({"name":name,"expires_in_days":days}))
        .send()
        .await?;
    assert_eq!(response.status(), 201);
    let body: Value = response.json().await?;
    Ok((
        body["token"].as_str().ok_or("token shown once")?.to_owned(),
        body,
    ))
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_002_an_invitation_is_email_bound_single_use_and_never_valid_after_expiry_or_revocation(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (admin, cookie) = f.admin().await?;
        let invitation = invite(f, &cookie, MEMBER, "member").await?;
        let token = invitation["token"].as_str().ok_or("token")?.to_owned();
        assert!(token.len() >= 43, "256 bits of randomness");
        assert_eq!((invitation["status"].as_str(), invitation["role"].as_str()), (Some("pending"), Some("member")));
        let expires = time::OffsetDateTime::parse(invitation["expires_at"].as_str().ok_or("expiry")?, &time::format_description::well_known::Rfc3339)?;
        let days = (expires - time::OffsetDateTime::now_utc()).whole_hours();
        assert!((167..=168).contains(&days), "the default lifetime is 7 days: {days} h");
        // Only the hash is stored; the listing never shows the token.
        let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM invitations WHERE position(convert_to($1,'UTF8') in token_hash)>0 OR email::text=$1")
            .bind(&token).fetch_one(&f.pool).await?;
        assert_eq!(stored, 0);
        let listed: Value = session(f, Method::GET, "/api/v1/invitations", &cookie).send().await?.json().await?;
        assert!(!listed.to_string().contains(&token));
        // The join page may look it up; the email is fixed by the invitation.
        let lookup: Value = f.post("/api/v1/invitations/lookup", &json!({"token":token})).await?.json().await?;
        assert_eq!((lookup["email"].as_str(), lookup["role"].as_str()), (Some(MEMBER), Some("member")));
        let accepted = accept(f, &token).await?;
        assert_eq!(accepted.status(), 201);
        let member_cookie = cookie_of(&accepted)?;
        let me: Value = f.get("/api/v1/auth/me", &member_cookie).await?.json().await?;
        assert_eq!((me["email"].as_str(), me["role"].as_str()), (Some(MEMBER), Some("member")));
        // Single use: accept and lookup again are the same uniform refusal.
        assert_eq!(error(accept(f, &token).await?).await?, (400, "invitation_invalid".into()));
        assert_eq!(error(f.post("/api/v1/invitations/lookup", &json!({"token":token})).await?).await?, (400, "invitation_invalid".into()));
        // An email with an account cannot be invited again.
        let taken = session(f, Method::POST, "/api/v1/invitations", &cookie).json(&json!({"email":MEMBER,"role":"member"})).send().await?;
        assert_eq!(error(taken).await?, (409, "email_taken".into()));
        // Expired and revoked invitations are refused like unknown ones.
        let expired = invite(f, &cookie, "late@example.invalid", "member").await?;
        sqlx::query("UPDATE invitations SET expires_at=CURRENT_TIMESTAMP-INTERVAL '1 second' WHERE id=$1")
            .bind(uuid::Uuid::parse_str(expired["id"].as_str().ok_or("id")?)?).execute(&f.pool).await?;
        assert_eq!(error(accept(f, expired["token"].as_str().ok_or("token")?).await?).await?, (400, "invitation_invalid".into()));
        let revoked = invite(f, &cookie, "revoked@example.invalid", "admin").await?;
        let path = format!("/api/v1/invitations/{}", revoked["id"].as_str().ok_or("id")?);
        assert_eq!(session(f, Method::DELETE, &path, &cookie).send().await?.status(), 204);
        assert_eq!(error(session(f, Method::DELETE, &path, &cookie).send().await?).await?.0, 409);
        assert_eq!(error(accept(f, revoked["token"].as_str().ok_or("token")?).await?).await?, (400, "invitation_invalid".into()));
        assert_eq!(error(accept(f, "not-a-real-invitation").await?).await?, (400, "invitation_invalid".into()));
        let statuses: Value = session(f, Method::GET, "/api/v1/invitations", &cookie).send().await?.json().await?;
        let mut seen: Vec<&str> = statuses["items"].as_array().ok_or("items")?.iter().filter_map(|item| item["status"].as_str()).collect();
        seen.sort_unstable();
        assert_eq!(seen, ["accepted", "expired", "revoked"]);
        // Members cannot invite; the accepted user records who invited them.
        let by_member = session(f, Method::POST, "/api/v1/invitations", &member_cookie).json(&json!({"email":"x@example.invalid","role":"admin"})).send().await?;
        assert_eq!(error(by_member).await?, (403, "forbidden".into()));
        let inviter: Option<uuid::Uuid> = sqlx::query_scalar("SELECT invited_by FROM users WHERE email=$1::text::citext").bind(MEMBER).fetch_one(&f.pool).await?;
        assert_eq!(inviter, Some(admin.id));
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_046_admins_manage_roles_and_disabling_revokes_every_credential_at_once(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (admin, cookie) = f.admin().await?;
        let member_cookie = member(f, &cookie).await?;
        let (member_token, _) = create_token(f, &member_cookie, "ci", 30).await?;
        let users: Value = session(f, Method::GET, "/api/v1/users", &cookie)
            .send()
            .await?
            .json()
            .await?;
        let items = users["items"].as_array().ok_or("items")?;
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|user| user.get("password_hash").is_none()));
        let member_id = items
            .iter()
            .find(|user| user["email"] == MEMBER)
            .ok_or("member")?["id"]
            .as_str()
            .ok_or("id")?
            .to_owned();
        // Members never administer users.
        let denied = session(f, Method::GET, "/api/v1/users", &member_cookie)
            .send()
            .await?;
        assert_eq!(error(denied).await?, (403, "forbidden".into()));
        // The last enabled admin can neither be demoted nor disabled.
        let own = format!("/api/v1/users/{}", admin.id);
        for body in [json!({"role":"member"}), json!({"disabled":true})] {
            let refused = session(f, Method::PATCH, &own, &cookie)
                .json(&body)
                .send()
                .await?;
            assert_eq!(error(refused).await?, (409, "last_admin".into()), "{body}");
        }
        let promoted: Value = session(
            f,
            Method::PATCH,
            &format!("/api/v1/users/{member_id}"),
            &cookie,
        )
        .json(&json!({"role":"admin"}))
        .send()
        .await?
        .json()
        .await?;
        assert_eq!(promoted["role"], "admin");
        let demoted = session(
            f,
            Method::PATCH,
            &format!("/api/v1/users/{member_id}"),
            &cookie,
        )
        .json(&json!({"role":"member"}))
        .send()
        .await?;
        assert_eq!(demoted.status(), 200);
        // Disabling ends the session and the token at once; login is refused like a wrong password.
        assert_eq!(
            bearer(f, Method::GET, "/api/v1/conversations", &member_token)
                .send()
                .await?
                .status(),
            200
        );
        let disabled: Value = session(
            f,
            Method::PATCH,
            &format!("/api/v1/users/{member_id}"),
            &cookie,
        )
        .json(&json!({"disabled":true}))
        .send()
        .await?
        .json()
        .await?;
        assert_eq!(disabled["disabled"], true);
        assert_eq!(
            f.get("/api/v1/auth/me", &member_cookie).await?.status(),
            401
        );
        assert_eq!(
            bearer(f, Method::GET, "/api/v1/conversations", &member_token)
                .send()
                .await?
                .status(),
            401
        );
        let login = f
            .post(
                "/api/v1/auth/login",
                &json!({"email":MEMBER,"password":MEMBER_PASSWORD}),
            )
            .await?;
        assert_eq!(error(login).await?, (401, "unauthorized".into()));
        let enabled = session(
            f,
            Method::PATCH,
            &format!("/api/v1/users/{member_id}"),
            &cookie,
        )
        .json(&json!({"disabled":false}))
        .send()
        .await?;
        assert_eq!(enabled.status(), 200);
        assert_eq!(
            f.post(
                "/api/v1/auth/login",
                &json!({"email":MEMBER,"password":MEMBER_PASSWORD})
            )
            .await?
            .status(),
            200
        );
        assert_eq!(
            bearer(f, Method::GET, "/api/v1/conversations", &member_token)
                .send()
                .await?
                .status(),
            401,
            "revoked tokens stay revoked"
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_023_a_personal_token_is_shown_once_stored_hashed_and_refused_after_revocation_or_expiry(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let member_cookie = member(f, &cookie).await?;
        let (token, created) = create_token(f, &member_cookie, "laptop", 90).await?;
        assert!(token.starts_with("tqa_pat_") && token.len() == "tqa_pat_".len() + 43, "{}", token.len());
        assert_eq!(created["hint"].as_str(), Some(&token[token.len() - 4..]));
        let expires = time::OffsetDateTime::parse(created["expires_at"].as_str().ok_or("expiry")?, &time::format_description::well_known::Rfc3339)?;
        assert_eq!((expires - time::OffsetDateTime::now_utc()).whole_days(), 89);
        // Never listed again, never stored in the clear.
        let listed: Value = session(f, Method::GET, "/api/v1/tokens", &member_cookie).send().await?.json().await?;
        assert!(!listed.to_string().contains(&token));
        assert_eq!(listed["items"][0]["name"], "laptop");
        let clear: i64 = sqlx::query_scalar("SELECT count(*) FROM api_tokens WHERE position(convert_to($1,'UTF8') in token_hash)>0").bind(&token).fetch_one(&f.pool).await?;
        assert_eq!(clear, 0);
        let hash: Vec<u8> = sqlx::query_scalar("SELECT token_hash FROM api_tokens").fetch_one(&f.pool).await?;
        assert_eq!(hash.len(), 32);
        // Expiry is required and bounded; names are unique per user.
        for body in [json!({"name":"x"}), json!({"name":"x","expires_in_days":0}), json!({"name":"x","expires_in_days":366}), json!({"name":"laptop","expires_in_days":1})] {
            let refused = session(f, Method::POST, "/api/v1/tokens", &member_cookie).json(&body).send().await?;
            assert!(matches!(error(refused).await?.0, 400 | 409), "{body}");
        }
        // Works as the user; last use is recorded.
        let me: Value = bearer(f, Method::GET, "/api/v1/auth/me", &token).send().await?.json().await?;
        assert_eq!(me["email"], MEMBER);
        let used: Option<time::OffsetDateTime> = sqlx::query_scalar("SELECT last_used_at FROM api_tokens").fetch_one(&f.pool).await?;
        assert!(used.is_some());
        // Revoked: refused at once. Another user cannot revoke it.
        let id = created["id"].as_str().ok_or("id")?;
        assert_eq!(session(f, Method::DELETE, &format!("/api/v1/tokens/{id}"), &cookie).send().await?.status(), 404);
        assert_eq!(session(f, Method::DELETE, &format!("/api/v1/tokens/{id}"), &member_cookie).send().await?.status(), 204);
        assert_eq!(bearer(f, Method::GET, "/api/v1/auth/me", &token).send().await?.status(), 401);
        // Expired: refused.
        let (late, _) = create_token(f, &member_cookie, "short", 1).await?;
        sqlx::query("UPDATE api_tokens SET expires_at=CURRENT_TIMESTAMP-INTERVAL '1 second' WHERE name='short'").execute(&f.pool).await?;
        assert_eq!(bearer(f, Method::GET, "/api/v1/auth/me", &late).send().await?.status(), 401);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_046_bearer_rules_are_separate_from_cookies_and_administration_needs_a_session(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (user, cookie) = f.admin().await?;
        let (token, _) = create_token(f, &cookie, "program", 30).await?;
        // A program needs no Origin; a cookie still does.
        let with_token = bearer(f, Method::POST, "/api/v1/conversations", &token)
            .header("idempotency-key", "program-create")
            .json(&json!({"title":"From a program","model_route":"local"}))
            .send()
            .await?;
        assert_eq!(with_token.status(), 201);
        let cookie_without_origin = f
            .client
            .post(f.url("/api/v1/conversations"))
            .header("cookie", &cookie)
            .header("idempotency-key", "cookie-create")
            .json(&json!({"title":"x","model_route":"local"}))
            .send()
            .await?;
        assert_eq!(error(cookie_without_origin).await?.0, 403);
        // A bad token is refused even next to a valid cookie: no fallback.
        let mixed = f
            .client
            .get(f.url("/api/v1/auth/me"))
            .bearer_auth("tqa_pat_wrong")
            .header("cookie", &cookie)
            .send()
            .await?;
        assert_eq!(error(mixed).await?, (401, "unauthorized".into()));
        // Token and user administration need a session.
        for (method, path, body) in [
            (Method::GET, "/api/v1/tokens".to_owned(), None),
            (
                Method::POST,
                "/api/v1/tokens".to_owned(),
                Some(json!({"name":"more","expires_in_days":30})),
            ),
            (Method::GET, "/api/v1/users".to_owned(), None),
            (
                Method::PATCH,
                format!("/api/v1/users/{}", user.id),
                Some(json!({"role":"admin"})),
            ),
            (Method::GET, "/api/v1/invitations".to_owned(), None),
            (
                Method::POST,
                "/api/v1/invitations".to_owned(),
                Some(json!({"email":"y@example.invalid","role":"member"})),
            ),
        ] {
            let request = bearer(f, method.clone(), &path, &token);
            let request = if let Some(body) = body {
                request.json(&body)
            } else {
                request
            };
            assert_eq!(
                error(request.send().await?).await?,
                (403, "session_required".into()),
                "{method} {path}"
            );
        }
        // Personal and runtime tokens never cross.
        let conversation = f.conversation(user.id).await?;
        let runtime = f
            .gateway
            .runtime_tokens()
            .mint(
                &conversation.id,
                &"sandbox-x".parse()?,
                std::net::Ipv4Addr::LOCALHOST.into(),
            )
            .await?;
        assert_eq!(
            bearer(f, Method::GET, "/api/v1/auth/me", runtime.expose_secret())
                .send()
                .await?
                .status(),
            401
        );
        assert_eq!(
            bearer(f, Method::GET, "/runtime/history?after=0&limit=1", &token)
                .send()
                .await?
                .status(),
            401
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_046_the_31st_failed_token_authentication_in_a_minute_is_429(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let (token, _) = create_token(f, &cookie, "valid", 30).await?;
        for attempt in 1..=30 {
            let status = bearer(
                f,
                Method::GET,
                "/api/v1/auth/me",
                &format!("tqa_pat_wrong{attempt}"),
            )
            .send()
            .await?
            .status();
            assert_eq!(status, 401, "attempt {attempt}");
        }
        let limited = bearer(f, Method::GET, "/api/v1/auth/me", "tqa_pat_wrong31")
            .send()
            .await?;
        assert_eq!(error(limited).await?, (429, "busy".into()));
        // Successes do not count, but once limited the address is refused for the window, even with a valid token
        // (like the login lockout), so a guesser learns nothing.
        assert_eq!(
            bearer(f, Method::GET, "/api/v1/auth/me", &token)
                .send()
                .await?
                .status(),
            429
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_046_admins_see_other_users_task_metadata_but_never_titles(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let member_cookie = member(f, &cookie).await?;
        let created = session(f, Method::POST, "/api/v1/conversations", &member_cookie)
            .header("idempotency-key", "member-task")
            .json(&json!({"title":"Private plans","model_route":"local"}))
            .send()
            .await?;
        assert_eq!(created.status(), 201);
        let id = created.json::<Value>().await?["id"]
            .as_str()
            .ok_or("id")?
            .to_owned();
        let all: Value = f
            .get("/api/v1/conversations?all=true", &cookie)
            .await?
            .json()
            .await?;
        let item = all["items"]
            .as_array()
            .ok_or("items")?
            .iter()
            .find(|item| item["id"] == id.as_str())
            .ok_or("listed")?
            .clone();
        assert!(item.get("title").is_none_or(Value::is_null), "{item}");
        assert_eq!(item["state"], "active");
        let one: Value = f
            .get(&format!("/api/v1/conversations/{id}"), &cookie)
            .await?
            .json()
            .await?;
        assert!(one.get("title").is_none_or(Value::is_null), "{one}");
        assert!(
            !all.to_string().contains("Private plans")
                && !one.to_string().contains("Private plans")
        );
        // Content stays owner-only.
        assert_eq!(
            f.get(
                &format!("/api/v1/conversations/{id}/events?after=0&limit=10"),
                &cookie
            )
            .await?
            .status(),
            403
        );
        let mine: Value = f
            .get(&format!("/api/v1/conversations/{id}"), &member_cookie)
            .await?
            .json()
            .await?;
        assert_eq!(mine["title"], "Private plans");
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_046_accounts_show_when_they_were_last_active_at_most_once_a_minute(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let member_cookie = member(f, &cookie).await?;
        // An authenticated request records activity.
        assert_eq!(f.get("/api/v1/auth/me", &member_cookie).await?.status(), 200);
        let first = member_last_active(f, &cookie).await?;
        assert!(first.is_string(), "{first}");
        // Within the minute, further requests do not write again.
        assert_eq!(f.get("/api/v1/auth/me", &member_cookie).await?.status(), 200);
        assert_eq!(member_last_active(f, &cookie).await?, first);
        // A minute later they do.
        sqlx::query("UPDATE users SET last_active_at=last_active_at-INTERVAL '2 minutes' WHERE email=$1::text::citext")
            .bind(MEMBER).execute(&f.pool).await?;
        let moved_back = member_last_active(f, &cookie).await?;
        assert_eq!(f.get("/api/v1/auth/me", &member_cookie).await?.status(), 200);
        let later = member_last_active(f, &cookie).await?;
        assert_ne!(later, moved_back);
        Ok(())
    })
    .await
}

/// The member's `last_active_at` in the administrator's user list.
async fn member_last_active(f: &Fixture, admin: &str) -> Result<Value> {
    let users: Value = session(f, Method::GET, "/api/v1/users", admin)
        .send()
        .await?
        .json()
        .await?;
    let member = users["items"]
        .as_array()
        .ok_or("items")?
        .iter()
        .find(|user| user["email"] == MEMBER)
        .cloned()
        .ok_or("member")?;
    Ok(member["last_active_at"].clone())
}
