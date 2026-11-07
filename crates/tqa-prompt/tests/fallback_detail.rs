//! PROMPT-U-024 to PROMPT-U-026: the detailed parse behind `parse_reply`, which says what exactly made a reply
//! unreadable, so the retry sentence can name it (ADR-0007). The failure code is the one `parse_reply` gives, always.
use proptest::prelude::*;
use serde_json::{Value, json};
use tqa_prompt::{
    FallbackCall, FallbackFailure, Rejection, ToolCheck, parse_reply, parse_reply_detailed,
};
use tqa_types::{FallbackProblem, ProblemName};

/// `read_file` needs a string `path`; `shell` needs a string `command`; nothing else exists.
fn check(name: &str, arguments: &Value) -> ToolCheck {
    let field = match name {
        "read_file" => "path",
        "shell" => "command",
        _ => return ToolCheck::Unknown,
    };
    if arguments.get(field).is_some_and(Value::is_string) {
        ToolCheck::Valid
    } else {
        ToolCheck::Invalid
    }
}
fn block(body: &str) -> String {
    format!("```tqa_call\n{body}\n```\n")
}
fn rejected(text: &str) -> Result<Rejection, String> {
    match parse_reply_detailed(text, false, &check) {
        Ok(reply) => Err(format!("parsed: {reply:?}")),
        Err(rejection) => Ok(rejection),
    }
}
fn name(text: &str) -> Result<ProblemName, String> {
    ProblemName::new(text).ok_or_else(|| format!("not a name: {text}"))
}

