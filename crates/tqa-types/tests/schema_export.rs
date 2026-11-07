//! G-08 schema export, determinism and mismatch diagnostics.

use std::{error::Error, fs, path::PathBuf, process::Command};

struct FixtureDirectory(PathBuf);

impl FixtureDirectory {
    fn new(case: &str) -> Result<Self, std::io::Error> {
        let path =
            std::env::temp_dir().join(format!("tqa-schema-test-{}-{case}", std::process::id()));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for FixtureDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn exporter() -> Command {
    Command::new(env!("CARGO_BIN_EXE_export-schema"))
}

#[test]
fn proto_c_001_schema_export_is_deterministic_and_checks_a_diff() -> Result<(), Box<dyn Error>> {
    let directory = FixtureDirectory::new("export")?;
    let path = directory.0.join("schema.json");
    assert!(exporter().arg(&path).output()?.status.success());
    let original = fs::read(&path)?;
    assert!(exporter().arg(&path).output()?.status.success());
    assert_eq!(fs::read(&path)?, original);
    assert!(
        exporter()
            .arg("--check")
            .arg(&path)
            .output()?
            .status
            .success()
    );
    fs::write(&path, "{}\n")?;
    let mismatch = exporter().arg("--check").arg(&path).output()?;
    assert_eq!(mismatch.status.code(), Some(1));
    let output = String::from_utf8(mismatch.stderr)?;
    assert!(output.contains("--- ") && output.contains("+++ ") && output.contains("@@ "));
    assert_eq!(fs::read_to_string(&path)?, "{}\n");
    assert!(exporter().arg(&path).output()?.status.success());
    assert_eq!(fs::read(&path)?, original);
    Ok(())
}

#[test]
fn proto_c_001_schema_diff_preserves_empty_files_and_missing_newlines() -> Result<(), Box<dyn Error>>
{
    let directory = FixtureDirectory::new("line-endings")?;
    let path = directory.0.join("schema.json");
    for (contents, expected_start, missing_newline) in
        [("", "@@ -0,0 +1,", false), ("{}", "@@ -1,1 +1,", true)]
    {
        fs::write(&path, contents)?;
        let mismatch = exporter().arg("--check").arg(&path).output()?;
        assert_eq!(mismatch.status.code(), Some(1));
        let diff = String::from_utf8(mismatch.stderr)?;
        assert!(
            diff.lines()
                .nth(2)
                .is_some_and(|line| line.starts_with(expected_start))
        );
        assert_eq!(
            diff.contains("\\ No newline at end of file"),
            missing_newline
        );
        assert_eq!(fs::read_to_string(&path)?, contents);
    }
    Ok(())
}

#[test]
fn proto_c_001_exporter_rejects_bad_arguments_and_io_failures() -> Result<(), Box<dyn Error>> {
    for arguments in [
        vec![],
        vec!["--check"],
        vec!["--unknown"],
        vec!["a", "b", "c"],
    ] {
        assert!(!exporter().args(arguments).output()?.status.success());
    }
    let directory = FixtureDirectory::new("errors")?;
    let path = directory.0.join("missing").join("schema.json");
    assert!(!exporter().arg(&path).output()?.status.success());
    assert!(
        !exporter()
            .arg("--check")
            .arg(&path)
            .output()?
            .status
            .success()
    );
    Ok(())
}

/// The string keywords a strict draft-07 validator (ajv's `strictTypes`) only accepts where a string type applies.
const STRING_KEYWORDS: [&str; 3] = ["pattern", "minLength", "maxLength"];

/// Collects every path a strict draft-07 validator refuses: a string keyword without a string type (its own or one
/// inherited through a combinator), or a required name the same schema does not define.
fn untyped_strings(
    value: &serde_json::Value,
    inherited: Option<&serde_json::Value>,
    path: &str,
    found: &mut Vec<String>,
) {
    let serde_json::Value::Object(object) = value else {
        if let serde_json::Value::Array(items) = value {
            for (index, item) in items.iter().enumerate() {
                untyped_strings(item, None, &format!("{path}/{index}"), found);
            }
        }
        return;
    };
    let own = object.get("type").or(inherited);
    let stringy = own.is_some_and(|kind| {
        kind == "string"
            || kind
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|k| k == "string"))
    });
    if !stringy
        && STRING_KEYWORDS
            .iter()
            .any(|keyword| object.contains_key(*keyword))
    {
        found.push(path.to_owned());
    }
    // ajv's strictRequired: a required name must be defined in the same schema's properties.
    let defined = object
        .get("properties")
        .and_then(serde_json::Value::as_object);
    for name in object
        .get("required")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
    {
        if !defined.is_some_and(|properties| {
            name.as_str()
                .is_some_and(|name| properties.contains_key(name))
        }) {
            found.push(format!("{path}/required/{name}"));
        }
    }
    for (key, child) in object {
        let child_path = format!("{path}/{key}");
        match key.as_str() {
            // A combinator's branches validate the same value, so the parent's type applies to them.
            "anyOf" | "oneOf" | "allOf" => {
                for (index, branch) in child.as_array().into_iter().flatten().enumerate() {
                    untyped_strings(branch, own, &format!("{child_path}/{index}"), found);
                }
            }
            "not" => untyped_strings(child, own, &child_path, found),
            // Property names are always strings, as JSON Schema defines them.
            "propertyNames" => untyped_strings(
                child,
                Some(&serde_json::json!("string")),
                &child_path,
                found,
            ),
            "properties" | "definitions" | "patternProperties" => {
                for (name, schema) in child.as_object().into_iter().flatten() {
                    untyped_strings(schema, None, &format!("{child_path}/{name}"), found);
                }
            }
            "const" | "enum" | "default" | "examples" | "required" => {}
            _ => untyped_strings(child, None, &child_path, found),
        }
    }
}

#[test]
fn proto_c_001_the_schema_keeps_the_strict_rules_so_strict_validators_compile_it() {
    let schema = tqa_types::protocol_schema();
    let mut found = Vec::new();
    untyped_strings(&schema, None, "#", &mut found);
    assert!(found.is_empty(), "breaks the strict rules: {found:#?}");
    // Positive control: the walk finds one planted where only a combinator's parent could give it a type.
    let planted = serde_json::json!({"anyOf":[{"properties":{"x":{"pattern":"^a$"}}}]});
    let mut control = Vec::new();
    untyped_strings(&planted, None, "#", &mut control);
    assert_eq!(control, ["#/anyOf/0/properties/x"]);
    let orphan = serde_json::json!({"anyOf":[{"not":{"required":["version"]}}]});
    let mut orphans = Vec::new();
    untyped_strings(&orphan, None, "#", &mut orphans);
    assert_eq!(
        orphans,
        ["#/anyOf/0/not/required/\"version\""],
        "a required name with no property is found"
    );
    let typed = serde_json::json!({"type":"string","anyOf":[{"pattern":"^a$"}]});
    let mut none = Vec::new();
    untyped_strings(&typed, None, "#", &mut none);
    assert!(none.is_empty(), "an inherited string type counts: {none:?}");
}
