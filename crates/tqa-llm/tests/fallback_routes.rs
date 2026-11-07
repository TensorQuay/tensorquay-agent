//! LLM-U-018 / CORE-U-041: admin-declared fallback routes and the unavailable status they react to.
#[allow(
    dead_code,
    reason = "This suite uses only the provider fixtures from the shared vocabulary."
)]
#[path = "fixtures/http.rs"]
mod fixtures;

use std::{error::Error, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use tqa_llm::{LlmError, MAX_FALLBACK_ROUTES, Registry};
use tqa_testkit::{FakeClock, FakeModelServer, FakeSleeper, Script, Scripted};

fn route(name: &str, fallbacks: &[&str]) -> String {
    let list: Vec<String> = fallbacks.iter().map(|name| format!("{name:?}")).collect();
    format!(
        "[[routes]]\nname = {name:?}\nprotocol = \"openai_chat\"\nbase_url = \"https://models.example/v1\"\nmodel = \"m\"\nauth = {{ kind = \"none\" }}\nfallbacks = [{}]\n",
        list.join(", ")
    )
}
/// A route name with its declared fallbacks.
type Declared<'a> = (&'a str, &'a [&'a str]);

fn registry(routes: &[Declared<'_>]) -> Result<Registry, tqa_llm::RegistryError> {
    let source: String = routes
        .iter()
        .map(|(name, fallbacks)| route(name, fallbacks))
        .collect();
    Registry::from_toml(&source)
}

#[test]
fn llm_u_018_fallbacks_keep_their_order_and_round_trip() -> Result<(), Box<dyn Error>> {
    let loaded = registry(&[("a", &["c", "b"]), ("b", &[]), ("c", &["b"])])?;
    let first = loaded.get("a").ok_or("missing route")?;
    assert_eq!(first.fallbacks, vec!["c".to_owned(), "b".to_owned()]);
    assert!(loaded.get("b").ok_or("missing route")?.fallbacks.is_empty());
    assert_eq!(Registry::from_toml(&loaded.to_toml()?)?, loaded);
    let plain = Registry::from_toml(&route("solo", &[]).replace("fallbacks = []\n", ""))?;
    assert!(
        plain
            .get("solo")
            .ok_or("missing route")?
            .fallbacks
            .is_empty()
    );
    assert_eq!(MAX_FALLBACK_ROUTES, 3);
    Ok(())
}

#[test]
fn llm_u_018_invalid_fallbacks_are_refused_at_load() {
    let cases: [(&str, Vec<Declared<'_>>); 6] = [
        ("unknown", vec![("a", &["missing"])]),
        ("itself", vec![("a", &["a"])]),
        ("duplicate", vec![("a", &["b", "b"]), ("b", &[])]),
        (
            "too many",
            vec![
                ("a", &["b", "c", "d", "e"]),
                ("b", &[]),
                ("c", &[]),
                ("d", &[]),
                ("e", &[]),
            ],
        ),
        ("two-route cycle", vec![("a", &["b"]), ("b", &["a"])]),
        (
            "longer cycle",
            vec![("a", &["b"]), ("b", &["c"]), ("c", &["a"])],
        ),
    ];
    for (case, routes) in cases {
        let error = registry(&routes).err();
        assert!(error.is_some(), "{case} was accepted");
        let text = error.map(|error| error.to_string()).unwrap_or_default();
        assert!(text.contains("fallback"), "{case}: {text}");
    }
}

#[tokio::test]
async fn llm_e_008_a_503_with_retry_after_is_unavailable_with_its_delay()
-> Result<(), Box<dyn Error>> {
    let clock = FakeClock::new();
    let status = |retry_after: Option<&str>| {
        Script::new(vec![Scripted::Status {
            code: 503,
            body: "busy".into(),
            retry_after: retry_after.map(str::to_owned),
        }])
    };
    let server = FakeModelServer::start(
        vec![status(Some("90")), status(None)],
        Arc::new(FakeSleeper::new(clock.clone())),
    )
    .await?;
    let provider = fixtures::provider(fixtures::route(server.base_url())?, &clock)?;
    let first = provider
        .complete(fixtures::request(), CancellationToken::new())
        .await
        .err()
        .ok_or("accepted 503")?;
    assert!(
        matches!(first, LlmError::Unavailable { retry_after: Some(delay) } if delay == Duration::from_secs(90))
    );
    assert!(first.retryable());
    assert_eq!(first.code(), "unavailable");
    let second = provider
        .complete(fixtures::request(), CancellationToken::new())
        .await
        .err()
        .ok_or("accepted 503")?;
    assert!(matches!(second, LlmError::Http { status: 503, .. }));
    server.shutdown().await?;
    Ok(())
}
