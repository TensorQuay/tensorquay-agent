//! SEC-012: reject oversize catalogs before request construction or network I/O.
#[path = "fixtures/http.rs"]
mod fixtures;
use serde_json::json;
use std::{error::Error, sync::Arc};
use tokio_util::sync::CancellationToken;
use tqa_llm::{LlmError, StreamLimits, collect};
use tqa_testkit::{FakeClock, FakeModelServer, FakeSleeper, Script};
use tqa_types::ToolCatalogLimit;

type TestResult = Result<(), Box<dyn Error>>;

fn exact_byte_limits() -> Result<(tqa_llm::ToolSpec, tqa_llm::ToolSpec), Box<dyn Error>> {
    let mut schema = fixtures::tool();
    schema.parameters = json!({"description":""});
    let overhead = serde_json::to_vec(&schema.parameters)?.len();
    schema.parameters["description"] = json!("x".repeat(65536 - overhead));
    let mut catalog = fixtures::tool();
    catalog.description.clear();
    // The route's default (auto) sends `strict` as the schema earns it; the exact limit counts what goes out.
    let strict = tqa_types::strict_compatible(&catalog.parameters);
    let wire = json!([{"type":"function","function":{"name":catalog.name,"description":"",
        "parameters":catalog.parameters,"strict":strict}}]);
    catalog.description = "x".repeat(1_048_576 - serde_json::to_vec(&wire)?.len());
    Ok((schema, catalog))
}

#[tokio::test]
async fn sec_012_llm_rejects_schema_catalog_and_hundred_thousand_tools_before_io() -> TestResult {
    let clock = FakeClock::new();
    let server = FakeModelServer::start(vec![], Arc::new(FakeSleeper::new(clock.clone()))).await?;
    let provider = fixtures::provider(fixtures::route(server.base_url())?, &clock)?;
    for limit in [
        ToolCatalogLimit::SchemaBytes,
        ToolCatalogLimit::CatalogBytes,
        ToolCatalogLimit::Count,
    ] {
        let mut tool = fixtures::tool();
        match limit {
            ToolCatalogLimit::SchemaBytes => {
                tool.parameters = json!({"private-schema":"x".repeat(65536)});
            }
            ToolCatalogLimit::CatalogBytes => {
                tool.description = "private-description".repeat(65536);
            }
            ToolCatalogLimit::Count => {}
        }
        let counts: &[usize] = if limit == ToolCatalogLimit::Count {
            &[257, 100_000]
        } else {
            &[1]
        };
        for count in counts {
            let mut request = fixtures::request();
            request.tools = vec![tool.clone(); *count];
            let error = provider
                .complete(request, CancellationToken::new())
                .await
                .err()
                .ok_or("oversize catalog reached the upstream")?;
            assert!(matches!(&error, LlmError::CatalogLimit(actual) if *actual == limit));
            assert_eq!(error.code(), "tool_catalog_limit");
            assert!(!error.retryable());
            assert!(!format!("{error:?} {error}").contains("private-"));
            assert!(
                server.requests().is_empty(),
                "upstream saw an oversized request"
            );
        }
    }
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn sec_012_llm_accepts_each_exact_limit_and_rejects_the_next_byte() -> TestResult {
    let clock = FakeClock::new();
    let server = FakeModelServer::start(
        vec![Script::new(vec![]); 3],
        Arc::new(FakeSleeper::new(clock.clone())),
    )
    .await?;
    let provider = fixtures::provider(fixtures::route(server.base_url())?, &clock)?;
    let (mut schema, mut catalog) = exact_byte_limits()?;
    for tools in [
        vec![schema.clone()],
        vec![catalog.clone()],
        vec![fixtures::tool(); 256],
    ] {
        let mut request = fixtures::request();
        request.tools = tools;
        collect(
            provider.complete(request, CancellationToken::new()).await?,
            StreamLimits::RECOMMENDED,
        )
        .await?;
    }
    assert_eq!(server.requests().len(), 3);
    assert_eq!(
        serde_json::to_vec(&server.requests()[1].body["tools"])?.len(),
        1_048_576
    );
    let padding = schema.parameters["description"]
        .as_str()
        .ok_or("schema padding")?
        .len();
    schema.parameters["description"] = json!("x".repeat(padding + 1));
    catalog.description.push('x');
    for (tool, expected) in [
        (schema, ToolCatalogLimit::SchemaBytes),
        (catalog, ToolCatalogLimit::CatalogBytes),
    ] {
        let mut request = fixtures::request();
        request.tools = vec![tool];
        let error = provider
            .complete(request, CancellationToken::new())
            .await
            .err()
            .ok_or("one extra byte was accepted")?;
        assert!(matches!(error, LlmError::CatalogLimit(actual) if actual == expected));
        assert_eq!(server.requests().len(), 3);
    }
    server.shutdown().await?;
    Ok(())
}
