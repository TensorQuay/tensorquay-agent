//! GW-I-040: bootstrap credentials are inert until a single, address-bound activation.

#[path = "support/fixture.rs"]
mod support;
use sqlx::PgPool;
use support::{TestResult, run};

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_040_reservation_authenticates_nothing_and_binds_once(pool: PgPool) -> TestResult {
    run(pool, async |fixture| {
        let (owner, _) = fixture.admin().await?;
        let conversation = fixture.conversation(owner.id).await?;
        let tokens = fixture.gateway.runtime_tokens();
        let peer = std::net::Ipv4Addr::LOCALHOST.into();
        let sandbox = "reserved-instance".parse()?;
        let (reservation, token) = tokens.reserve(&conversation.id).await?;
        assert_eq!(tokens.resolve(&token, peer).await?, None);
        assert!(tokens.reserve(&conversation.id).await.is_err());
        tokens.bind(&reservation, &sandbox, peer).await?;
        assert_eq!(
            tokens.resolve(&token, peer).await?,
            Some((conversation.id.clone(), sandbox.clone()))
        );
        assert!(tokens.bind(&reservation, &sandbox, peer).await.is_err());
        assert_eq!(
            tokens
                .resolve(&token, std::net::Ipv6Addr::LOCALHOST.into())
                .await?,
            None
        );
        tokens.revoke(&sandbox).await?;
        assert_eq!(tokens.resolve(&token, peer).await?, None);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_040_expired_reservation_never_activates(pool: PgPool) -> TestResult {
    run(pool, async |fixture| {
        let (owner, _) = fixture.admin().await?;
        let conversation = fixture.conversation(owner.id).await?;
        let tokens = fixture.gateway.runtime_tokens();
        let peer = std::net::Ipv4Addr::LOCALHOST.into();
        let (reservation, token) = tokens.reserve(&conversation.id).await?;
        sqlx::query("UPDATE runtime_token_reservations SET expires_at=CURRENT_TIMESTAMP-interval '1 second' WHERE conversation_id=$1")
            .bind(uuid::Uuid::parse_str(conversation.id.as_str())?).execute(&fixture.pool).await?;
        assert!(tokens.bind(&reservation, &"expired-instance".parse()?, peer).await.is_err());
        assert_eq!(tokens.resolve(&token, peer).await?, None);
        let (new_reservation, replacement) = tokens.reserve(&conversation.id).await?;
        tokens.bind(&new_reservation, &"new-instance".parse()?, peer).await?;
        assert!(tokens.resolve(&replacement, peer).await?.is_some());
        assert_eq!(tokens.resolve(&token, peer).await?, None);
        Ok(())
    }).await
}
