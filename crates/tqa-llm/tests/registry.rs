//! LLM-U-010/011/C-022: the route registry is exact, validated and round-trippable.
//! LLM-U-051: a route may name its display name for people; the relay never reads it, and unset it is never written.

use std::error::Error;

use tqa_llm::{Auth, CacheHint, Limits, Protocol, Registry};

const ROUTE: &str = r#"
[[routes]]
name = "local"
protocol = "openai_chat"
base_url = "https://models.example/v1"
model = "example-model"
auth = { kind = "none" }
"#;

#[test]
fn llm_u_010_registry_defaults_and_round_trip() -> Result<(), Box<dyn Error>> {
    let registry = Registry::from_toml(ROUTE)?;
    assert_eq!(registry.names().collect::<Vec<_>>(), ["local"]);
    assert!(registry.get("absent").is_none());
    let route = registry.get("local").ok_or("missing route")?;
    assert_eq!(route.protocol, Protocol::OpenAiChat);
    assert_eq!(route.auth, Auth::None);
    assert!(route.native_tools);
    assert!(!route.parallel_tools);
    assert!(!route.vision);
    assert_eq!(route.cache_hint, CacheHint::None);
    assert_eq!(Registry::from_toml(&registry.to_toml()?)?, registry);
    Ok(())
}

#[test]
fn llm_u_010_every_declared_route_field_round_trips() -> Result<(), Box<dyn Error>> {
    let source = ROUTE.replace("kind = \"none\"", "kind = \"bearer\"")
        + r#"
key_ref = "secrets/example"
context_window = 128000
max_output = 16000
native_tools = false
parallel_tools = true
vision = true
reasoning = { field = "reasoning_effort", allowed = ["low", "high"], default = "low" }
cache_hint = "prefix"
stream_usage = true
extra_headers = { x-feature = "enabled" }
extra_body = { template_options = { thinking = true } }
quirks = []
"#;
    let registry = Registry::from_toml(&source)?;
    let route = registry.get("local").ok_or("missing route")?;
    assert_eq!(
        route.limits,
        Limits {
            context_window: Some(128_000),
            max_output: Some(16_000),
            ..Limits::default()
        }
    );
    assert!(route.limits.derived());
    assert_eq!(route.extra_body["template_options"]["thinking"], true);
    assert_eq!(Registry::from_toml(&registry.to_toml()?)?, registry);
    Ok(())
}

#[test]
fn llm_u_010_unknown_keys_include_parser_location() -> Result<(), Box<dyn Error>> {
    for source in [
        format!("{ROUTE}typo = true\n"),
        ROUTE.replace("kind = \"none\"", "kind = \"none\", typo = true"),
        "unknown = 1".to_owned(),
    ] {
        let error = Registry::from_toml(&source)
            .err()
            .ok_or("accepted unknown key")?;
        assert!(error.line().is_some());
        assert!(error.column().is_some());
        assert!(error.to_string().contains("unknown field"));
    }
    Ok(())
}

#[test]
fn llm_u_010_registry_rejects_invalid_contracts() {
    let invalid = [
        ROUTE.repeat(2),
        ROUTE.replace("protocol = \"openai_chat\"", ""),
        ROUTE.replace("kind = \"none\"", "kind = \"bearer\""),
        format!(
            "{ROUTE}reasoning = {{ field = \"reasoning_effort\", allowed = [\"low\"], default = \"high\" }}"
        ),
        ROUTE.replace("https://models.example/v1", "file:///models"),
    ];
    for source in invalid {
        assert!(Registry::from_toml(&source).is_err());
    }
    for field in [
        "messages",
        "tools",
        "stream",
        "model",
        "tool_choice",
        "stream_options",
    ] {
        assert!(
            Registry::from_toml(&format!(
                "{ROUTE}extra_body = {{ {field} = \"collision\" }}"
            ))
            .is_err()
        );
    }
}

