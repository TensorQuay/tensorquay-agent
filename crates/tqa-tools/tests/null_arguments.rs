//! TOOL-U-034: a strict model sends `null` for an optional argument it doesn't use; the registry normalises that to
//! absent, at every depth the schema describes, so a call's recorded arguments are the tool's real ones. An empty or
//! blank string for an optional string argument is absent too (small models fill one with `""`; a 13h Gate 5 demo
//! ended on three refusals of `read_file`'s `cursor: ""`), swept over every tool family's schema. So is a null-word
//! (`None`, `null`, `undefined`, any case) where the field can't really hold that word: a cursor (an opaque token) and
//! a closed list (an `enum`); a free string keeps it (the lead's decision of 2 Oct, after the demo rerun ended on
//! `grep`'s `cursor: "None"`).
use serde_json::json;
use std::{error::Error, sync::Arc};
#[path = "support/context.rs"]
mod support;
use tqa_tools::{ToolRegistry, builtin};

type TestResult = Result<(), Box<dyn Error>>;

fn registry() -> Result<ToolRegistry, Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(builtin::ReadFile::new()))?;
    registry.register(Arc::new(builtin::EditFile::new()))?;
    registry.register(Arc::new(builtin::Grep::new()))?;
    Ok(registry)
}

#[test]
fn tool_u_034_nulls_for_optional_arguments_are_dropped() -> TestResult {
    let registry = registry()?;
    let mut read = json!({"path": "../x", "cursor": null, "start_line": null, "end_line": null});
    registry.normalize_arguments("read_file", &mut read);
    assert_eq!(read, json!({"path": "../x"}));
    // Nested, as the schema describes it; given values stay.
    let mut edit = json!({"path": "a", "edits": [
        {"old_text": "x", "new_text": "y", "replace_all": null},
        {"old_text": "p", "new_text": "q", "replace_all": true}]});
    registry.normalize_arguments("edit_file", &mut edit);
    assert_eq!(
        edit,
        json!({"path": "a", "edits": [{"old_text": "x", "new_text": "y"}, {"old_text": "p", "new_text": "q", "replace_all": true}]})
    );
    // Arguments without nulls are unchanged.
    let mut plain = json!({"pattern": "alpha", "max_results": 5});
    registry.normalize_arguments("grep", &mut plain);
    assert_eq!(plain, json!({"pattern": "alpha", "max_results": 5}));
    Ok(())
}

#[test]
fn tool_u_034_a_null_for_a_required_argument_stays_and_is_refused() -> TestResult {
    let registry = registry()?;
    let mut read = json!({"path": null});
    registry.normalize_arguments("read_file", &mut read);
    assert_eq!(
        read,
        json!({"path": null}),
        "a required, non-nullable argument keeps its null"
    );
    assert!(
        registry.prepare("read_file", &read).is_err(),
        "and validation refuses it"
    );
    // An unknown tool's arguments are left alone.
    let mut other = json!({"a": null});
    registry.normalize_arguments("nope", &mut other);
    assert_eq!(other, json!({"a": null}));
    Ok(())
}

#[test]
fn tool_u_034_normalised_calls_still_validate() -> TestResult {
    let registry = registry()?;
    let mut read = json!({"path": "a.rs", "cursor": null, "start_line": null, "end_line": null});
    registry.normalize_arguments("read_file", &mut read);
    registry.prepare("read_file", &read)?;
    Ok(())
}

/// An external (MCP-style) tool: its schema's nullable properties are its own, and a `null` it's sent is meant.
#[test]
fn tool_u_034_an_external_tools_null_is_kept_exactly() -> TestResult {
    let spec = tqa_tools::ToolSpec {
        name: tqa_tools::ToolName::new("file_issue")?,
        description: "File an issue".into(),
        parameters: json!({"type":"object","required":["title","assignee"],
            "properties":{"title":{"type":"string"},"assignee":{"type":["string","null"]},"labels":{"type":["array","null"]}}}),
        parallel_safe: false,
        changes_workspace: false,
    };
    let mut registry = registry()?;
    registry.register(Arc::new(tqa_testkit::FakeTool::new(spec, vec![])))?;
    let mut args = json!({"title": "t", "assignee": null, "labels": null});
    registry.normalize_arguments("file_issue", &mut args);
    assert_eq!(
        args,
        json!({"title": "t", "assignee": null, "labels": null}),
        "sent exactly as the model wrote it"
    );
    registry.prepare("file_issue", &args)?;
    Ok(())
}

