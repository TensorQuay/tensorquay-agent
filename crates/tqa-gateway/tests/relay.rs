//! GW-I-039: authenticated transparent forwarding, route policy and per-request metering.

#[path = "support/fixture.rs"]
mod support;
use secrecy::ExposeSecret;
use serde_json::json;
use sqlx::PgPool;
use std::sync::Arc;
use support::TestResult;
use tqa_testkit::{FakeModelServer, Script, Scripted};

fn registry(server: &FakeModelServer) -> Result<tqa_llm::Registry, Box<dyn std::error::Error>> {
    Ok(tqa_llm::Registry::from_toml(&format!(
        "[[routes]]\nname='local'\nprotocol='openai_chat'\nbase_url='{}'\nmodel='pinned-model'\nauth={{kind='bearer'}}\nkey_ref='secrets/local'\nstream_usage=true\nextra_headers={{x-route-setting='enabled'}}\nextra_body={{extension={{enabled=true}}}}\nreasoning={{field='thinking_mode',allowed=['low','high'],default='low'}}",
        server.base_url()
    ))?)
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_039_relay_shapes_authorized_requests_preserves_bytes_and_records_usage(
    pool: PgPool,
) -> TestResult {
    let usage = tqa_llm::Usage {
        prompt_tokens: 12,
        completion_tokens: 7,
        cached_tokens: Some(4),
        reasoning_tokens: Some(2),
    };
    let script = Script::new(vec![
        Scripted::Text("héllo\nworld".into()),
        Scripted::Usage(usage),
        Scripted::MalformedSse(tqa_testkit::MalformedSse::SplitUtf8),
    ]);
    let server = FakeModelServer::start(
        vec![script.clone(), script],
        Arc::new(tqa_llm::TokioSleeper),
    )
    .await?;
    let result = support::run_configured(pool, Some(registry(&server)?), "", async |fixture| {
        let (owner,_) = fixture.admin().await?;
        let conversation = fixture.conversation(owner.id).await?;
        // The run the call names is the task's own, so the usage row records it (GW-I-151).
        fixture.gateway.events().append(&conversation.id, support::started(1, "relay-run")?).await?;
        let token = fixture.gateway.runtime_tokens().mint(&conversation.id, &"relay-sandbox".parse()?, std::net::Ipv4Addr::LOCALHOST.into()).await?;
        let secret = secrecy::SecretString::from(format!("synthetic-provider-{}", uuid::Uuid::now_v7()));
        fixture.gateway.secret_store().put(&"secrets/local".parse()?, secret.clone()).await?;
        let input = json!({"model":"untrusted-model","messages":[{"role":"user","content":"hi","opaque_signature":"keep"}],"stream":true,"reasoning_effort":"high","extension":{"enabled":false},"unknown":[1,2],"stream_options":{"include_usage":false}});
        let response = fixture.client.post(fixture.url("/relay/r/local/chat/completions")).bearer_auth(token.expose_secret()).header("X-TQA-Run-ID", "relay-run").json(&input).send().await?;
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers().get("content-type").ok_or("missing content type")?, "text/event-stream");
        let bytes = response.bytes().await?;
        let direct = fixture.client.post(format!("{}/chat/completions", server.base_url())).json(&input).send().await?.bytes().await?;
        assert_eq!(bytes, direct);
        let requests = server.requests();
        let upstream = requests.first().ok_or("missing upstream request")?;
        assert_eq!(upstream.body["model"], "pinned-model");
        assert_eq!(upstream.body["thinking_mode"], "high");
        assert_eq!(upstream.body["extension"], json!({"enabled":true}));
        assert_eq!(upstream.body["messages"], input["messages"]);
        assert_eq!(upstream.body["unknown"], input["unknown"]);
        assert_eq!(upstream.body["stream_options"]["include_usage"], true);
        assert!(upstream.body.get("reasoning_effort").is_none());
        assert!(upstream.headers.contains_key("authorization"));
        assert!(upstream.headers.contains_key("x-route-setting"));
        assert!(!String::from_utf8_lossy(&bytes).contains(secret.expose_secret()));
        assert!(!String::from_utf8_lossy(&upstream.encoded_body).contains(token.expose_secret()));
        let row = wait_usage(&fixture.pool).await?;
        assert_eq!(row, (Some(12),Some(7),Some(4),Some(2)));
        let row = sqlx::query!("SELECT route,run_id,conversation_id,user_id,(SELECT owner_id FROM conversations c WHERE c.id=u.conversation_id) AS owner FROM usage u").fetch_one(&fixture.pool).await?;
        assert_eq!(row.route, "local");
        assert_eq!(row.run_id.as_deref(), Some("relay-run"));
        assert_eq!(row.conversation_id.map(|id| id.to_string()).as_deref(), Some(conversation.id.as_str()));
        // Counted for the task's owner from the start, so the count outlives the task's purge.
        assert!(row.user_id.is_some() && row.user_id == row.owner);
        Ok(())
    }).await;
    server.shutdown().await?;
    result
}

