//! GW-I-039: credential placement is tested before any transport or request recorder.

use super::request::{Operation, build};
use secrecy::{ExposeSecret, SecretString};
use tqa_llm::{Auth, Registry, Route};

fn route() -> Result<Route, Box<dyn std::error::Error>> {
    let registry = Registry::from_toml(
        "[[routes]]\nname='local'\nprotocol='openai_chat'\nbase_url='https://example.com/v1/'\nmodel='fixed'\nauth={kind='none'}\nextra_headers={x-extension='preserved'}",
    )?;
    Ok(registry.get("local").ok_or("missing route")?.clone())
}

#[test]
fn gw_i_039_credentials_follow_each_declared_auth_scheme() -> Result<(), Box<dyn std::error::Error>>
{
    let client = reqwest::Client::new();
    let secret = SecretString::from(format!("synthetic-{}", uuid::Uuid::now_v7()));
    for (auth, header) in [
        (Auth::Bearer, "authorization"),
        (
            Auth::Header {
                name: "x-provider-key".into(),
            },
            "x-provider-key",
        ),
    ] {
        let mut route = route()?;
        route.auth = auth;
        let request = build(&client, &route, Operation::Chat, Some(&secret))?.build()?;
        let credential = request.headers().get(header).ok_or("missing credential")?;
        let expected = if header == "authorization" {
            format!("Bearer {}", secret.expose_secret())
        } else {
            secret.expose_secret().to_owned()
        };
        let matches = credential.to_str()? == expected;
        assert!(matches, "credential placement differed; values omitted");
        assert!(credential.is_sensitive());
        assert_eq!(request.headers()["x-extension"], "preserved");
        assert_eq!(request.url().path(), "/v1/chat/completions");
        assert_eq!(request.method(), reqwest::Method::POST);
        assert!(request.url().query().is_none());
    }
    let mut route = route()?;
    route.auth = Auth::Query {
        name: "provider_key".into(),
    };
    let request = build(&client, &route, Operation::Models, Some(&secret))?.build()?;
    assert!(
        request
            .url()
            .query_pairs()
            .any(|(name, value)| name == "provider_key" && value == secret.expose_secret())
    );
    assert!(!request.headers().contains_key("authorization"));
    assert_eq!(request.url().path(), "/v1/models");
    assert_eq!(request.method(), reqwest::Method::GET);
    Ok(())
}

#[test]
fn gw_i_039_missing_or_invalid_credentials_are_redacted() -> Result<(), Box<dyn std::error::Error>>
{
    let client = reqwest::Client::new();
    let mut route = route()?;
    let request = build(&client, &route, Operation::Chat, None)?.build()?;
    assert!(!request.headers().contains_key("authorization"));
    route.auth = Auth::Bearer;
    for secret in [
        None,
        Some(SecretString::from("")),
        Some(SecretString::from("invalid\r\ncredential")),
    ] {
        let error = build(&client, &route, Operation::Chat, secret.as_ref())
            .err()
            .ok_or("accepted invalid credential")?;
        assert_eq!(error, crate::GatewayError::SecretUnavailable);
        assert!(!error.to_string().contains("credential"));
    }
    Ok(())
}