/// Every optional (nullable) string argument of `tool`'s schema, sent as `""` and as blanks, is dropped as absent;
/// a real value stays. Answers how many properties it checked.
fn blank_optionals_are_absent(
    registry: &ToolRegistry,
    tool: &str,
    required: &serde_json::Value,
) -> usize {
    let spec = registry.specs().find(|spec| spec.name.as_str() == tool);
    let properties = spec
        .and_then(|spec| spec.parameters["properties"].as_object())
        .cloned()
        .unwrap_or_default();
    let optional_strings: Vec<&String> = properties
        .iter()
        .filter(|(_, schema)| {
            let kinds = &schema["type"];
            let has = |kind: &str| {
                kinds == kind
                    || kinds
                        .as_array()
                        .is_some_and(|all| all.iter().any(|k| k == kind))
            };
            has("null") && has("string")
        })
        .map(|(name, _)| name)
        .collect();
    for name in &optional_strings {
        let mut base = required.clone();
        if let Some(members) = base.as_object_mut() {
            members.remove(name.as_str());
        }
        for blank in ["", "  "] {
            let mut args = base.clone();
            args[name.as_str()] = json!(blank);
            registry.normalize_arguments(tool, &mut args);
            assert_eq!(args, base, "{tool}.{name} = {blank:?} is absent");
        }
        let mut given = base.clone();
        given[name.as_str()] = json!("x");
        registry.normalize_arguments(tool, &mut given);
        assert_eq!(
            given[name.as_str()],
            "x",
            "{tool}.{name}: a real value stays"
        );
        let closed = name.as_str() == "cursor" || properties[name.as_str()].get("enum").is_some();
        for word in ["None", "null", "NULL", "undefined"] {
            let mut args = base.clone();
            args[name.as_str()] = json!(word);
            registry.normalize_arguments(tool, &mut args);
            if closed {
                assert_eq!(
                    args, base,
                    "{tool}.{name} = {word:?} is absent: it can't be a real value"
                );
            } else {
                assert_eq!(
                    args[name.as_str()],
                    word,
                    "{tool}.{name} is a free string: {word:?} stays"
                );
            }
        }
    }
    optional_strings.len()
}

#[test]
fn tool_u_034_files_blank_optional_strings_are_absent() -> TestResult {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(builtin::ReadFile::new()))?;
    registry.register(Arc::new(builtin::WriteFile::new()))?;
    registry.register(Arc::new(builtin::EditFile::new()))?;
    assert_eq!(
        blank_optionals_are_absent(&registry, "read_file", &json!({"path": "a"})),
        1,
        "cursor"
    );
    blank_optionals_are_absent(
        &registry,
        "write_file",
        &json!({"path": "a", "content": ""}),
    );
    // A required string keeps its empty value: an empty file is a real request.
    let mut write = json!({"path": "a", "content": ""});
    registry.normalize_arguments("write_file", &mut write);
    assert_eq!(write, json!({"path": "a", "content": ""}));
    Ok(())
}

#[test]
fn tool_u_034_search_blank_optional_strings_are_absent() -> TestResult {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(builtin::Grep::new()))?;
    registry.register(Arc::new(builtin::Glob::new()))?;
    assert!(blank_optionals_are_absent(&registry, "grep", &json!({"pattern": "alpha"})) >= 4);
    assert_eq!(
        blank_optionals_are_absent(&registry, "glob", &json!({"pattern": "*.rs"})),
        1,
        "cursor"
    );
    Ok(())
}

#[test]
fn tool_u_034_shell_blank_optional_strings_are_absent() -> TestResult {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(builtin::Shell::new()))?;
    assert_eq!(
        blank_optionals_are_absent(&registry, "shell", &json!({"command": "ls"})),
        1,
        "cwd"
    );
    Ok(())
}

