//! TOOL-U-032 (schemas): every builtin tool's parameter schema meets the strict rules (`tqa_types::strict_compatible`,
//! LLM-U-034), so a strict endpoint accepts it: every property required, optional ones nullable, every object closed,
//! and the rules the schemas used to state with `not` checked by the tools themselves.
use serde_json::json;
use tqa_types::{ConversationId, strict_compatible};

use super::{EditFile, Glob, Grep, ReadFile, ReadNotes, Shell, WriteFile, WriteNotes, arguments};
use crate::Tool;

#[test]
fn tool_u_032_every_builtin_schema_is_strict_valid() -> Result<(), Box<dyn std::error::Error>> {
    let conversation: ConversationId = "conversation_strict".parse()?;
    let proxy = crate::ProxyEndpoint::new("http://gateway:3128")?;
    let fetch = super::FetchUrl::new(proxy, &"runtime-token-value".into())?;
    let schemas = [
        ("read_file", ReadFile::new().spec().parameters.clone()),
        ("write_file", WriteFile::new().spec().parameters.clone()),
        ("edit_file", EditFile::new().spec().parameters.clone()),
        ("glob", Glob::new().spec().parameters.clone()),
        ("grep", Grep::new().spec().parameters.clone()),
        (
            "read_notes",
            ReadNotes::new(&conversation).spec().parameters.clone(),
        ),
        (
            "write_notes",
            WriteNotes::new(&conversation).spec().parameters.clone(),
        ),
        ("shell", Shell::new().spec().parameters.clone()),
        ("fetch_url", fetch.spec().parameters.clone()),
        ("computer", super::computer::schema()),
    ];
    assert_eq!(
        schemas.len() + 1,
        super::BUILTIN_TOOL_NAMES.len(),
        "skill is checked in tqa-prompt"
    );
    for (name, schema) in schemas {
        assert!(strict_compatible(&schema), "{name}: {schema}");
    }
    Ok(())
}

#[test]
fn tool_u_032_a_null_argument_is_the_same_as_an_absent_one() {
    // The shared parser drops null members at any depth before the tool reads its arguments.
    #[derive(serde::Deserialize, Debug, PartialEq)]
    struct Nested {
        flag: Option<bool>,
        #[serde(default)]
        count: u32,
    }
    #[derive(serde::Deserialize, Debug, PartialEq)]
    struct Outer {
        name: String,
        #[serde(default)]
        items: Vec<Nested>,
        limit: Option<u32>,
    }
    let parsed: Result<Outer, _> =
        arguments(json!({"name": "n", "limit": null, "items": [{"flag": null, "count": null}]}));
    assert_eq!(
        parsed.ok(),
        Some(Outer {
            name: "n".into(),
            items: vec![Nested {
                flag: None,
                count: 0
            }],
            limit: None
        })
    );
}