#[test]
fn prompt_u_024_each_malformed_shape_is_named_and_stays_malformed() -> Result<(), String> {
    let cases = [
        // 13 of 20 in the 16x replay: valid JSON, no name.
        (
            block(r#"{"arguments": {"path": "src/a.py"}}"#),
            FallbackProblem::NoName,
        ),
        (
            block(r#"{"name": "read_file"}"#),
            FallbackProblem::NoArguments,
        ),
        (block("[1]"), FallbackProblem::NotObject),
        (
            block(r#"{"name": "read_file", "arguments": {"path": "a"}, "tool": "read_file"}"#),
            FallbackProblem::ExtraKeys {
                keys: vec![name("tool")?],
            },
        ),
        // A key that isn't an identifier may be text the model meant as a value: never kept.
        (
            block(r#"{"name": "read_file", "arguments": {"path": "a"}, "a b": 1}"#),
            FallbackProblem::ExtraKeys { keys: vec![] },
        ),
        (
            block(r#"{"name": 3, "arguments": {}}"#),
            FallbackProblem::NameNotString,
        ),
    ];
    for (text, problem) in cases {
        let rejection = rejected(&text)?;
        assert_eq!(rejection.failure, FallbackFailure::Malformed, "{text}");
        assert_eq!(rejection.problem, Some(problem), "{text}");
        assert_eq!(rejection.call, None, "{text}");
        assert_eq!(
            parse_reply(&text, false, &check).err(),
            Some(FallbackFailure::Malformed)
        );
    }
    Ok(())
}

/// A malformed reply's JSON error, as `(cut_off, line, column)`; `None` for any other problem.
fn not_json(text: &str) -> Result<Option<(bool, u32, u32)>, String> {
    let rejection = rejected(text)?;
    assert_eq!(rejection.failure, FallbackFailure::Malformed, "{text}");
    Ok(match rejection.problem {
        Some(FallbackProblem::NotJson {
            cut_off,
            line,
            column,
        }) => Some((cut_off, line, column)),
        _ => None,
    })
}

#[test]
fn prompt_u_024_a_json_error_names_its_line_and_whether_the_reply_was_cut() -> Result<(), String> {
    // A long write_file body cut off before the object closes (the reply ended inside its string).
    let cut = "```tqa_call\n{\"name\": \"write_file\", \"arguments\": {\"path\": \"a.py\", \"content\": \"def f():\\n    return 1";
    assert!(matches!(not_json(cut)?, Some((true, 1, _))), "{cut}");
    // A stray character on line 3 is a syntax error there, not a cut.
    let stray = block("{\n\"name\": \"read_file\",\n\"arguments\": {\"path\": \"a\"}}}");
    assert!(matches!(not_json(&stray)?, Some((false, 3, _))), "{stray}");
    // A blank line before the object counts: the line is the one the model wrote inside its block.
    let blank = block("\n{\n\"name\": \"read_file\",\n\"arguments\": {\"path\": \"a\"}}}");
    assert!(matches!(not_json(&blank)?, Some((false, 4, _))), "{blank}");
    // A raw line break inside a string (a long body written as it would be in a file) is a syntax error too, where
    // the parser met it: the start of the next line.
    let raw = block(
        "{\"name\": \"write_file\", \"arguments\": {\"path\": \"a.py\", \"content\": \"a\nb\"}}",
    );
    assert_eq!(not_json(&raw)?, Some((false, 2, 0)), "{raw}");
    Ok(())
}

#[test]
fn prompt_u_025_rejections_carry_the_failing_call_and_the_count() -> Result<(), String> {
    let unknown = rejected(&block(r#"{"name": "deploy", "arguments": {"to": "prod"}}"#))?;
    assert_eq!(unknown.failure, FallbackFailure::UnknownTool);
    assert_eq!(
        unknown.problem,
        Some(FallbackProblem::UnknownTool {
            name: Some(name("deploy")?)
        })
    );
    assert_eq!(
        unknown.call,
        Some(Box::new(FallbackCall {
            name: "deploy".into(),
            arguments: json!({"to": "prod"}),
        }))
    );
    let unnamed = rejected(&block(r#"{"name": "rm -rf /", "arguments": {}}"#))?;
    assert_eq!(
        unnamed.problem,
        Some(FallbackProblem::UnknownTool { name: None })
    );
    // A name the model wrote keeps 32 bytes: a 40-byte identifier-shaped token is not named, as a tool or a key.
    let token = format!("ghp_{}", "A1b2".repeat(9));
    let long = rejected(&block(&format!(
        r#"{{"name": "{token}", "arguments": {{}}}}"#
    )))?;
    assert_eq!(
        long.problem,
        Some(FallbackProblem::UnknownTool { name: None })
    );
    let key = rejected(&block(&format!(
        r#"{{"name": "read_file", "arguments": {{"path": "a"}}, "{token}": 1, "tool": 2}}"#
    )))?;
    assert_eq!(
        key.problem,
        Some(FallbackProblem::ExtraKeys {
            keys: vec![name("tool")?]
        })
    );

    // Schema: the parser only knows the tool refused the arguments; the caller asks the registry what was wrong.
    let schema = rejected(&block(r#"{"name": "read_file", "arguments": {"path": 7}}"#))?;
    assert_eq!(schema.failure, FallbackFailure::Schema);
    assert_eq!(schema.problem, None);
    assert_eq!(
        schema.call,
        Some(Box::new(FallbackCall {
            name: "read_file".into(),
            arguments: json!({"path": 7}),
        }))
    );
    let arguments = rejected(&block(r#"{"name": "read_file", "arguments": "a.rs"}"#))?;
    assert_eq!(
        (arguments.failure, arguments.problem, arguments.call),
        (FallbackFailure::Arguments, None, None)
    );

    let one = block(r#"{"name": "shell", "arguments": {"command": "ls"}}"#);
    let too_many = rejected(&one.repeat(3))?;
    assert_eq!(too_many.failure, FallbackFailure::TooMany);
    assert_eq!(
        too_many.problem,
        Some(FallbackProblem::TooMany { count: 3, limit: 1 })
    );
    let Err(parallel) = parse_reply_detailed(&one.repeat(65), true, &check) else {
        return Err("65 calls parsed".into());
    };
    assert_eq!(
        parallel.problem,
        Some(FallbackProblem::TooMany {
            count: 65,
            limit: 64
        })
    );

    // Several bad blocks: the first is described, the one that decides the code.
    let first = format!(
        "{}{}",
        block(r#"{"arguments": {"path": "a"}}"#),
        block(r#"{"name": "deploy", "arguments": {}}"#)
    );
    let rejection = rejected(&first)?;
    assert_eq!(
        (rejection.failure, rejection.problem),
        (FallbackFailure::Malformed, Some(FallbackProblem::NoName))
    );
    Ok(())
}

/// Inputs from the `fallback`, `text_calls` and `json_fence` suites' shapes, and the 16x replay's.
fn corpus() -> Vec<String> {
    let mut texts: Vec<String> = [
        "plain answer",
        "",
        "{\"name\": \"read_file\", \"arguments\": {\"path\": \"a\"}}",
        "{'name': 'read_file', arguments: {path: 'a',},}",
        "```json\n{\"name\": \"read_file\", \"arguments\": {\"path\": 7}}\n```",
        "```json\n{\"a\": 1}\n```",
        "```\n{\"name\": \"shell\", \"arguments\": {\"command\": \"ls\"}}\n```",
        "```tool_call\n{\"name\": \"read_file\" \"arguments\": }\n```",
        "```tool_call\n{\"name\": \"read_file\"}\n```",
        "Let me read it.\n<tool_call>\n<function=read_file>\n<parameter=path>\na\n</parameter>\n</function>\n</tool_call>",
        "Done.\n</tool_call>",
        "[The last reply could not be read as a tool call: the tqa_call block is not valid JSON. Reply again with each call in its own tqa_call block.]",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    for body in [
        r#"{"arguments": {"path": "a"}}"#,
        r#"{"name": "deploy", "arguments": {}}"#,
        r#"{"name": "read_file", "arguments": "a"}"#,
        r#"{"name": "read_file", "arguments": {"path": "a"}, "tool": 1}"#,
        r#"{"name": 3, "arguments": {}}"#,
        "[1]",
        "{\"name\": \"write_file\", \"arguments\": {\"content\": \"x",
    ] {
        texts.push(block(body));
        texts.push(format!("Now I'll edit it.\n{}", block(body)));
    }
    texts.push(block(r#"{"name": "shell", "arguments": {"command": "a"}}"#).repeat(2));
    texts
}

fn agrees(text: &str) {
    for parallel in [false, true] {
        assert_eq!(
            parse_reply(text, parallel, &check),
            parse_reply_detailed(text, parallel, &check).map_err(|rejection| rejection.failure),
            "{text:?}"
        );
    }
}

#[test]
fn prompt_u_026_the_detailed_parse_decides_exactly_as_parse_reply() {
    for text in corpus() {
        agrees(&text);
    }
}

proptest! {
    #[test]
    fn prompt_u_026_agreement_holds_for_any_text(text in "(```(tqa_call|tool_call|json)?\n)?[{}\\[\\]\":,a-z0-9 \n]{0,80}(\n```)?") {
        agrees(&text);
    }
}
