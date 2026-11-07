//! TOOL-U-070: what a tool's schema refused in a call's arguments, named without any of their values, for the text
//! protocol's retry sentence (ADR-0007). A missing property comes before an unexpected one, so a model that wrote
//! `path` for `pattern` learns the real name first; a nullable property left out is no problem (it's filled).
use serde_json::{Value, json};
use std::{error::Error, sync::Arc};
use tqa_tools::{ToolRegistry, builtin};
use tqa_types::{FallbackProblem, JsonType, ProblemName, ProblemPath};

type TestResult = Result<(), Box<dyn Error>>;

fn registry() -> Result<ToolRegistry, Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(builtin::Glob::new()))?;
    registry.register(Arc::new(builtin::WriteFile::new()))?;
    registry.register(Arc::new(builtin::EditFile::new()))?;
    Ok(registry)
}
fn place(text: &str) -> Result<Option<ProblemPath>, Box<dyn Error>> {
    Ok(Some(
        ProblemPath::from_pointer(text).ok_or_else(|| format!("not a place: {text}"))?,
    ))
}
fn name(text: &str) -> Result<ProblemName, Box<dyn Error>> {
    ProblemName::new(text).ok_or_else(|| format!("not a name: {text}").into())
}
fn names(texts: &[&str]) -> Result<Vec<ProblemName>, Box<dyn Error>> {
    texts.iter().map(|text| name(text)).collect()
}

#[test]
fn tool_u_070_each_schema_problem_is_named() -> TestResult {
    let registry = registry()?;
    let cases: [(&str, Value, FallbackProblem); 4] = [
        // An extra `path` beside the pattern: glob's schema takes no other property.
        (
            "glob",
            json!({"pattern": "**/*.py", "path": "src"}),
            FallbackProblem::UnexpectedProperty {
                tool: name("glob")?,
                at: None,
                property: Some(name("path")?),
                allowed: names(&["cursor", "max_results", "pattern"])?,
            },
        ),
        // `path` instead of `pattern`: both errors, and the missing one is named.
        (
            "glob",
            json!({"path": "src"}),
            FallbackProblem::MissingProperty {
                tool: name("glob")?,
                at: None,
                property: name("pattern")?,
                expected: vec![JsonType::String],
            },
        ),
        (
            "write_file",
            json!({"path": "a/b.py", "content": "x = 1\n"}),
            FallbackProblem::MissingProperty {
                tool: name("write_file")?,
                at: None,
                property: name("create_parents")?,
                expected: vec![JsonType::Boolean],
            },
        ),
        (
            "write_file",
            json!({"path": "a.py", "content": "x", "create_parents": "yes"}),
            FallbackProblem::WrongType {
                tool: name("write_file")?,
                at: None,
                property: name("create_parents")?,
                expected: vec![JsonType::Boolean],
            },
        ),
    ];
    for (tool, arguments, problem) in cases {
        assert!(registry.prepare(tool, &arguments).is_err(), "{arguments}");
        assert_eq!(
            registry.schema_problem(tool, &arguments),
            Some(problem),
            "{arguments}"
        );
    }
    Ok(())
}

#[test]
fn tool_u_070_valid_or_filled_arguments_and_unknown_tools_have_no_problem() -> TestResult {
    let registry = registry()?;
    // `cursor` and `max_results` are nullable: left out, they're filled, so no problem.
    assert_eq!(
        registry.schema_problem("glob", &json!({"pattern": "*.rs"})),
        None
    );
    assert_eq!(
        registry.schema_problem(
            "write_file",
            &json!({"path": "a", "content": "", "create_parents": false})
        ),
        None
    );
    assert_eq!(registry.schema_problem("deploy", &json!({})), None);
    Ok(())
}

#[test]
fn tool_u_070_never_a_value_and_only_identifier_names() -> TestResult {
    let registry = registry()?;
    // A value only the call holds: it must never reach the problem.
    let marker = "private-value-marker-7f3a";
    let arguments = json!({"path": "a.py", "content": marker, "create_parents": marker});
    let problem = registry
        .schema_problem("write_file", &arguments)
        .ok_or("a problem")?;
    let written = serde_json::to_string(&problem)?;
    assert!(!written.contains(marker), "{written}");
    assert!(!written.contains("a.py"), "{written}");
    // A key the model wrote that isn't an identifier may be a value in the wrong place: not named.
    let odd = json!({"pattern": "*", "sk live key": 1});
    assert_eq!(
        registry.schema_problem("glob", &odd),
        Some(FallbackProblem::UnexpectedProperty {
            tool: name("glob")?,
            at: None,
            property: None,
            allowed: names(&["cursor", "max_results", "pattern"])?,
        })
    );
    Ok(())
}

