//! TOOL-U-066: a `type` refused for its ref or its text says what to do instead (16o live W11, A5: the model put the
//! address in `keys`, sent `text` "None" and a ref carried over from an earlier call; three refusals named only "a ref
//! from the latest snapshot", the opposite of the address bar's way, and its run ended by the loop guard).
use serde_json::json;

use super::{Computer, Screen, TestResult, context, form, refusal, state};
use crate::Tool;

/// What a `type` refused for its ref adds: the way that needs no ref.
const NO_REF: &str = "leave out ref to type into the focus (after ctrl+l for an address)";
/// What a `type` whose text sits in `keys` is told.
const IN_TEXT: &str = "invalid_args: /text: the text to type goes in text";

// TEST: TOOL-U-066
#[tokio::test]
async fn tool_u_066_a_type_refused_for_its_ref_says_to_leave_it_out() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (cx, _) = context(&directory)?;
    let tool = Computer::new(Screen::with(vec![form()], vec![state("w1", 1)]));
    tool.call(super::call(&json!({"action": "observe"})), &cx)
        .await?;
    // A ref that isn't one (the live run's, carried over from an earlier call).
    let said = refusal(
        &tool,
        &cx,
        json!({"action": "type", "ref": "e7a82fedd2874f4f", "text": "httpbin.org/ip"}),
    )
    .await;
    assert!(said.starts_with("invalid_args: /ref: "), "{said}");
    assert!(said.contains(NO_REF), "{said}");
    // A well-formed ref that isn't in the latest snapshot.
    let said = refusal(
        &tool,
        &cx,
        json!({"action": "type", "ref": "e99", "text": "httpbin.org/ip"}),
    )
    .await;
    assert!(said.starts_with("stale_ref: "), "{said}");
    assert!(said.contains(NO_REF), "{said}");
    // A click's refusal is unchanged: leaving out a ref is no way to click.
    let said = refusal(&tool, &cx, json!({"action": "click", "ref": "e99"})).await;
    assert!(!said.contains(NO_REF), "{said}");
    Ok(())
}

// TEST: TOOL-U-066
#[tokio::test]
async fn tool_u_066_text_left_in_keys_is_said_to_go_in_text() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (cx, _) = context(&directory)?;
    let tool = Computer::new(Screen::with(vec![form()], vec![state("w1", 1)]));
    tool.call(super::call(&json!({"action": "observe"})), &cx)
        .await?;
    // The text in keys: none, blank, or a null-word beside it (the live run's "None").
    for text in [None, Some(""), Some("  "), Some("None"), Some("null")] {
        let mut fields = json!({"action": "type", "keys": "httpbin.org/ip"});
        if let Some(text) = text {
            fields["text"] = json!(text);
        }
        let said = refusal(&tool, &cx, fields.clone()).await;
        assert!(said.starts_with(IN_TEXT), "{fields}: {said}");
    }
    // Both at once (the live run's call): one answer naming both.
    let said = refusal(
        &tool,
        &cx,
        json!({"action": "type", "keys": "httpbin.org/ip", "text": "None", "ref": "e7a82fedd2874f4f"}),
    )
    .await;
    assert!(said.starts_with(IN_TEXT), "{said}");
    assert!(said.contains(NO_REF), "{said}");
    // Without keys: no text is still "type needs the text", and "None" is typed as given (a person may mean it).
    let said = refusal(&tool, &cx, json!({"action": "type"})).await;
    assert!(
        said.starts_with("invalid_args: /text: type needs the text"),
        "{said}"
    );
    let said = refusal(&tool, &cx, json!({"action": "type", "text": "None"})).await;
    assert!(said.starts_with("accepted"), "{said}");
    // A text of keys a model sends on purpose is no blank: Enter to submit, Tab to the next field (lead 03:07).
    for keyed in ["\n", "\t", " \n"] {
        let said = refusal(&tool, &cx, json!({"action": "type", "text": keyed})).await;
        assert!(said.starts_with("accepted"), "{keyed:?}: {said}");
        let said = refusal(
            &tool,
            &cx,
            json!({"action": "type", "text": keyed, "keys": "Return"}),
        )
        .await;
        assert!(
            said.starts_with("accepted"),
            "{keyed:?} beside keys: {said}"
        );
    }
    // Blank text (only plain spaces) without keys is still refused as empty.
    let said = refusal(&tool, &cx, json!({"action": "type", "text": "  "})).await;
    assert!(
        said.starts_with("invalid_args: /text: the text is empty"),
        "{said}"
    );
    Ok(())
}