type Counters = (Option<i64>, Option<i64>, Option<i64>, Option<i64>);
async fn wait_usage(pool: &PgPool) -> Result<Counters, Box<dyn std::error::Error>> {
    tokio::time::timeout(support::DEADLINE, async {
        loop {
            let row = sqlx::query!(
                "SELECT prompt_tokens,completion_tokens,cached_tokens,reasoning_tokens FROM usage"
            )
            .fetch_one(pool)
            .await?;
            if row.prompt_tokens.is_some() {
                return Ok((
                    row.prompt_tokens,
                    row.completion_tokens,
                    row.cached_tokens,
                    row.reasoning_tokens,
                ));
            }
            tokio::task::yield_now().await;
        }
    })
    .await?
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_039_models_errors_cuts_and_unapproved_paths_remain_transparent(
    pool: PgPool,
) -> TestResult {
    let cut = Script::new(vec![
        Scripted::Text("partial".into()),
        Scripted::CutAfter(1),
    ]);
    let server = FakeModelServer::start(
        vec![
            Script::new(vec![Scripted::Status {
                code: 503,
                body: "upstream raw error\n".into(),
                retry_after: Some("7".into()),
            }]),
            cut.clone(),
            cut,
        ],
        Arc::new(tqa_llm::TokioSleeper),
    )
    .await?;
    let result = support::run_configured(pool, Some(registry(&server)?), "", async |fixture| {
        let (conversation, token) = caller(fixture).await?;
        let models = fixture
            .client
            .get(fixture.url("/relay/r/local/models"))
            .bearer_auth(token.expose_secret())
            .send()
            .await?;
        assert_eq!(models.status(), 200);
        assert_eq!(
            models.json::<serde_json::Value>().await?,
            json!({"data":[{"id":"fake-model"}]})
        );
        let response = completion(fixture, &token).send().await?;
        assert_eq!(response.status(), 503);
        // An upstream refusal is relayed as it is, never as the relay's own throttle (GW-U-033).
        assert_eq!(response.headers().get("tqa-relay-throttle"), None);
        assert_eq!(
            response
                .headers()
                .get("retry-after")
                .ok_or("missing retry header")?,
            "7"
        );
        assert_eq!(response.bytes().await?.as_ref(), b"upstream raw error\n");
        let cut = completion(fixture, &token).send().await?.bytes().await?;
        let direct = fixture
            .client
            .post(format!("{}/chat/completions", server.base_url()))
            .json(&json!({}))
            .send()
            .await?
            .bytes()
            .await?;
        assert_eq!(cut, direct);
        assert!(!String::from_utf8_lossy(&cut).contains("[DONE]"));
        for path in ["files", "completions", "chat/completions/extra"] {
            assert_eq!(
                fixture
                    .client
                    .post(fixture.url(&format!("/relay/r/local/{path}")))
                    .bearer_auth(token.expose_secret())
                    .json(&json!({}))
                    .send()
                    .await?
                    .status(),
                404
            );
        }
        // Two metered requests: the error's row stays unreported (nothing streamed); the cut stream's holds the
        // relay's estimate of what streamed ("partial", 2 tokens), marked (GW-I-275). The meter writes after the
        // stream ends, so the estimate is awaited.
        let rows = tokio::time::timeout(support::DEADLINE, async {
            loop {
                let rows = sqlx::query!(
                    "SELECT completion_tokens,estimated,conversation_id FROM usage ORDER BY completion_tokens NULLS FIRST"
                )
                .fetch_all(&fixture.pool)
                .await?;
                if rows.iter().any(|row| row.estimated) {
                    return Ok::<_, Box<dyn std::error::Error>>(rows);
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .map_err(|_| "the cut stream's estimate was never written")??;
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| row.conversation_id.map(|id| id.to_string()).as_deref()
            == Some(conversation.id.as_str())));
        assert_eq!(
            rows.iter().map(|row| (row.completion_tokens, row.estimated)).collect::<Vec<_>>(),
            [(None, false), (Some(2), true)]
        );
        Ok(())
    })
    .await;
    server.shutdown().await?;
    result
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_039_peer_revocation_and_route_binding_guard_the_relay(pool: PgPool) -> TestResult {
    let server = FakeModelServer::start(vec![], Arc::new(tqa_llm::TokioSleeper)).await?;
    let result = support::run_configured(pool, Some(registry(&server)?), "", async |fixture| {
        let (conversation, token) = caller(fixture).await?;
        assert_eq!(
            fixture
                .client
                .post(fixture.url("/relay/r/local/chat/completions"))
                .json(&json!({}))
                .send()
                .await?
                .status(),
            401
        );
        assert_eq!(
            fixture
                .client
                .post(fixture.url("/relay/r/other/chat/completions"))
                .bearer_auth(token.expose_secret())
                .json(&json!({}))
                .send()
                .await?
                .status(),
            403
        );
        let wrong_peer = fixture
            .gateway
            .runtime_tokens()
            .mint(
                &conversation.id,
                &"wrong-peer".parse()?,
                std::net::Ipv6Addr::LOCALHOST.into(),
            )
            .await?;
        assert_eq!(
            completion(fixture, &wrong_peer)
                .header("x-forwarded-for", "::1")
                .send()
                .await?
                .status(),
            401
        );
        fixture
            .gateway
            .runtime_tokens()
            .revoke(&"wrong-peer".parse()?)
            .await?;
        assert_eq!(completion(fixture, &wrong_peer).send().await?.status(), 401);
        assert_eq!(completion(fixture, &token).send().await?.status(), 401);
        assert!(server.requests().is_empty());
        Ok(())
    })
    .await;
    server.shutdown().await?;
    result
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_039_request_rate_is_shared_by_a_users_conversations(pool: PgPool) -> TestResult {
    let server = FakeModelServer::start(
        vec![Script::new(vec![Scripted::Text("allowed".into())])],
        Arc::new(tqa_llm::TokioSleeper),
    )
    .await?;
    let result = support::run_configured(
        pool,
        Some(registry(&server)?),
        "[relay]\ncapacity=1\nrefill_per_second=0.001",
        async |fixture| {
            let (conversation, token) = caller(fixture).await?;
            assert_eq!(completion(fixture, &token).send().await?.status(), 200);
            let next = fixture.conversation(conversation.owner_id).await?;
            let token = fixture
                .gateway
                .runtime_tokens()
                .mint(
                    &next.id,
                    &"another-conversation".parse()?,
                    std::net::Ipv4Addr::LOCALHOST.into(),
                )
                .await?;
            let response = completion(fixture, &token).send().await?;
            assert_eq!(response.status(), 429);
            // GW-U-033: the user's rate refusal is marked as a user throttle.
            assert_eq!(
                response
                    .headers()
                    .get("tqa-relay-throttle")
                    .and_then(|value| value.to_str().ok()),
                Some("user")
            );
            assert!(
                response
                    .headers()
                    .get("retry-after")
                    .ok_or("missing retry delay")?
                    .to_str()?
                    .parse::<u64>()?
                    > 0
            );
            // Model discovery shares the user's rate, and its refusal is the same user throttle.
            let models = fixture
                .client
                .get(fixture.url("/relay/r/local/models"))
                .bearer_auth(token.expose_secret())
                .send()
                .await?;
            assert_eq!(models.status(), 429);
            assert_eq!(
                models
                    .headers()
                    .get("tqa-relay-throttle")
                    .and_then(|value| value.to_str().ok()),
                Some("user")
            );
            assert_eq!(server.requests().len(), 1);
            Ok(())
        },
    )
    .await;
    server.shutdown().await?;
    result
}

fn completion(
    fixture: &support::Fixture,
    token: &secrecy::SecretString,
) -> reqwest::RequestBuilder {
    fixture
        .client
        .post(fixture.url("/relay/r/local/chat/completions"))
        .bearer_auth(token.expose_secret())
        .json(&json!({"model":"untrusted-model","messages":[],"stream":true}))
}

/// A registry of three routes on the fake server: `local` falls back to `backup`; `other` is unrelated.
fn fallback_registry(
    server: &FakeModelServer,
) -> Result<tqa_llm::Registry, Box<dyn std::error::Error>> {
    let route = |name: &str, fallbacks: &str| {
        format!(
            "[[routes]]\nname='{name}'\nprotocol='openai_chat'\nbase_url='{}'\nmodel='{name}-model'\nauth={{kind='none'}}\nfallbacks=[{fallbacks}]\n",
            server.base_url()
        )
    };
    Ok(tqa_llm::Registry::from_toml(&format!(
        "{}{}{}",
        route("local", "'backup'"),
        route("backup", ""),
        route("other", "")
    ))?)
}

/// A relay caller for a conversation on `route`.
async fn caller_on(
    fixture: &support::Fixture,
    owner: uuid::Uuid,
    route: &str,
) -> Result<(tqa_gateway::Conversation, secrecy::SecretString), Box<dyn std::error::Error>> {
    let conversation = fixture
        .gateway
        .create_for_user(owner, "Fallback", route)
        .await?;
    let token = fixture
        .gateway
        .runtime_tokens()
        .mint(
            &conversation.id,
            &format!("relay-{route}").parse()?,
            std::net::Ipv4Addr::LOCALHOST.into(),
        )
        .await?;
    Ok((conversation, token))
}

fn completion_on(
    fixture: &support::Fixture,
    route: &str,
    token: &secrecy::SecretString,
) -> reqwest::RequestBuilder {
    fixture
        .client
        .post(fixture.url(&format!("/relay/r/{route}/chat/completions")))
        .bearer_auth(token.expose_secret())
        .json(&json!({"messages":[{"role":"user","content":"hello"}],"stream":true}))
}

// TEST: GW-I-039, GW-I-009
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_039_the_relay_accepts_the_routes_fallbacks_and_meters_the_route_used(
    pool: PgPool,
) -> TestResult {
    let server = FakeModelServer::start(
        vec![
            Script::new(vec![Scripted::Text("from backup".into())]),
            Script::new(vec![Scripted::Text("from local".into())]),
        ],
        Arc::new(tqa_llm::TokioSleeper),
    )
    .await?;
    let result = support::run_configured(
        pool,
        Some(fallback_registry(&server)?),
        "",
        async |fixture| {
            let (owner, _) = fixture.admin().await?;
            let (conversation, token) = caller_on(fixture, owner.id, "local").await?;
            // A declared fallback of the conversation's route is forwarded and metered under the route actually used.
            let fallback = completion_on(fixture, "backup", &token).send().await?;
            assert_eq!(fallback.status(), 200);
            fallback.bytes().await?;
            let routes: Vec<String> =
                sqlx::query_scalar("SELECT route FROM usage WHERE conversation_id=$1::uuid")
                    .bind(conversation.id.as_str())
                    .fetch_all(&fixture.pool)
                    .await?;
            assert_eq!(routes, ["backup"]);
            // A route that is not a fallback of this conversation's route is refused.
            assert_eq!(
                completion_on(fixture, "other", &token)
                    .send()
                    .await?
                    .status(),
                403
            );
            // Fallbacks are not symmetric: a conversation on `backup` may not use `local`.
            let (_, backup) = caller_on(fixture, owner.id, "backup").await?;
            assert_eq!(
                completion_on(fixture, "local", &backup)
                    .send()
                    .await?
                    .status(),
                403
            );
            // GW-I-009: a refused route reaches nothing upstream.
            assert_eq!(server.requests().len(), 1, "a refused route was forwarded");
            // A model named in the body is not a way out of the route: the route's own model is sent.
            let named = fixture
                .client
                .post(fixture.url("/relay/r/local/chat/completions"))
                .bearer_auth(token.expose_secret())
                .json(&json!({"model":"other-model","messages":[{"role":"user","content":"hello"}],"stream":true}))
                .send()
                .await?;
            assert_eq!(named.status(), 200);
            named.bytes().await?;
            let models: Vec<serde_json::Value> =
                server.requests().iter().map(|request| request.body["model"].clone()).collect();
            assert_eq!(models, [json!("backup-model"), json!("local-model")]);
            Ok(())
        },
    )
    .await;
    server.shutdown().await?;
    result
}

async fn caller(
    fixture: &support::Fixture,
) -> Result<(tqa_gateway::Conversation, secrecy::SecretString), Box<dyn std::error::Error>> {
    let (owner, _) = fixture.admin().await?;
    let conversation = fixture.conversation(owner.id).await?;
    let token = fixture
        .gateway
        .runtime_tokens()
        .mint(
            &conversation.id,
            &"relay-caller".parse()?,
            std::net::Ipv4Addr::LOCALHOST.into(),
        )
        .await?;
    fixture
        .gateway
        .secret_store()
        .put(
            &"secrets/local".parse()?,
            format!("synthetic-provider-{}", uuid::Uuid::now_v7()).into(),
        )
        .await?;
    Ok((conversation, token))
}

#[derive(Default)]
struct PendingUpstream {
    entered: tokio::sync::Notify,
    dropped: tokio::sync::Notify,
}
impl tqa_types::Sleeper for PendingUpstream {
    fn sleep(
        &self,
        _: std::time::Duration,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(async {
            let _guard = ObserveDrop(&self.dropped);
            self.entered.notify_one();
            std::future::pending::<()>().await;
        })
    }
}
struct ObserveDrop<'a>(&'a tokio::sync::Notify);
impl Drop for ObserveDrop<'_> {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_039_runtime_disconnect_drops_upstream_within_200_ms(pool: PgPool) -> TestResult {
    let sleeper = Arc::new(PendingUpstream::default());
    let server = FakeModelServer::start(
        vec![Script::new(vec![
            Scripted::Text("first".into()),
            Scripted::Delay(std::time::Duration::from_secs(30)),
            Scripted::Text("never delivered".into()),
        ])],
        sleeper.clone(),
    )
    .await?;
    let result = support::run_configured(pool, Some(registry(&server)?), "", async |fixture| {
        let (_, token) = caller(fixture).await?;
        let mut response = completion(fixture, &token).send().await?;
        assert_eq!(response.status(), 200);
        assert!(response.chunk().await?.is_some());
        tokio::time::timeout(support::DEADLINE, sleeper.entered.notified()).await?;
        drop(response);
        tokio::time::timeout(
            std::time::Duration::from_millis(200),
            sleeper.dropped.notified(),
        )
        .await?;
        // Its usage row: the server's usage never came, so the relay's estimate from what streamed ("first"), marked
        // (GW-I-275), written once the upstream is gone.
        let row = tokio::time::timeout(support::DEADLINE, async {
            loop {
                let row: Option<(Option<i64>, bool)> = sqlx::query_as(
                    "SELECT completion_tokens, estimated FROM usage WHERE completion_tokens IS NOT NULL",
                )
                .fetch_optional(&fixture.pool)
                .await?;
                if let Some(row) = row {
                    return Ok::<_, Box<dyn std::error::Error>>(row);
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .map_err(|_| "the cut-off request's usage was never written")??;
        assert_eq!(row, (Some(2), true));
        Ok(())
    })
    .await;
    server.shutdown().await?;
    result
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_039_a_configured_relay_limit_reaches_session_config_and_is_enforced(
    pool: PgPool,
) -> TestResult {
    let server = FakeModelServer::start(
        vec![Script::new(vec![Scripted::Text("accepted".into())])],
        Arc::new(tqa_llm::TokioSleeper),
    )
    .await?;
    let limit = 2 * 1024 * 1024;
    let config = format!("[relay]\nbody_bytes={limit}\n");
    let result =
        support::run_configured(pool, Some(registry(&server)?), &config, async |fixture| {
            let (conversation, _) = caller(fixture).await?;
            // The runtime learns the limit it must keep requests (screenshots included) within (071).
            let runtime = fixture.runtime(&conversation).await?;
            assert_eq!(runtime.config["route"]["max_request_bytes"], limit);
            // Its sandbox's own token (starting the sandbox replaced the manually minted one).
            let token = fixture
                .backend
                .token(&conversation.id)
                .ok_or("runtime token")?;
            let within =
                json!({"messages":[{"role":"user","content":"x".repeat(limit / 2)}],"stream":true});
            let response = completion(fixture, &token).json(&within).send().await?;
            assert_eq!(response.status(), 200);
            response.bytes().await?;
            let over = completion(fixture, &token)
                .body(vec![b' '; limit + 1])
                .send()
                .await?;
            assert_eq!(over.status(), 413);
            Ok(())
        })
        .await;
    server.shutdown().await?;
    result
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_039_relay_body_cap_is_16_mib_and_validation_never_forwards(
    pool: PgPool,
) -> TestResult {
    let server = FakeModelServer::start(
        vec![Script::new(vec![Scripted::Text("accepted".into())])],
        Arc::new(tqa_llm::TokioSleeper),
    )
    .await?;
    let result = support::run_configured(pool, Some(registry(&server)?), "", async |fixture| {
        let (_, token) = caller(fixture).await?;
        let large =
            json!({"messages":[{"role":"user","content":"x".repeat(1024*1024+1)}],"stream":true});
        let response = completion(fixture, &token).json(&large).send().await?;
        assert_eq!(response.status(), 200);
        response.bytes().await?;
        assert_eq!(
            completion(fixture, &token)
                .body(vec![b' '; 16 * 1024 * 1024 + 1])
                .send()
                .await?
                .status(),
            413
        );
        assert_eq!(
            completion(fixture, &token)
                .header("x-tqa-run-id", "invalid run")
                .send()
                .await?
                .status(),
            400
        );
        assert_eq!(
            completion(fixture, &token)
                .json(&json!({"reasoning_effort":"invalid"}))
                .send()
                .await?
                .status(),
            400
        );
        fixture
            .gateway
            .secret_store()
            .delete(&"secrets/local".parse()?)
            .await?;
        assert_eq!(completion(fixture, &token).send().await?.status(), 503);
        assert_eq!(server.requests().len(), 1);
        Ok(())
    })
    .await;
    server.shutdown().await?;
    result
}

/// GW-I-039 (GW-U-062): an event stream labelled text/plain is forwarded unchanged and still metered.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_039_a_mislabelled_stream_is_forwarded_and_metered(pool: PgPool) -> TestResult {
    let usage = tqa_llm::Usage {
        prompt_tokens: 9,
        completion_tokens: 4,
        cached_tokens: None,
        reasoning_tokens: None,
    };
    let script = Script::new(vec![
        Scripted::Text("hi".into()),
        Scripted::Usage(usage),
        Scripted::Header("content-type".into(), "text/plain; charset=utf-8".into()),
    ]);
    let server = FakeModelServer::start(vec![script], Arc::new(tqa_llm::TokioSleeper)).await?;
    let result = support::run_configured(pool, Some(registry(&server)?), "", async |fixture| {
        let (owner, _) = fixture.admin().await?;
        let conversation = fixture.conversation(owner.id).await?;
        let token = fixture
            .gateway
            .runtime_tokens()
            .mint(
                &conversation.id,
                &"relay-sandbox".parse()?,
                std::net::Ipv4Addr::LOCALHOST.into(),
            )
            .await?;
        fixture
            .gateway
            .secret_store()
            .put(
                &"secrets/local".parse()?,
                secrecy::SecretString::from("synthetic-provider-key".to_owned()),
            )
            .await?;
        let input = json!({"model":"m","messages":[{"role":"user","content":"hi"}],"stream":true});
        let response = fixture
            .client
            .post(fixture.url("/relay/r/local/chat/completions"))
            .bearer_auth(token.expose_secret())
            .json(&input)
            .send()
            .await?;
        assert_eq!(response.status(), 200);
        assert_eq!(
            response
                .headers()
                .get("content-type")
                .ok_or("missing content type")?,
            "text/plain; charset=utf-8"
        );
        let body = String::from_utf8(response.bytes().await?.to_vec())?;
        assert!(body.starts_with("data: "), "{body}");
        assert_eq!(
            wait_usage(&fixture.pool).await?,
            (Some(9), Some(4), None, None)
        );
        Ok(())
    })
    .await;
    server.shutdown().await?;
    result
}
