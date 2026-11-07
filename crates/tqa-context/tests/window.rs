//! CTX-U-001/002/015/017: conservative token estimates and stable prompt slots.
use std::error::Error;
use tqa_context::{
    Budget, CallRecord, ContextError, Message, Role, Window, estimate, message_tokens,
};

#[test]
fn ctx_u_001_estimate_counts_utf8_bytes_and_all_message_fields() -> Result<(), Box<dyn Error>> {
    for (text, tokens) in [
        ("", 0),
        ("1234567", 2),
        ("é", 1),
        ("你好", 2),
        ("12345678", 3),
    ] {
        assert_eq!(estimate(text).tokens, tokens);
        assert!(estimate(text).approximate);
    }
    let message = Message {
        role: Role::Assistant,
        text: "1234567".into(),
        reasoning: "abcd".into(),
        reasoning_states: vec![],
        tool_calls: vec![CallRecord {
            call_id: "call".parse()?,
            name: "read_file".into(),
            arguments: "{}".into(),
        }],
        tool_call_id: Some("call".parse()?),
    };
    // Reasoning text is never sent, so it takes no room (CORE-U-073).
    let without = Message {
        reasoning: String::new(),
        ..message.clone()
    };
    assert_eq!(
        message_tokens(&message).tokens,
        message_tokens(&without).tokens
    );
    assert_eq!(message_tokens(&message).tokens, 12);
    assert!(message_tokens(&message).approximate);
    let encoded = serde_json::to_vec(&message)?;
    assert_eq!(serde_json::from_slice::<Message>(&encoded)?, message);
    Ok(())
}

#[test]
fn ctx_u_002_fixed_slots_and_exact_budget_boundary() -> Result<(), Box<dyn Error>> {
    let history = vec![Message::new(Role::User, "abcdefg")];
    let budget = Budget {
        context_window: 32,
        reserve_output: 10,
        guessed: false,
    };
    let window = Window::assemble("1234567", "a", "[]", &history, budget)?;
    assert_eq!(
        window
            .messages()
            .iter()
            .map(|message| message.text.as_str())
            .collect::<Vec<_>>(),
        vec!["1234567", "a", "[]", "abcdefg"]
    );
    assert!(
        window.messages()[..3]
            .iter()
            .all(|message| message.role == Role::System)
    );
    assert_eq!(window.messages()[3].role, Role::User);
    for (budget, available) in [
        (
            Budget {
                guessed: true,
                ..budget
            },
            14,
        ),
        (
            Budget {
                context_window: 10,
                ..budget
            },
            0,
        ),
    ] {
        assert!(
            matches!(Window::assemble("1234567","a","[]",&history,budget),Err(ContextError::OverBudget {needed:22,available:actual}) if actual==available)
        );
    }
    Ok(())
}

#[test]
fn ctx_u_015_prefix_stays_identical_as_history_changes() -> Result<(), Box<dyn Error>> {
    let budget = Budget {
        context_window: 1000,
        reserve_output: 100,
        guessed: false,
    };
    let first = Window::assemble("a", "bc", "[]", &[], budget)?;
    for turn in 0..20 {
        let next = Window::assemble(
            "a",
            "bc",
            "[]",
            &[Message::new(Role::User, format!("turn {turn}"))],
            budget,
        )?;
        assert_eq!(first.prefix_bytes(), next.prefix_bytes());
    }
    assert_ne!(
        first.prefix_bytes(),
        Window::assemble("ab", "c", "[]", &[], budget)?.prefix_bytes()
    );
    Ok(())
}

/// CTX-U-017: on a native-tool route the catalogue travels once, in the request's `tools`; the window still counts it.
#[test]
fn ctx_u_017_the_native_tools_slot_is_empty_but_still_counted() -> Result<(), Box<dyn Error>> {
    let history = vec![Message::new(Role::User, "abcdefg")];
    let catalogue = r#"[{"name":"read_file","description":"Read a file"}]"#;
    let roomy = Budget {
        context_window: 400,
        reserve_output: 10,
        guessed: false,
    };
    let text = Window::assemble("system", "rules", catalogue, &history, roomy)?;
    let native = Window::assemble_native("system", "rules", catalogue, &history, roomy)?;
    assert_eq!(
        text.messages()[2].text,
        catalogue,
        "the text protocol keeps the slot"
    );
    assert_eq!(native.messages()[2].text, "", "native: the slot is empty");
    assert_eq!(native.messages()[2].role, Role::System);
    assert_eq!(
        native.messages().len(),
        text.messages().len(),
        "three fixed slots either way"
    );
    assert_eq!(
        native.prefix_bytes(),
        text.prefix_bytes(),
        "the cache prefix still includes the catalogue"
    );
    let needed = Window::needed_tokens("system", "rules", catalogue, &history);
    // The fit check counts the catalogue: just enough fits, one token less doesn't.
    let at = |capacity: u64| Budget {
        context_window: u32::try_from(capacity).unwrap_or(u32::MAX) + 10,
        reserve_output: 10,
        guessed: false,
    };
    assert!(Window::assemble_native("system", "rules", catalogue, &history, at(needed)).is_ok());
    assert!(matches!(
        Window::assemble_native("system", "rules", catalogue, &history, at(needed - 1)),
        Err(ContextError::OverBudget { .. })
    ));
    Ok(())
}
