//! SBX-U-001: the runtime image states the protocol revision this source tree speaks, from one constant.
use std::{error::Error, path::Path};
use tqa_types::{PROTOCOL_REVISION, RUNTIME_REVISION_LABEL};

/// The build stages of a Dockerfile, each with its own instruction lines (continuations joined).
fn stages(dockerfile: &str) -> Vec<(String, Vec<String>)> {
    let mut stages: Vec<(String, Vec<String>)> = Vec::new();
    let mut pending = String::new();
    for line in dockerfile.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        pending.push_str(line.trim_end_matches('\\'));
        pending.push(' ');
        if line.ends_with('\\') {
            continue;
        }
        let instruction = std::mem::take(&mut pending);
        let words: Vec<&str> = instruction.split_whitespace().collect();
        match words.as_slice() {
            ["FROM", .., "AS", name] => stages.push(((*name).to_owned(), Vec::new())),
            ["FROM", ..] => stages.push((String::new(), Vec::new())),
            [] => {}
            _ => {
                if let Some((_, lines)) = stages.last_mut() {
                    lines.push(words.join(" "));
                }
            }
        }
    }
    stages
}

#[test]
fn sbx_u_001_the_runtime_image_label_matches_the_protocol_revision() -> Result<(), Box<dyn Error>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../images/runtime/Dockerfile");
    let dockerfile = std::fs::read_to_string(path)?;
    let expected = format!("{RUNTIME_REVISION_LABEL}=\"{PROTOCOL_REVISION}\"");
    let stages = stages(&dockerfile);
    // The label goes with the runtime binary, which each shipped target copies last (REL-U-011), so a cached OS
    // stage carries neither and every copy of the label must equal the constant.
    let binary = "COPY --from=build /build/target/release/tqa-runtime /usr/local/bin/tqa-runtime";
    let is_label =
        |line: &&String| line.starts_with("LABEL ") && line.contains(RUNTIME_REVISION_LABEL);
    for (stage, lines) in &stages {
        for line in lines.iter().filter(is_label) {
            assert!(
                line.contains(&expected),
                "{stage}: {line} must be {expected}"
            );
        }
        assert_eq!(
            lines.iter().any(|line| is_label(&line)),
            lines.iter().any(|line| line == binary),
            "{stage}: the label is set where the runtime binary is copied, and only there"
        );
    }
    for target in ["base", "workstation", "desktop"] {
        let (_, lines) = stages
            .iter()
            .find(|(stage, _)| stage == target)
            .ok_or(format!("no {target} stage"))?;
        assert!(
            lines.iter().any(|line| is_label(&line)),
            "{target} must carry {expected} itself"
        );
    }
    Ok(())
}

#[test]
fn sbx_u_001_the_stage_reader_joins_continuations_and_skips_comments() {
    let parsed = stages(
        "FROM a AS one\n# LABEL x=\"1\"\nLABEL y=\"2\" \\\n      z=\"3\"\nFROM b\nFROM one AS two\nRUN true\n",
    );
    assert_eq!(
        parsed,
        [
            ("one".to_owned(), vec!["LABEL y=\"2\" z=\"3\"".to_owned()]),
            (String::new(), vec![]),
            ("two".to_owned(), vec!["RUN true".to_owned()]),
        ]
    );
}
