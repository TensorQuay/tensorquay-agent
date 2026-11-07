//! CORE-U-041 configuration: the runner refuses fallback routes it could not use safely before any run starts.
mod support;
use std::{error::Error, sync::Arc};
use support::*;
use tqa_core::{CoreError, RouteProvider, Runner};
use tqa_llm::Registry;
use tqa_testkit::FakeProvider;
use tqa_tools::ToolRegistry;
use tqa_types::RouteSnapshot;

fn snapshot(name: &str) -> RouteSnapshot {
    RouteSnapshot {
        name: name.into(),
        relay_path: format!("/relay/r/{name}"),
        model: format!("{name}-model"),
        ..route()
    }
}
/// A fallback whose provider serves `served`, declared as the snapshot `declared`.
fn fallback(
    h: &Harness,
    declared: RouteSnapshot,
    served: &str,
) -> Result<RouteProvider, Box<dyn Error>> {
    let routes = Registry::from_toml(&format!(
        "[[routes]]\nname='{served}'\nprotocol='openai_chat'\nbase_url='http://localhost'\nmodel='{served}-model'\nauth={{kind='none'}}\n"
    ))?;
    let provider = FakeProvider::new(
        routes.get(served).ok_or("missing route")?.clone(),
        vec![],
        Arc::new(h.sleeper.clone()),
    );
    Ok(RouteProvider {
        route: declared,
        provider: Arc::new(provider),
    })
}
fn refused(h: Harness) -> Option<&'static str> {
    match Runner::new(h.deps) {
        Err(CoreError::InvalidConfiguration(reason)) => Some(reason),
        _ => None,
    }
}

#[test]
fn core_u_041_unusable_fallback_routes_are_refused_before_any_run() -> TestResult {
    let mut h = Harness::new(vec![], ToolRegistry::new())?;
    h.deps.fallback_routes = ["a", "b", "c", "d"]
        .iter()
        .map(|name| fallback(&h, snapshot(name), name))
        .collect::<Result<_, _>>()?;
    assert_eq!(refused(h), Some("too many fallback routes"));

    // A provider that differs by name only, or by model only, is still the wrong route.
    let by_name = RouteSnapshot {
        model: "elsewhere-model".into(),
        ..snapshot("backup")
    };
    let by_model = RouteSnapshot {
        model: "other-model".into(),
        ..snapshot("backup")
    };
    for (declared, served) in [(by_name, "elsewhere"), (by_model, "backup")] {
        let mut h = Harness::new(vec![], ToolRegistry::new())?;
        h.deps.fallback_routes = vec![fallback(&h, declared, served)?];
        assert_eq!(
            refused(h),
            Some("fallback provider does not match its route"),
            "{served}"
        );
    }

    let mut h = Harness::new(vec![], ToolRegistry::new())?;
    let own = route().name;
    h.deps.fallback_routes = vec![fallback(&h, snapshot(&own), &own)?];
    assert_eq!(
        refused(h),
        Some("duplicate fallback route"),
        "the conversation's own route"
    );

    let mut h = Harness::new(vec![], ToolRegistry::new())?;
    h.deps.fallback_routes = vec![
        fallback(&h, snapshot("backup"), "backup")?,
        fallback(&h, snapshot("backup"), "backup")?,
    ];
    assert_eq!(
        refused(h),
        Some("duplicate fallback route"),
        "the same fallback twice"
    );

    let mut h = Harness::new(vec![], ToolRegistry::new())?;
    let invalid = RouteSnapshot {
        relay_path: "not-a-relay-path".into(),
        ..snapshot("backup")
    };
    h.deps.fallback_routes = vec![fallback(&h, invalid, "backup")?];
    assert_eq!(refused(h), Some("fallback route snapshot"));

    let mut h = Harness::new(vec![], ToolRegistry::new())?;
    h.deps.fallback_routes = ["a", "b", "c"]
        .iter()
        .map(|name| fallback(&h, snapshot(name), name))
        .collect::<Result<_, _>>()?;
    assert!(
        Runner::new(h.deps).is_ok(),
        "three valid fallbacks, the limit, are accepted"
    );
    Ok(())
}

#[test]
fn core_u_041_a_provider_for_another_route_than_the_session_is_refused() -> TestResult {
    for (name, model) in [("other", None), ("test", Some("other-model"))] {
        let mut h = Harness::new(vec![], ToolRegistry::new())?;
        h.deps.route.name = name.into();
        if let Some(model) = model {
            h.deps.route.model = model.into();
        }
        assert_eq!(
            refused(h),
            Some("provider route does not match session"),
            "{name} {model:?}"
        );
    }
    Ok(())
}