// TEST: TOOL-U-070
#[test]
fn tool_u_070_a_nested_problem_names_its_place() -> TestResult {
    let registry = registry()?;
    let edit =
        |item: Value| json!({"path": "a.rs", "edits": [{"old_text": "a", "new_text": "b"}, item]});
    let items = names(&["new_text", "old_text", "replace_all"])?;
    let cases: [(Value, FallbackProblem); 3] = [
        (
            edit(json!({"old_text": "x"})),
            FallbackProblem::MissingProperty {
                tool: name("edit_file")?,
                at: place("/edits/1")?,
                property: name("new_text")?,
                expected: vec![JsonType::String],
            },
        ),
        (
            edit(json!({"old_text": "x", "new_text": "y", "x": 1})),
            FallbackProblem::UnexpectedProperty {
                tool: name("edit_file")?,
                at: place("/edits/1")?,
                property: Some(name("x")?),
                allowed: items,
            },
        ),
        (
            edit(json!({"old_text": "x", "new_text": "y", "replace_all": "yes"})),
            FallbackProblem::WrongType {
                tool: name("edit_file")?,
                at: place("/edits/1")?,
                property: name("replace_all")?,
                expected: vec![JsonType::Boolean, JsonType::Null],
            },
        ),
    ];
    for (arguments, problem) in cases {
        assert!(
            registry.prepare("edit_file", &arguments).is_err(),
            "{arguments}"
        );
        assert_eq!(
            registry.schema_problem("edit_file", &arguments),
            Some(problem),
            "{arguments}"
        );
    }
    Ok(())
}

// TEST: TOOL-U-070
#[test]
fn tool_u_070_a_long_name_the_model_wrote_is_not_kept() -> TestResult {
    let registry = registry()?;
    // Identifier-shaped and 40 bytes, like a token written where a key belongs: dropped. Our own names keep 64.
    let token = format!("ghp_{}", "A1b2".repeat(9));
    assert_eq!(token.len(), 40);
    let mut arguments = json!({"pattern": "*"});
    arguments[&token] = json!(1);
    assert_eq!(
        registry.schema_problem("glob", &arguments),
        Some(FallbackProblem::UnexpectedProperty {
            tool: name("glob")?,
            at: None,
            property: None,
            allowed: names(&["cursor", "max_results", "pattern"])?,
        })
    );
    Ok(())
}

// TEST: TOOL-U-070
#[test]
fn tool_u_070_a_tool_whose_name_is_not_identifier_shaped_has_no_problem() -> TestResult {
    // An MCP tool's name can be longer than a problem's name (64 bytes): no problem, so the retry says the schema in
    // general, as before this change.
    let name = tqa_tools::ToolName::mcp("files", &"t".repeat(60))?;
    let long = name.as_str().to_owned();
    assert_eq!(long.len(), 70);
    let spec = tqa_tools::ToolSpec {
        name,
        description: "Test".into(),
        parameters: json!({"type":"object","required":["path"],"properties":{"path":{"type":"string"}},
            "additionalProperties":false}),
        parallel_safe: true,
        changes_workspace: false,
    };
    let mut registry = ToolRegistry::new();
    registry.register_mcp(Arc::new(tqa_testkit::FakeTool::new(spec, vec![])))?;
    assert!(registry.prepare(&long, &json!({})).is_err());
    assert_eq!(registry.schema_problem(&long, &json!({})), None);
    Ok(())
}

// TEST: TOOL-U-070
#[test]
fn tool_u_070_a_map_key_the_model_wrote_is_never_named() -> TestResult {
    // An MCP tool with map-shaped parameters: their keys are the model's own words.
    let spec = tqa_tools::ToolSpec {
        name: tqa_tools::ToolName::mcp("web", "call")?,
        description: "Test".into(),
        parameters: json!({"type":"object","additionalProperties":false,"properties":{
            "headers":{"type":"object","additionalProperties":{"type":"string"}},
            "servers":{"type":"object","additionalProperties":{"type":"object","required":["url"],
                "properties":{"url":{"type":"string"}}}},
            "env":{"type":"object","additionalProperties":false,
                "patternProperties":{"^[A-Z_]+$":{"type":"string"}}}}}),
        parallel_safe: true,
        changes_workspace: false,
    };
    let mut registry = ToolRegistry::new();
    registry.register_mcp(Arc::new(tqa_testkit::FakeTool::new(spec, vec![])))?;
    let token = format!("ghp_{}", "A1b2".repeat(9));
    assert_eq!(token.len(), 40);
    let mut wrong = json!({"headers": {}});
    wrong["headers"][&token] = json!(5);
    let mut missing = json!({"servers": {}});
    missing["servers"][&token] = json!({});
    // A pattern's key: 40 bytes of the pattern's upper-case letters and underscores.
    let pattern_key = format!("GHP_{}", "ABC_".repeat(9));
    assert_eq!(pattern_key.len(), 40);
    let mut pattern = json!({"env": {}});
    pattern["env"][&pattern_key] = json!(5);
    for (arguments, key) in [(wrong, &token), (missing, &token), (pattern, &pattern_key)] {
        assert!(
            registry.prepare("mcp.web.call", &arguments).is_err(),
            "{arguments}"
        );
        let problem = registry.schema_problem("mcp.web.call", &arguments);
        assert_eq!(
            problem,
            Some(FallbackProblem::OtherSchema {
                tool: name("mcp.web.call")?
            }),
            "{arguments}"
        );
        assert!(!serde_json::to_string(&problem)?.contains(key.as_str()));
    }
    Ok(())
}