#[test]
fn llm_u_010_protocol_and_auth_variants_are_registry_data() -> Result<(), Box<dyn Error>> {
    for protocol in ["openai_chat", "openai_responses", "anthropic_messages"] {
        for auth in [
            "{ kind = \"none\" }",
            "{ kind = \"bearer\" }",
            "{ kind = \"header\", name = \"x-credential\" }",
            "{ kind = \"query\", name = \"credential\" }",
        ] {
            let source = ROUTE
                .replace("openai_chat", protocol)
                .replace("{ kind = \"none\" }", auth)
                + "key_ref = \"secrets/example\"\nmax_output = 4096\n";
            let registry = Registry::from_toml(&source)?;
            assert_eq!(Registry::from_toml(&registry.to_toml()?)?, registry);
        }
    }
    Ok(())
}

#[test]
fn llm_u_011_limits_are_derived_only_with_a_context_window() {
    assert!(
        !Limits {
            context_window: None,
            max_output: None,
            ..Limits::default()
        }
        .derived()
    );
    assert!(
        !Limits {
            context_window: None,
            max_output: Some(10),
            ..Limits::default()
        }
        .derived()
    );
    assert!(
        Limits {
            context_window: Some(100),
            max_output: None,
            ..Limits::default()
        }
        .derived()
    );
}

#[test]
fn llm_u_010_registry_rejects_ambiguous_or_unsafe_configuration() {
    for suffix in [
        "quirks=['unknown']",
        "extra_headers={ 'bad header'='value' }",
        "extra_headers={ x-feature=\"bad\\nvalue\" }",
        "extra_headers={ Authorization='short-value' }",
        "reasoning={field='messages',allowed=['low'],default='low'}",
        "reasoning={field='',allowed=['low'],default='low'}",
        "reasoning={field='effort',allowed=['low'],default='low'}\nextra_body={effort='low'}",
    ] {
        assert!(
            Registry::from_toml(&format!("{ROUTE}{suffix}")).is_err(),
            "{suffix}"
        );
    }
    for auth in [
        "{kind='header',name='bad name'}",
        "{kind='query',name=''}",
        "{kind='query',name='bad&name'}",
    ] {
        let source = ROUTE.replace("{ kind = \"none\" }", auth) + "key_ref='secrets/test'";
        assert!(Registry::from_toml(&source).is_err(), "{auth}");
    }
    for url in [
        "not a url",
        "https://user:password@models.example/v1",
        "https://models.example/v1?credential=value",
        "https://models.example/v1#fragment",
    ] {
        assert!(Registry::from_toml(&ROUTE.replace("https://models.example/v1", url)).is_err());
    }
    assert!(Registry::from_toml(&ROUTE.replace("name = \"local\"", "name = \"\"")).is_err());
    assert!(Registry::from_toml(&" ".repeat(tqa_llm::MAX_REGISTRY_BYTES + 1)).is_err());
}

#[test]
fn llm_c_020_extra_fields_cannot_override_owned_values_or_credentials() {
    for field in [
        "max_tokens",
        "temperature",
        "stop",
        "metadata",
        "parallel_tool_calls",
        "reasoning_effort",
    ] {
        assert!(
            Registry::from_toml(&format!("{ROUTE}extra_body = {{ {field} = 'collision' }}"))
                .is_err(),
            "{field}"
        );
    }
    for header in ["Authorization", "x-api-key", "API-KEY"] {
        assert!(
            Registry::from_toml(&format!(
                "{ROUTE}extra_headers = {{ '{header}' = 'short-value' }}"
            ))
            .is_err(),
            "{header}"
        );
    }
    let custom = ROUTE.replace("{ kind = \"none\" }", "{kind='header',name='x-credential'}")
        + "key_ref='secrets/test'\nextra_headers={'X-Credential'='short-value'}";
    assert!(Registry::from_toml(&custom).is_err());
}

