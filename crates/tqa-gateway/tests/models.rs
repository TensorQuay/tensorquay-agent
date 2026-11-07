//! GW-I-042: model discovery shares conversation permissions and exposes only public fields.
//! GW-I-152: where each model runs, for the picker's second line: on this computer, on your network, or hosted, with
//! the provider's name for a well-known public API (GW-U-094); never an address or a host name.

#[path = "support/fixture.rs"]
mod fixture;
use fixture::TestResult;
use serde_json::{Value, json};
use sqlx::PgPool;

#[sqlx::test(migrator = "tqa_gateway::MIGRATOR")]
#[ignore = "requires TQA_DATABASE_URL and DATABASE_URL"]
async fn gw_i_042_models_are_authenticated_sorted_and_exactly_projected(
    pool: PgPool,
) -> TestResult {
    let registry = tqa_llm::Registry::from_toml(
        "[[routes]]\nname='zeta'\nmodel='second'\nprotocol='openai_chat'\nbase_url='https://models.example/v1'\nkey_ref='secrets/private-reference'\nauth={kind='bearer'}\nextra_headers={ x_private='hidden' }\nextra_body={ private_knob=true }\nnative_tools=false\n[[routes]]\nname='alpha'\nmodel='first'\nprotocol='openai_chat'\nbase_url='https://models.example/v1'\ncontext_window=32000\nvision=true\nreasoning={field='effort',allowed=['low','high'],default='low'}",
    )?;
    fixture::run_configured(pool, Some(registry), "", async |f| {
        assert_eq!(f.get("/api/v1/models", "").await?.status(), 401);
        let (_, admin) = f.admin().await?;
        let (_, member) = f.member("member@example.invalid").await?;
        for cookie in [admin, member] {
            let response = f.get("/api/v1/models", &cookie).await?;
            assert_eq!(response.status(), 200);
            assert_eq!(response.json::<Value>().await?, json!({"items":[
                {"name":"alpha","display_name":null,"description":null,"verified":null,"model":"first","protocol":"openai_chat","context_window":32000,"vision":true,"native_tools":true,"reasoning":{"allowed":["low","high"],"default":"low"},"runs_on":{"where":"hosted","provider":null}},
                {"name":"zeta","display_name":null,"description":null,"verified":null,"model":"second","protocol":"openai_chat","context_window":null,"vision":false,"native_tools":false,"reasoning":null,"runs_on":{"where":"hosted","provider":null}}
            ]}));
            for route in ["alpha", "zeta", "absent"] {
                let response = f.action("/api/v1/conversations", &cookie, Some(route), &json!({"title":"Model permission", "model_route":route})).await?;
                assert_eq!(response.status(), if route == "absent" {403} else {201});
            }
        }
        Ok(())
    }).await
}

// TEST: GW-I-042
// 15c's known issue: New task picked a route whose check had failed, because the list never said. Each route's
// `verified` is its current check: passed, failed, or null when never checked or checked before its last change.
#[sqlx::test(migrator = "tqa_gateway::MIGRATOR")]
#[ignore = "requires TQA_DATABASE_URL and DATABASE_URL"]
async fn gw_i_042_a_route_says_whether_its_current_check_passed(pool: PgPool) -> TestResult {
    let routes: String = ["passed", "failed", "stale", "never"]
        .map(|name| {
            format!(
                "[[routes]]\nname='{name}'\nmodel='m-{name}'\nprotocol='openai_chat'\nbase_url='https://models.example/v1'\n"
            )
        })
        .concat();
    let registry = tqa_llm::Registry::from_toml(&routes)?;
    fixture::run_configured(pool, Some(registry), "", async |f| {
        f.admin().await?;
        let (_, member) = f.member("member@example.invalid").await?;
        // As the probe stores them: the tested version and the digest of the route's document then.
        for (route, ok) in [("passed", true), ("failed", false), ("stale", true)] {
            sqlx::query(
                "INSERT INTO route_probes (route, route_version, document_digest, ok, observed) \
                 SELECT name, version, sha256(convert_to(document::text, 'UTF8')), $2, '{}'::jsonb \
                 FROM model_routes WHERE name = $1",
            )
            .bind(route)
            .bind(ok)
            .execute(&f.pool)
            .await?;
        }
        // The stale route's probe tested another document than the route's now.
        sqlx::query("UPDATE route_probes SET document_digest = sha256('an earlier document'::bytea) WHERE route = 'stale'")
            .execute(&f.pool)
            .await?;
        let listed = f.get("/api/v1/models", &member).await?.json::<Value>().await?;
        let verified: Vec<(String, Value)> = listed["items"]
            .as_array()
            .ok_or("no items")?
            .iter()
            .map(|item| (item["name"].as_str().unwrap_or_default().to_owned(), item["verified"].clone()))
            .collect();
        assert_eq!(
            verified,
            [
                ("failed".to_owned(), json!(false)),
                ("never".to_owned(), Value::Null),
                ("passed".to_owned(), json!(true)),
                ("stale".to_owned(), Value::Null),
            ]
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-152
#[sqlx::test(migrator = "tqa_gateway::MIGRATOR")]
#[ignore = "requires TQA_DATABASE_URL and DATABASE_URL"]
async fn gw_i_152_a_model_says_where_it_runs_and_never_its_address(pool: PgPool) -> TestResult {
    let routes = [
        (
            "loopback",
            "http://127.0.0.1:11434/v1",
            "bearer",
            "this_computer",
            None,
        ),
        (
            "container",
            "http://host.docker.internal:11434/v1",
            "bearer",
            "this_computer",
            None,
        ),
        (
            "service",
            "http://ollama:11434/v1",
            "bearer",
            "network",
            None,
        ),
        (
            "lan",
            "http://192.168.1.1:11434/v1",
            "bearer",
            "network",
            None,
        ),
        (
            "openai",
            "https://api.openai.com/v1",
            "bearer",
            "hosted",
            Some("OpenAI"),
        ),
        (
            "other",
            "https://api.provider.example/v1",
            "bearer",
            "hosted",
            None,
        ),
        // No key is not the same as this computer.
        (
            "keyless",
            "https://open.provider.example/v1",
            "none",
            "hosted",
            None,
        ),
    ];
    let toml = routes
        .iter()
        .map(|(name, url, auth, _, _)| {
            let key = if *auth == "none" { String::new() } else { "key_ref='secrets/k'\n".to_owned() };
            format!("[[routes]]\nname='{name}'\nmodel='m'\nprotocol='openai_chat'\nbase_url='{url}'\nauth={{kind='{auth}'}}\n{key}")
        })
        .collect::<Vec<_>>()
        .concat();
    let registry = tqa_llm::Registry::from_toml(&toml)?;
    fixture::run_configured(pool, Some(registry), "", async |f| {
        f.admin().await?;
        let (_, member) = f.member("member@example.invalid").await?;
        let text = f.get("/api/v1/models", &member).await?.text().await?;
        let body: Value = serde_json::from_str(&text)?;
        for (name, _, _, place, provider) in routes {
            let item = body["items"]
                .as_array()
                .and_then(|items| items.iter().find(|item| item["name"] == name))
                .ok_or(name)?;
            assert_eq!(
                item["runs_on"],
                json!({"where": place, "provider": provider}),
                "{name}"
            );
        }
        for host in [
            "127.0.0.1",
            "docker.internal",
            "ollama",
            "192.168",
            "api.openai.com",
            "provider.example",
            "11434",
        ] {
            assert!(!text.contains(host), "the answer names {host}: {text}");
        }
        Ok(())
    })
    .await
}
