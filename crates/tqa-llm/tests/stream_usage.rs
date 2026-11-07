//! LLM-U-053: a route's `stream_usage` is on unless set false. A Chat request then asks for usage in the stream; a route
//! set false (a server that refuses the field) has the relay send the request without `stream_options`, whatever the
//! runtime put there. Leaving the field out keeps it out of the stored record, and the record's old form (the field
//! written as false) is still available to the seeder's one-time digest upgrade.
use serde_json::{Value, json};
use std::error::Error;
use tqa_llm::{
    CompletionRequest, ContentPart, Message, Registry, Role, encode_request,
    relay_shaping::shape_request,
};

type TestResult = Result<(), Box<dyn Error>>;

fn registry(extra: &str) -> Result<Registry, Box<dyn Error>> {
    Ok(Registry::from_toml(&format!(
        "[[routes]]\nname='local'\nprotocol='openai_chat'\nbase_url='http://127.0.0.1:9/v1'\nmodel='m'\n\
         auth={{kind='none'}}\n{extra}"
    ))?)
}

fn request() -> CompletionRequest {
    CompletionRequest {
        messages: vec![Message {
            role: Role::User,
            content: vec![ContentPart::Text("hi".into())],
            tool_calls: vec![],
            tool_call_id: None,
            name: None,
        }],
        ..CompletionRequest::default()
    }
}

fn body(registry: &Registry) -> Result<Value, Box<dyn Error>> {
    let route = registry.get("local").ok_or("no route")?;
    Ok(serde_json::from_slice(&encode_request(route, &request())?)?)
}

// TEST: LLM-U-053
#[test]
fn llm_u_053_usage_in_the_stream_is_on_unless_set_false() -> TestResult {
    let absent = registry("")?;
    let route = absent.get("local").ok_or("no route")?;
    assert_eq!(route.stream_usage, None);
    assert!(route.stream_usage());
    assert_eq!(
        body(&absent)?["stream_options"],
        json!({"include_usage": true})
    );
    let on = registry("stream_usage=true\n")?;
    assert_eq!(body(&on)?["stream_options"], json!({"include_usage": true}));
    let off = registry("stream_usage=false\n")?;
    assert!(!off.get("local").ok_or("no route")?.stream_usage());
    assert_eq!(body(&off)?.get("stream_options"), None, "nothing to refuse");
    Ok(())
}

// TEST: LLM-U-053
#[test]
fn llm_u_053_the_relay_leaves_stream_options_out_for_a_route_set_false() -> TestResult {
    // The runtime always asks for usage; the relay follows the registry's route.
    let off = registry("stream_usage=false\n")?;
    let mut sent =
        json!({"messages": [], "stream": true, "stream_options": {"include_usage": true}});
    shape_request(off.get("local").ok_or("no route")?, &mut sent)?;
    assert_eq!(sent.get("stream_options"), None);
    let absent = registry("")?;
    let mut sent = json!({"messages": [], "stream": true});
    shape_request(absent.get("local").ok_or("no route")?, &mut sent)?;
    assert_eq!(sent["stream_options"], json!({"include_usage": true}));
    Ok(())
}

// TEST: LLM-U-053
#[test]
fn llm_u_053_an_absent_setting_stays_out_of_the_record_and_its_old_form_is_kept() -> TestResult {
    let absent = registry("")?;
    let config = absent.config(absent.get("local").ok_or("no route")?.clone());
    assert_eq!(
        config.stream_usage, None,
        "the record keeps the file's silence"
    );
    assert_eq!(serde_json::to_value(&config)?.get("stream_usage"), None);
    // The seed digest of releases before this one was taken over the field written as false.
    assert_eq!(config.legacy_form().stream_usage, Some(false));
    assert_eq!(
        serde_json::to_value(config.legacy_form())?["stream_usage"],
        json!(false)
    );
    let on = registry("stream_usage=true\n")?;
    let config = on.config(on.get("local").ok_or("no route")?.clone());
    assert_eq!(
        config.legacy_form(),
        config,
        "a set value was written the same before"
    );
    Ok(())
}