/// A channel client the sweep never calls.
struct Unused;
#[async_trait::async_trait]
impl builtin::channel::ChannelClient for Unused {
    async fn channels(
        &self,
        _: &tokio_util::sync::CancellationToken,
    ) -> Result<Vec<builtin::channel::ChannelSummary>, builtin::channel::ChannelFailure> {
        Ok(Vec::new())
    }
    async fn read(
        &self,
        _: &str,
        _: &builtin::channel::ReadRequest,
        _: &tokio_util::sync::CancellationToken,
    ) -> Result<builtin::channel::ChannelPage, builtin::channel::ChannelFailure> {
        Err(builtin::channel::ChannelFailure::new(
            "unavailable",
            "unused",
        ))
    }
    async fn post(
        &self,
        _: &str,
        _: &tqa_types::ChannelPost,
        _: &str,
        _: &tokio_util::sync::CancellationToken,
    ) -> Result<tqa_types::ChannelMessage, builtin::channel::ChannelFailure> {
        Err(builtin::channel::ChannelFailure::new(
            "unavailable",
            "unused",
        ))
    }
    async fn claim(
        &self,
        _: &str,
        _: u64,
        _: &str,
        _: &tokio_util::sync::CancellationToken,
    ) -> Result<builtin::channel::Claimed, builtin::channel::ChannelFailure> {
        Err(builtin::channel::ChannelFailure::new(
            "unavailable",
            "unused",
        ))
    }
}

#[test]
fn tool_u_034_channel_blank_optional_strings_are_absent() -> TestResult {
    let mut registry = ToolRegistry::new();
    for tool in builtin::channel::tools(Arc::new(Unused)) {
        registry.register(tool)?;
    }
    let names: Vec<String> = registry
        .specs()
        .map(|spec| spec.name.as_str().to_owned())
        .collect();
    let mut checked = 0;
    for name in &names {
        checked += blank_optionals_are_absent(&registry, name, &json!({"channel": "c"}));
    }
    assert!(
        checked >= 3,
        "the channel tools' optional strings were swept: {checked}"
    );
    Ok(())
}

/// A refusal names the field and the rule, so a model can correct its next call: a forged search cursor says which
/// argument and what to do instead.
#[tokio::test]
async fn tool_u_034_a_refused_cursor_names_the_field_and_the_way_out() -> TestResult {
    let fixture = tqa_testkit::workspace_fixture()?;
    let cx = support::context(&fixture);
    for (tool, args) in [
        (
            Arc::new(builtin::Grep::new()) as Arc<dyn tqa_tools::Tool>,
            json!({"pattern": "a", "cursor": "e30"}),
        ),
        (
            Arc::new(builtin::Glob::new()),
            json!({"pattern": "*", "cursor": "%%%"}),
        ),
    ] {
        let error = tool
            .call(args.clone(), &cx)
            .await
            .err()
            .ok_or_else(|| format!("accepted: {args}"))?;
        assert_eq!(
            error.model_message(),
            "invalid_args: /cursor: not a cursor from an earlier call of this tool; to start from the beginning, \
             leave cursor out",
            "{args}"
        );
    }
    Ok(())
}

/// grep's `file_type` is a closed list its code checks (no `enum` in the schema), so grep itself takes a null-word there
/// as absent, as it does in its cursor; the demo rerun's refused call now reads like the same call without them.
#[tokio::test]
async fn tool_u_034_search_null_words_in_closed_fields_are_absent() -> TestResult {
    let fixture = tqa_testkit::workspace_fixture()?;
    let cx = support::context(&fixture);
    let refused_on_2_oct = json!({"case_insensitive": false, "context_lines": 0, "cursor": "None", "file_type": "None",
        "fixed_string": false, "max_results": 20, "output_mode": "content", "path": "hello.txt", "pattern": "a"});
    let mut plain = refused_on_2_oct.clone();
    for name in ["cursor", "file_type"] {
        plain.as_object_mut().ok_or("args")?.remove(name);
    }
    let grep = builtin::Grep::new();
    let with = tqa_tools::Tool::call(&grep, refused_on_2_oct, &cx)
        .await?
        .text;
    let without = tqa_tools::Tool::call(&grep, plain, &cx).await?.text;
    assert_eq!(with, without);
    for cursor in ["None", "null", "Undefined"] {
        let glob = tqa_tools::Tool::call(
            &builtin::Glob::new(),
            json!({"pattern": "*", "cursor": cursor}),
            &cx,
        )
        .await;
        assert!(glob.is_ok(), "glob cursor {cursor:?}: {glob:?}");
    }
    Ok(())
}

