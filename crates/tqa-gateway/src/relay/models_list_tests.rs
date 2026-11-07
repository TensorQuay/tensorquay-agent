//! GW-U-070: reading an endpoint's model list. The OpenAI-compatible list (Chat, Responses, local servers, proxies) and
//! a Messages page with its cursor; only ids and display names are kept; bad ids are skipped; the whole list stops at
//! its cap and says so; anything that isn't a list is unreadable.
use super::models_list::{Collected, MAX_ID_CHARS, MAX_MODELS, page};
use serde_json::json;

type TestResult = Result<(), Box<dyn std::error::Error>>;

// TEST: GW-U-070
#[test]
fn gw_u_070_an_openai_list_gives_its_ids() -> TestResult {
    let body = json!({"object": "list", "data": [
        {"id": "model-a", "object": "model", "created": 1, "owned_by": "someone"},
        {"id": "model-b", "object": "model"},
    ]});
    let page = page(body.to_string().as_bytes())?;
    let ids: Vec<&str> = page.models.iter().map(|model| model.id.as_str()).collect();
    assert_eq!(ids, ["model-a", "model-b"]);
    assert!(page.models.iter().all(|model| model.display_name.is_none()));
    assert_eq!(page.next, None);
    Ok(())
}

// TEST: GW-U-070
#[test]
fn gw_u_070_a_messages_page_gives_ids_names_and_its_cursor() -> TestResult {
    let body = json!({"data": [
        {"type": "model", "id": "m-1", "display_name": "Model One", "created_at": "2026-01-01T00:00:00Z"},
        {"type": "model", "id": "m-2", "display_name": "Model Two"},
    ], "has_more": true, "first_id": "m-1", "last_id": "m-2"});
    let page = page(body.to_string().as_bytes())?;
    assert_eq!(page.models[0].display_name.as_deref(), Some("Model One"));
    assert_eq!(page.next.as_deref(), Some("m-2"));
    let last = json!({"data": [{"id": "m-3"}], "has_more": false, "last_id": "m-3"});
    assert_eq!(page_of(&last)?.next, None);
    Ok(())
}

fn page_of(
    value: &serde_json::Value,
) -> Result<super::models_list::Page, super::models_list::Unreadable> {
    page(value.to_string().as_bytes())
}

// TEST: GW-U-070
#[test]
fn gw_u_070_bad_ids_are_skipped_and_nothing_else_is_kept() -> TestResult {
    let long = "x".repeat(MAX_ID_CHARS + 1);
    let body = json!({"data": [
        {"id": "kept", "secret_field": "never kept", "display_name": "Kept"},
        {"id": ""}, {"id": long}, {"id": "bad\u{7}id"}, {"id": 42}, {"name": "no id"}, "not an object",
        {"id": "also-kept", "display_name": "bad\nname"},
    ]});
    let page = page_of(&body)?;
    let ids: Vec<&str> = page.models.iter().map(|model| model.id.as_str()).collect();
    assert_eq!(ids, ["kept", "also-kept"]);
    // A display name with a control character is dropped, the id kept.
    assert_eq!(page.models[1].display_name, None);
    assert!(!serde_json::to_string(&page.models)?.contains("never kept"));
    Ok(())
}

// TEST: GW-U-070
#[test]
fn gw_u_070_what_isnt_a_list_is_unreadable() {
    for body in [
        "not json".as_bytes().to_vec(),
        json!({"models": []}).to_string().into_bytes(),
        json!({"data": {"id": "x"}}).to_string().into_bytes(),
        json!([{"id": "x"}]).to_string().into_bytes(),
    ] {
        assert!(page(&body).is_err(), "{}", String::from_utf8_lossy(&body));
    }
}

// TEST: GW-U-070
#[test]
fn gw_u_070_the_whole_list_stops_at_its_cap() -> TestResult {
    let mut collected = Collected::default();
    let many: Vec<serde_json::Value> = (0..MAX_MODELS + 20)
        .map(|index| json!({"id": format!("m-{index}")}))
        .collect();
    let first = page_of(&json!({"data": &many[..300]}))?;
    assert!(collected.add(first.models));
    let second = page_of(&json!({"data": &many[300..]}))?;
    assert!(!collected.add(second.models), "the cap stops collecting");
    assert_eq!(collected.models.len(), MAX_MODELS);
    assert!(collected.truncated);
    Ok(())
}
