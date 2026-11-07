use super::{Place, loopback, place, provider};

// GW-U-094: where a route runs, from its address as written; no lookup decides it. The cases are shared with the web's
// copy of the rule (WEB-U-095, `web/src/settings/route-local.ts`), so the two can't drift apart.
const CASES: &str = include_str!("../../../web/src/settings/route-place-cases.json");

#[test]
fn gw_u_094_an_address_says_this_computer_network_or_hosted() -> Result<(), serde_json::Error> {
    // A place the table names that isn't one of the three fails to parse, rather than counting as hosted.
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    enum Expected {
        ThisComputer,
        Network,
        Hosted,
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Case {
        url: String,
        place: Expected,
    }
    let cases: Vec<Case> = serde_json::from_str(CASES)?;
    assert!(cases.len() >= 30, "the shared table lost its cases");
    for case in cases {
        let expected = match case.place {
            Expected::ThisComputer => Place::ThisComputer,
            Expected::Network => Place::Network,
            Expected::Hosted => Place::Hosted,
        };
        assert_eq!(place(&case.url), expected, "{}", case.url);
    }
    Ok(())
}

#[test]
fn gw_u_094_only_a_well_known_public_api_names_its_provider() {
    for (url, expected) in [
        ("https://api.openai.com/v1", Some("OpenAI")),
        ("https://API.Anthropic.com/v1", Some("Anthropic")),
        ("https://api.deepseek.com", Some("DeepSeek")),
        (
            "https://generativelanguage.googleapis.com/v1beta/openai",
            Some("Google"),
        ),
        ("https://openrouter.ai/api/v1", Some("OpenRouter")),
        ("https://api.openai.com./v1", Some("OpenAI")),
        ("https://api.mistral.ai/v1", Some("Mistral")),
        ("https://api.groq.com/openai/v1", Some("Groq")),
        // A look-alike, a proxy in front of one, or any other host names nothing.
        ("https://api.openai.com.proxy.example/v1", None),
        ("https://openai-proxy.internal/v1", None),
        ("https://models.example/v1", None),
        ("http://127.0.0.1:11434/v1", None),
    ] {
        assert_eq!(provider(url), expected, "{url}");
    }
}

// TEST: GW-U-154
#[test]
fn gw_u_154_only_a_loopback_address_is_the_gateways_own_container() {
    for url in [
        "http://localhost:11434/v1",
        "http://LOCALHOST:8000",
        "http://ollama.localhost/v1",
        "http://localhost./v1",
        "http://127.0.0.1:8000/v1",
        "http://[::1]:11434/v1",
        "http://[::ffff:127.0.0.1]:8000",
    ] {
        assert!(loopback(url), "{url}");
    }
    // Anywhere in the 127/8 block, not only its first address (built here, so no other literal address is written).
    let elsewhere = format!("http://127.{}.{}.{}/v1", 8, 9, 10);
    assert!(loopback(&elsewhere), "{elsewhere}");
    for url in [
        "http://host.docker.internal:11434/v1",
        "http://host.containers.internal:11434/v1",
        "http://ollama:11434/v1",
        "http://192.0.2.20:8000/v1",
        "http://[fd00::1]:8000",
        "https://api.openai.com/v1",
        "not a url",
    ] {
        assert!(!loopback(url), "{url}");
    }
}
