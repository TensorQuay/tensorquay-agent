//! GW-I-040: credentials identify one sandbox instance and never accompany conversation creation.

#[path = "support/fixture.rs"]
mod support;
use secrecy::ExposeSecret;
use sqlx::PgPool;
use support::{TestResult, run};

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_040_runtime_tokens_are_scoped_to_sandbox_instances_and_revoke_idempotently(
    pool: PgPool,
) -> TestResult {
    run(pool, async |fixture| {
        let (owner, _) = fixture.admin().await?;
        let created = fixture.conversation(owner.id).await?;
        let id = &created.id;
        let count = sqlx::query_scalar!(
            "SELECT count(*) FROM runtime_tokens WHERE conversation_id=$1",
            uuid::Uuid::parse_str(id.as_str())?
        )
        .fetch_one(&fixture.pool)
        .await?;
        assert_eq!(count, Some(0));
        let first = "sandbox-one".parse()?;
        let second = "sandbox-two".parse()?;
        let tokens = fixture.gateway.runtime_tokens();
        let peer = std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);
        let token = tokens.mint(id, &first, peer).await?;
        assert_eq!(token.expose_secret().len(), 64);
        assert_eq!(
            tokens.resolve(&token, peer).await?,
            Some((id.clone(), first.clone()))
        );
        let replacement = tokens.mint(id, &second, peer).await?;
        assert_ne!(token.expose_secret(), replacement.expose_secret());
        assert_eq!(tokens.resolve(&token, peer).await?, None);
        assert_eq!(
            tokens.resolve(&replacement, peer).await?,
            Some((id.clone(), second.clone()))
        );
        tokens.revoke(&first).await?;
        assert!(tokens.resolve(&replacement, peer).await?.is_some());
        tokens.revoke(&second).await?;
        tokens.revoke(&second).await?;
        assert!(tokens.resolve(&replacement, peer).await?.is_none());
        assert!(tokens.resolve(&"invalid".into(), peer).await?.is_none());
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_040_peer_address_binding_rejects_another_sandbox_address(pool: PgPool) -> TestResult {
    run(pool, async |fixture| {
        let (owner, _) = fixture.admin().await?;
        let created = fixture.conversation(owner.id).await?;
        let token = fixture
            .gateway
            .runtime_tokens()
            .mint(
                &created.id,
                &"sandbox-address".parse()?,
                std::net::Ipv4Addr::LOCALHOST.into(),
            )
            .await?;
        let first = fixture
            .gateway
            .runtime_tokens()
            .resolve(&token, std::net::Ipv4Addr::LOCALHOST.into())
            .await?;
        let second = fixture
            .gateway
            .runtime_tokens()
            .resolve(&token, std::net::Ipv6Addr::LOCALHOST.into())
            .await?;
        assert!(first.is_some());
        assert_eq!(second, None);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_040_revocation_closes_a_registered_link_and_prevents_redial(
    pool: PgPool,
) -> TestResult {
    run(pool, async |fixture| {
        let (owner, _) = fixture.admin().await?;
        let conversation = fixture.conversation(owner.id).await?;
        let mut runtime = fixture.runtime(&conversation).await?;
        fixture
            .gateway
            .runtime_tokens()
            .revoke(&runtime.sandbox)
            .await?;
        assert_eq!(runtime.error().await?, tqa_types::ErrorCode::Restarting);
        runtime.closed().await?;
        assert!(fixture.runtime(&conversation).await.is_err());
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_040_link_uses_the_transport_address_instead_of_forwarded_headers(
    pool: PgPool,
) -> TestResult {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    run(pool, async |fixture| {
        let (owner, _) = fixture.admin().await?;
        let conversation = fixture.conversation(owner.id).await?;
        let token = fixture
            .gateway
            .runtime_tokens()
            .mint(
                &conversation.id,
                &"wrong-address".parse()?,
                std::net::Ipv6Addr::LOCALHOST.into(),
            )
            .await?;
        let mut request = fixture
            .url("/runtime/link")
            .replacen("http:", "ws:", 1)
            .into_client_request()?;
        request.headers_mut().insert(
            "authorization",
            format!("Bearer {}", token.expose_secret()).parse()?,
        );
        request
            .headers_mut()
            .insert("x-forwarded-for", "::1".parse()?);
        let error =
            tokio::time::timeout(support::DEADLINE, tokio_tungstenite::connect_async(request))
                .await?
                .err()
                .ok_or("wrong peer accepted")?;
        let tokio_tungstenite::tungstenite::Error::Http(response) = error else {
            return Err("expected an HTTP authentication refusal".into());
        };
        assert_eq!(response.status(), 401);
        Ok(())
    })
    .await
}