/// A closed list that holds a null-word as a real value keeps it, in any case: an enum of `["none", "gzip"]` means
/// "None" (no compression); a null-word it doesn't list is still absent.
#[test]
fn tool_u_034_a_closed_list_that_lists_a_null_word_keeps_it() -> TestResult {
    let spec = tqa_tools::ToolSpec {
        name: tqa_tools::ToolName::new("pack")?,
        description: "Test tool".into(),
        parameters: json!({"type": "object", "additionalProperties": false, "required": ["path", "compression", "level"],
        "properties": {
            "path": {"type": "string"},
            "compression": {"type": ["string", "null"], "enum": ["none", "gzip", null]},
            "level": {"type": ["string", "null"], "enum": ["fast", "best", null]}
        }}),
        parallel_safe: true,
        changes_workspace: false,
    };
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(
        tqa_testkit::FakeTool::new(spec, vec![]).with_null_means_absent(),
    ))?;
    for kept in ["none", "None", "NONE"] {
        let mut args = json!({"path": "a", "compression": kept, "level": "fast"});
        registry.normalize_arguments("pack", &mut args);
        assert_eq!(args["compression"], kept, "a listed null-word is a value");
    }
    let mut args = json!({"path": "a", "compression": "null", "level": "None"});
    registry.normalize_arguments("pack", &mut args);
    assert_eq!(
        args,
        json!({"path": "a"}),
        "null-words the lists don't hold are absent"
    );
    Ok(())
}

/// The rule's edges, each leaving the argument as sent: a schema without properties, an argument the schema doesn't
/// name, a list whose schema doesn't describe its items, a property without a type, a blank for one that takes only
/// null, a null for one that isn't nullable, and a blank for a nullable one that takes no string.
#[test]
fn tool_u_034_arguments_the_schema_doesnt_describe_are_left_as_sent() -> TestResult {
    let spec = |name: &str,
                parameters: serde_json::Value|
     -> Result<tqa_tools::ToolSpec, Box<dyn Error>> {
        Ok(tqa_tools::ToolSpec {
            name: tqa_tools::ToolName::new(name)?,
            description: "Test tool".into(),
            parameters,
            parallel_safe: true,
            changes_workspace: false,
        })
    };
    let mut registry = ToolRegistry::new();
    for (name, parameters) in [
        ("open_object", json!({"type": "object"})),
        (
            "described",
            json!({"type": "object", "properties": {
                "tags": {"type": ["array", "null"]},
                "loose": {"enum": ["a", "None", null]},
                "only_null": {"type": "null"},
                "either": {"type": ["string", "integer"]},
                "count": {"type": ["integer", "null"]}
            }}),
        ),
    ] {
        let tool =
            tqa_testkit::FakeTool::new(spec(name, parameters)?, vec![]).with_null_means_absent();
        registry.register(Arc::new(tool))?;
    }
    let sent = json!({"anything": "None", "blank": ""});
    let mut args = sent.clone();
    registry.normalize_arguments("open_object", &mut args);
    assert_eq!(
        args, sent,
        "a schema without properties describes no argument"
    );
    let sent = json!({"extra": null, "tags": ["None", ""], "loose": "None", "only_null": "", "either": null, "count": ""});
    let mut args = sent.clone();
    registry.normalize_arguments("described", &mut args);
    assert_eq!(
        args, sent,
        "undescribed, untyped, null-only, non-nullable and non-string properties are left as sent"
    );
    // A property that takes only null, sent as null, is absent like any nullable one.
    let mut args = json!({"only_null": null});
    registry.normalize_arguments("described", &mut args);
    assert_eq!(args, json!({}));
    Ok(())
}
