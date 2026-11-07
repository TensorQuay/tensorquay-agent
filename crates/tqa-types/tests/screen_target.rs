//! PROTO-U-037 (ADR-0039, revision 14): `tool.result`'s optional `target`, the element a `computer` action acted on,
//! and its optional `evidence`, which of the action's screenshots were kept.
use serde_json::json;
use tqa_types::{Event, ScreenEvidence, ScreenRole, ScreenTarget};

type TestResult = Result<(), Box<dyn std::error::Error>>;

// TEST: PROTO-U-037
#[test]
fn proto_u_037_a_tool_result_carries_its_target_or_writes_none() -> TestResult {
    let with = json!({"type": "tool.result", "call_id": "c-1", "status": "ok", "content": "Clicked e7.",
                      "target": {"role": "button", "name": "Save"}});
    let event: Event = serde_json::from_value(with.clone())?;
    assert_eq!(serde_json::to_value(&event)?, with, "round-trips exactly");
    let without =
        json!({"type": "tool.result", "call_id": "c-2", "status": "ok", "content": "done"});
    let event: Event = serde_json::from_value(without.clone())?;
    assert_eq!(
        serde_json::to_value(&event)?,
        without,
        "no target is written when there is none"
    );
    Ok(())
}

// TEST: PROTO-U-037
#[test]
fn proto_u_037_an_odd_target_is_refused() {
    for target in [
        json!({"role": "button"}),
        json!({"role": "button", "name": "Save", "x": 3}),
        json!({"role": "", "name": "Save"}),
        json!({"role": "button", "name": "two\nlines"}),
        json!({"role": "Button", "name": ""}),
        json!({"role": "toolbar", "name": ""}),
        json!({"role": "button", "name": "n".repeat(201)}),
    ] {
        let event = json!({"type": "tool.result", "call_id": "c-1", "status": "ok", "content": "x", "target": target});
        assert!(serde_json::from_value::<Event>(event).is_err(), "{target}");
    }
    assert!(
        ScreenTarget::new(ScreenRole::Button, String::new()).is_ok(),
        "an empty name is allowed"
    );
}

// TEST: PROTO-U-037
#[test]
fn proto_u_037_a_role_is_one_of_the_agreed_words() -> TestResult {
    let words = [
        "button",
        "link",
        "textbox",
        "searchbox",
        "checkbox",
        "radio",
        "switch",
        "combobox",
        "listbox",
        "option",
        "menu",
        "menuitem",
        "menuitemcheckbox",
        "menuitemradio",
        "tab",
        "slider",
        "spinbutton",
        "treeitem",
        "cell",
        "icon",
        "heading",
        "other",
    ];
    for word in words {
        let role = ScreenRole::from_word(word);
        assert_eq!(serde_json::to_value(role)?, json!(word));
        assert_eq!(serde_json::from_value::<ScreenRole>(json!(word))?, role);
    }
    assert_eq!(ScreenRole::from_word("toolbar"), ScreenRole::Other);
    assert!(
        ScreenTarget::new(ScreenRole::Icon, "é".repeat(200)).is_ok(),
        "the bound counts characters, not bytes"
    );
    Ok(())
}

// TEST: PROTO-U-037
#[test]
fn proto_u_037_a_tool_result_says_which_evidence_was_kept() -> TestResult {
    let with = json!({"type": "tool.result", "call_id": "c-1", "status": "ok", "content": "Clicked e7.",
                      "target": {"role": "button", "name": "Save"}, "evidence": {"before": true, "after": false}});
    let event: Event = serde_json::from_value(with.clone())?;
    assert!(matches!(
        &event,
        Event::ToolResult {
            evidence: Some(ScreenEvidence {
                before: Some(true),
                after: false
            }),
            ..
        }
    ));
    assert_eq!(serde_json::to_value(&event)?, with, "round-trips exactly");
    // A screenshot call's Ok result: its one capture, as `after`, and no `before`.
    let shot = json!({"type": "tool.result", "call_id": "c-2", "status": "ok", "content": "A screenshot.",
                      "evidence": {"after": false}});
    let event: Event = serde_json::from_value(shot.clone())?;
    assert!(matches!(
        &event,
        Event::ToolResult {
            evidence: Some(ScreenEvidence {
                before: None,
                after: false
            }),
            ..
        }
    ));
    assert_eq!(
        serde_json::to_value(&event)?,
        shot,
        "round-trips exactly, before still absent"
    );
    for evidence in [
        json!({"before": true}),
        json!({}),
        json!({"before": null, "after": true}),
        json!({"before": true, "after": null}),
        json!({"before": true, "after": true, "during": true}),
        json!(true),
    ] {
        let event = json!({"type": "tool.result", "call_id": "c-1", "status": "ok", "content": "x", "evidence": evidence});
        assert!(
            serde_json::from_value::<Event>(event).is_err(),
            "{evidence}"
        );
    }
    Ok(())
}
