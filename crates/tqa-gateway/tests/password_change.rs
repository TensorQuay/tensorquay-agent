//! GW-I-124: a signed-in user changes their own password. It needs the current password and a new one that meets the
//! rules the first administrator's password met; the caller's session is kept and their other sessions end; the
//! change is audited without a value; attempts count against the login rate limit of the caller's address. A personal
//! API token can't change a password, and tokens are not revoked by the change.
#[path = "support/fixture.rs"]
mod support;

use reqwest::{Method, Response};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const PATH: &str = "/api/v1/auth/password";
const ADMIN: &str = "admin@example.invalid";
const NEW_PASSWORD: &str = "a-new-synthetic-passphrase-for-the-change";

fn session(f: &Fixture, method: Method, path: &str, cookie: &str) -> reqwest::RequestBuilder {
    f.client
        .request(method, f.url(path))
        .header("origin", f.origin())
        .header("cookie", cookie)
}

async fn change(f: &Fixture, cookie: &str, current: &str, new: &str) -> Result<Response> {
    Ok(session(f, Method::POST, PATH, cookie)
        .json(&json!({"current_password": current, "new_password": new}))
        .send()
        .await?)
}

/// The status and the first field refusal's `(field, reason)`, if any.
async fn refusal(response: Response) -> Result<(u16, String, String)> {
    let status = response.status().as_u16();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    let detail = &body["error"]["details"][0];
    Ok((
        status,
        detail["field"].as_str().unwrap_or_default().to_owned(),
        detail["reason"].as_str().unwrap_or_default().to_owned(),
    ))
}

async fn login(f: &Fixture, password: &str) -> Result<Response> {
    Ok(f.post(
        "/api/v1/auth/login",
        &json!({"email": ADMIN, "password": password}),
    )
    .await?)
}

fn cookie_of(response: &Response) -> Result<String> {
    Ok(response
        .headers()
        .get("set-cookie")
        .ok_or("no session cookie")?
        .to_str()?
        .split(';')
        .next()
        .ok_or("cookie")?
        .to_owned())
}

// TEST: GW-I-124
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_124_a_user_changes_their_own_password(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (admin, cookie) = f.admin().await?;
        let old = f.password().to_owned();
        let other = cookie_of(&login(f, &old).await?)?;
        let token: Value = session(f, Method::POST, "/api/v1/tokens", &cookie)
            .json(&json!({"name": "ci", "expires_in_days": 30}))
            .send()
            .await?
            .json()
            .await?;
        let token = token["token"].as_str().ok_or("token")?.to_owned();

        // A personal token can't change a password, and the body is closed.
        let by_token = f
            .client
            .post(f.url(PATH))
            .bearer_auth(&token)
            .json(&json!({"current_password": old, "new_password": NEW_PASSWORD}))
            .send()
            .await?;
        assert_eq!(by_token.status(), 403);
        let unknown = session(f, Method::POST, PATH, &cookie)
            .json(&json!({"current_password": old, "new_password": NEW_PASSWORD, "email": "x@example.invalid"}))
            .send()
            .await?;
        assert_eq!(unknown.status(), 400);

        // Refusals name the field, and none of them changes anything.
        let cases = [
            ("not-the-current-password", NEW_PASSWORD, "current_password", "password_incorrect"),
            (old.as_str(), "password1234", "new_password", "password_weak"),
            (old.as_str(), "short-one", "new_password", "password_weak"),
            (old.as_str(), "prefix-admin@example.invalid-suffix", "new_password", "password_weak"),
            (old.as_str(), old.as_str(), "new_password", "password_unchanged"),
        ];
        for (current, new, field, reason) in cases {
            let refused = refusal(change(f, &cookie, current, new).await?).await?;
            assert_eq!(refused, (400, field.to_owned(), reason.to_owned()), "{new}");
        }
        assert_eq!(f.get("/api/v1/auth/me", &other).await?.status(), 200, "a refusal ends no session");

        // The change: this session stays, the other one ends, the token keeps working.
        let changed = change(f, &cookie, &old, NEW_PASSWORD).await?;
        assert_eq!(changed.status(), 204);
        assert!(changed.headers().get("set-cookie").is_none(), "the session cookie is kept as it is");
        assert_eq!(f.get("/api/v1/auth/me", &cookie).await?.status(), 200);
        assert_eq!(f.get("/api/v1/auth/me", &other).await?.status(), 401);
        let with_token = f.client.get(f.url("/api/v1/auth/me")).bearer_auth(&token).send().await?;
        assert_eq!(with_token.status(), 200);
        assert_eq!(login(f, &old).await?.status(), 401);
        assert_eq!(login(f, NEW_PASSWORD).await?.status(), 200);

        // One audit row, by the user, without a value.
        let rows = sqlx::query!(
            "SELECT actor_kind, actor_user_id, area, item, action, before_version, after_version FROM admin_audit WHERE area='account'"
        )
        .fetch_all(&f.pool)
        .await?;
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(
            (row.actor_kind.as_str(), row.actor_user_id, row.area.as_str(), row.item.as_str(), row.action.as_str()),
            ("user", Some(admin.id), "account", "password", "replace")
        );
        assert_eq!((row.before_version, row.after_version), (None, None));

        // Attempts share the login limit of the address: once it's spent, even the right password waits, and so
        // does a login.
        let mut limited = false;
        for _ in 0..12 {
            limited |= change(f, &cookie, "not-the-current-password", "another-new-passphrase-here").await?.status() == 429;
        }
        assert!(limited, "attempts are rate-limited");
        assert_eq!(change(f, &cookie, NEW_PASSWORD, "another-new-passphrase-here").await?.status(), 429);
        assert_eq!(login(f, NEW_PASSWORD).await?.status(), 429);
        Ok(())
    })
    .await
}