/// A Messages route with a thinking budget per allowed value.
const MESSAGES_THINKING: &str = "max_output=16000\nreasoning={field='thinking',allowed=['low','high'],\
    default='low',budget_tokens={low=2048,high=8192}}\n";

#[test]
fn llm_c_022_messages_routes_declare_output_and_budgets() {
    let base =
        "[[routes]]\nname='t'\nprotocol='anthropic_messages'\nbase_url='http://h/v1'\nmodel='m'\n";
    let reasoning = |table: &str| {
        format!("reasoning={{field='thinking',allowed=['low','high'],default='low'{table}}}\n")
    };
    assert!(Registry::from_toml(&format!("{base}{MESSAGES_THINKING}")).is_ok());
    for bad in [
        String::new(),
        format!("max_output=16000\n{}", reasoning("")),
        format!(
            "max_output=16000\n{}",
            reasoning(",budget_tokens={low=2048}")
        ),
        format!(
            "max_output=16000\n{}",
            reasoning(",budget_tokens={low=2048,high=4096,x=4096}")
        ),
        format!(
            "max_output=16000\n{}",
            reasoning(",budget_tokens={low=512,high=4096}")
        ),
        format!(
            "max_output=4096\n{}",
            reasoning(",budget_tokens={low=2048,high=4096}")
        ),
        "max_output=100\nextra_body={thinking={type='enabled'}}\n".into(),
        "max_output=100\nextra_body={system='x'}\n".into(),
        "max_output=100\nextra_headers={'Anthropic-Version'='2099-01-01'}\n".into(),
    ] {
        assert!(
            Registry::from_toml(&format!("{base}{bad}")).is_err(),
            "{bad}"
        );
    }
    let chat = base.replace("anthropic_messages", "openai_chat");
    assert!(
        Registry::from_toml(&format!(
            "{chat}{}",
            reasoning(",budget_tokens={low=2048,high=4096}")
        ))
        .is_err()
    );
}

#[test]
fn proto_u_013_the_registry_cache_hint_is_the_wire_enum() -> Result<(), Box<dyn Error>> {
    // One definition: the route configuration takes the revision 7 wire enum, not a copy of it.
    let hint: tqa_types::CacheHint = CacheHint::AnthropicCacheControl;
    assert_eq!(hint, tqa_types::CacheHint::AnthropicCacheControl);
    let source = format!("{ROUTE}cache_hint = \"prefix\"\n");
    let registry = Registry::from_toml(&source)?;
    let route = registry.get("local").ok_or("missing route")?;
    assert_eq!(route.cache_hint, tqa_types::CacheHint::Prefix);
    Ok(())
}

// TEST: LLM-U-051
#[test]
fn llm_u_051_a_route_may_name_its_display_name_and_unset_it_is_never_written()
-> Result<(), Box<dyn Error>> {
    let named = ROUTE.replace(
        "name = \"local\"\n",
        "name = \"local\"\ndisplay_name = \"Local model\"\n",
    );
    let registry = Registry::from_toml(&named)?;
    assert_eq!(registry.display_name("local"), Some("Local model"));
    assert_eq!(registry.display_name("absent"), None);
    // The file round-trips with it.
    assert_eq!(Registry::from_toml(&registry.to_toml()?)?, registry);
    assert!(
        registry
            .to_toml()?
            .contains("display_name = \"Local model\"")
    );
    // Unset, a route has none and writes no key: its record is byte for byte what it was before.
    let plain = Registry::from_toml(ROUTE)?;
    assert_eq!(plain.display_name("local"), None);
    let config = tqa_llm::RouteConfig::from(plain.get("local").ok_or("missing route")?.clone());
    let record = serde_json::to_string(&config)?;
    assert!(!record.contains("display_name"), "{record}");
    assert!(!plain.to_toml()?.contains("display_name"));
    // The two registries differ only by the name.
    assert_ne!(plain, registry);
    assert_eq!(plain.get("local"), registry.get("local"));
    Ok(())
}
