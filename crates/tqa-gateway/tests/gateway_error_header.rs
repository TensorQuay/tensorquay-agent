//! GW-I-148 (the gateway-refusal contract): every refusal the gateway makes itself on the model relay names its code in
//! `X-TQA-Gateway-Error`, equal to `error.code`, so the runtime can tell it from a provider's error; an answer forwarded
//! from the model server never carries the header, even when the server sends one. The engine relay and the MCP proxy
//! are held to the same rule by their suites' helpers (`support::gateway_refusal`).
#[path = "support/fixture.rs"]
mod support;
use axum::{
    Router,
    http::{HeaderValue, StatusCode},
    response::IntoResponse,
};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::TestResult;

/// A model server that answers every request with an error that looks like the gateway's own.
struct Impostor {
    base: String,
    address: std::net::SocketAddr,
    server: tokio::task::JoinHandle<()>,
}

impl Impostor {
    async fn start() -> std::io::Result<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let app = Router::new().fallback(|| async {
            (
                StatusCode::BAD_REQUEST,
                [
                    ("x-tqa-gateway-error", "key_use_unrecorded"),
                    ("content-type", "application/json"),
                    // No pooled connection outlives the server, so a stopped one is unreachable at once.
                    ("connection", "close"),
                ],
                r#"{"error":{"code":"key_use_unrecorded","message":"from the model server"}}"#,
            )
                .into_response()
        });
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Ok(Self {
            base: format!("http://{address}"),
            address,
            server,
        })
    }

    /// Stops the server and waits until its port refuses connections.
    async fn stop(&self) -> TestResult {
        self.server.abort();
        let deadline = tokio::time::Instant::now() + support::DEADLINE;
        while tokio::net::TcpStream::connect(self.address).await.is_ok() {
            if tokio::time::Instant::now() > deadline {
                return Err("the model server did not stop".into());
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        Ok(())
    }
}

impl Drop for Impostor {
    fn drop(&mut self) {
        self.server.abort();
    }
}

fn registry(base: &str) -> Result<tqa_llm::Registry, Box<dyn std::error::Error>> {
    Ok(tqa_llm::Registry::from_toml(&format!(
        "[[routes]]\nname='local'\nprotocol='openai_chat'\nbase_url='{base}'\nmodel='pinned-model'\nauth={{kind='bearer'}}\nkey_ref='secrets/local'\nreasoning={{field='thinking_mode',allowed=['low','high'],default='low'}}\n\n[[routes]]\nname='other'\nprotocol='openai_chat'\nbase_url='{base}'\nmodel='other-model'\nauth={{kind='none'}}\n"
    ))?)
}

async fn token(fixture: &support::Fixture) -> Result<SecretString, Box<dyn std::error::Error>> {
    let (owner, _) = fixture.admin().await?;
    let conversation = fixture.conversation(owner.id).await?;
    Ok(fixture
        .gateway
        .runtime_tokens()
        .mint(
            &conversation.id,
            &format!("header-{}", uuid::Uuid::now_v7()).parse()?,
            std::net::Ipv4Addr::LOCALHOST.into(),
        )
        .await?)
}

fn chat(
    fixture: &support::Fixture,
    path: &str,
    token: Option<&SecretString>,
) -> reqwest::RequestBuilder {
    let request = fixture
        .client
        .post(fixture.url(path))
        .json(&json!({"model": "any", "messages": [{"role": "user", "content": "hi"}]}));
    match token {
        Some(token) => request.bearer_auth(token.expose_secret()),
        None => request,
    }
}

/// Sends the request and checks the answer is the gateway's own refusal `code` at `status`, named in the header.
async fn refused(request: reqwest::RequestBuilder, status: u16, code: &str) -> TestResult {
    let response = request.send().await?;
    let seen = response.status().as_u16();
    let header = response.headers().get("x-tqa-gateway-error").cloned();
    let body: Value = response.json().await?;
    assert_eq!(
        (seen, body["error"]["code"].as_str()),
        (status, Some(code)),
        "{body}"
    );
    assert_eq!(header, Some(HeaderValue::from_str(code)?), "{code}");
    Ok(())
}

/// Sends the request and checks the model server's error came through as it was, without the header it forged.
async fn forwarded(request: reqwest::RequestBuilder) -> TestResult {
    let response = request.send().await?;
    assert_eq!(response.status(), 400);
    assert_eq!(response.headers().get("x-tqa-gateway-error"), None);
    let body: Value = response.json().await?;
    assert_eq!(body["error"]["message"], "from the model server");
    Ok(())
}

// TEST: GW-I-148
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_148_the_model_relays_own_refusals_name_their_code(pool: PgPool) -> TestResult {
    let impostor = Impostor::start().await?;
    let config = "[relay]\nbody_bytes=4096\n";
    support::run_configured(pool, Some(registry(&impostor.base)?), config, async |f| {
        let token = token(f).await?;
        let path = "/relay/r/local/chat/completions";
        refused(chat(f, path, None), 401, "unauthorized").await?;
        refused(
            chat(f, "/relay/r/other/chat/completions", Some(&token)),
            403,
            "forbidden",
        )
        .await?;
        // The route speaks Chat, not Messages.
        refused(
            chat(f, "/relay/r/local/messages", Some(&token)),
            404,
            "not_found",
        )
        .await?;
        let invalid = chat(f, path, Some(&token))
            .json(&json!({"model": "any", "messages": [], "reasoning_effort": "invalid"}));
        refused(invalid, 400, "invalid_request").await?;
        let big =
            json!({"model": "any", "messages": [{"role": "user", "content": "x".repeat(8192)}]});
        refused(
            chat(f, path, Some(&token)).json(&big),
            413,
            "body_too_large",
        )
        .await?;
        refused(chat(f, path, Some(&token)), 503, "secret_unavailable").await?;
        f.gateway
            .secret_store()
            .put(
                &"secrets/local".parse()?,
                SecretString::from("synthetic-provider"),
            )
            .await?;
        forwarded(chat(f, path, Some(&token))).await?;
        forwarded(
            f.client
                .get(f.url("/relay/r/local/models"))
                .bearer_auth(token.expose_secret()),
        )
        .await?;
        sqlx::query("ALTER TABLE secret_reads RENAME TO secret_reads_away")
            .execute(&f.pool)
            .await?;
        let unrecorded = refused(chat(f, path, Some(&token)), 503, "key_use_unrecorded").await;
        sqlx::query("ALTER TABLE secret_reads_away RENAME TO secret_reads")
            .execute(&f.pool)
            .await?;
        unrecorded?;
        impostor.stop().await?;
        refused(chat(f, path, Some(&token)), 502, "upstream_unavailable").await
    })
    .await
}

// TEST: GW-I-148
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_148_the_relays_rate_refusal_names_its_code(pool: PgPool) -> TestResult {
    let impostor = Impostor::start().await?;
    let config = "[relay]\ncapacity=1\nrefill_per_second=0.001\n";
    support::run_configured(pool, Some(registry(&impostor.base)?), config, async |f| {
        let token = token(f).await?;
        f.gateway
            .secret_store()
            .put(
                &"secrets/local".parse()?,
                SecretString::from("synthetic-provider"),
            )
            .await?;
        let path = "/relay/r/local/chat/completions";
        forwarded(chat(f, path, Some(&token))).await?;
        refused(chat(f, path, Some(&token)), 429, "rate_limited").await
    })
    .await
}
