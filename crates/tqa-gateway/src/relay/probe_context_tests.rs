//! GW-U-153 (F3): where Ollama's own API is, and which loaded model is the route's.
use super::{listed, origin};
use serde_json::json;

// TEST: GW-U-153
#[test]
fn gw_u_153_the_servers_root_is_the_base_url_without_v1() {
    for (base, root) in [
        (
            "http://host.docker.internal:11434/v1",
            "http://host.docker.internal:11434/",
        ),
        (
            "http://host.docker.internal:11434/v1/",
            "http://host.docker.internal:11434/",
        ),
        (
            "http://192.0.2.5:11434/ollama/v1?x=1#f",
            "http://192.0.2.5:11434/ollama",
        ),
        ("http://ollama:11434", "http://ollama:11434/"),
    ] {
        assert_eq!(
            origin(base).map(|url| url.to_string()).as_deref(),
            Some(root),
            "{base}"
        );
    }
    assert!(origin("not a url").is_none());
}

// TEST: GW-U-153
#[test]
fn gw_u_153_the_routes_model_is_found_by_name_or_as_latest() {
    let answer = json!({"models": [
        {"name": "llama3:8b", "model": "llama3:8b", "context_length": 8192},
        {"name": "qwen3:latest", "model": "qwen3:latest", "context_length": 4096},
    ]});
    assert_eq!(listed(&answer, "qwen3"), Some(4096), "no tag means latest");
    assert_eq!(listed(&answer, "qwen3:latest"), Some(4096));
    assert_eq!(listed(&answer, "llama3:8b"), Some(8192));
    // Another tag, an unloaded model, a missing or zero context, or another shape: unknown.
    assert_eq!(listed(&answer, "qwen3:8b"), None);
    assert_eq!(listed(&answer, "mistral"), None);
    assert_eq!(listed(&json!({"models": [{"name": "m"}]}), "m"), None);
    assert_eq!(
        listed(
            &json!({"models": [{"name": "m", "context_length": 0}]}),
            "m"
        ),
        None
    );
    assert_eq!(listed(&json!({"loaded": []}), "m"), None);
}
