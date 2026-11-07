//! TK-U-050: every scenario a stack gives the fake model loads by the fake model's own rules. A fixture the loader
//! refuses stops the fake model at start, and with it the whole stack suite; twice a tool call without `split_into`
//! did that (15b's task page, then c16's task page and Desktop).

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
};
use tqa_testkit::standalone::Scenario;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The committed scenarios: the stack fixtures and the demo's own.
fn scenarios() -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut paths = Vec::new();
    for dir in ["deploy/tests/fixtures", "crates/tqa-testkit/scenarios"] {
        for entry in fs::read_dir(repo().join(dir))? {
            let path = entry?.path();
            if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                paths.push(path);
            }
        }
    }
    paths.sort();
    Ok(paths)
}

// TEST: TK-U-050
#[test]
fn tk_u_050_every_committed_scenario_loads() -> Result<(), Box<dyn Error>> {
    let paths = scenarios()?;
    let refused: Vec<String> = paths
        .iter()
        .filter_map(|path| {
            Scenario::load(path)
                .err()
                .map(|error| format!("{}: {error}", path.display()))
        })
        .collect();
    assert!(refused.is_empty(), "the fake model refuses: {refused:#?}");
    // Each fixture a Compose overlay mounts for the fake model is one of them.
    for overlay in fs::read_dir(repo().join("deploy/compose"))? {
        let overlay = overlay?.path();
        for line in fs::read_to_string(&overlay)?.lines() {
            if let Some(source) = line.trim().strip_prefix("source: ../tests/fixtures/") {
                let fixture = repo().join("deploy/tests/fixtures").join(source);
                assert!(
                    paths.contains(&fixture),
                    "{} mounts {source}, which isn't checked",
                    overlay.display()
                );
            }
        }
    }
    Ok(())
}

// TEST: TK-U-050
#[test]
fn tk_u_050_a_tool_call_without_split_into_is_refused() -> Result<(), Box<dyn Error>> {
    let dir = std::env::temp_dir().join(format!("tk-u-050-{}", std::process::id()));
    fs::create_dir_all(&dir)?;
    let path = dir.join("scenario.json");
    let call = |split: &str| {
        format!(
            r#"{{"model": "fixture", "responses": [{{"events": [{{"type": "tool_call", "name": "shell", "arguments": {{"command": "true"}}{split}}}], "usage": {{"prompt_tokens": 1, "completion_tokens": 1}}}}]}}"#
        )
    };
    fs::write(&path, call(r#", "split_into": 1"#))?;
    let kept = Scenario::load(&path).map(|_| ());
    fs::write(&path, call(""))?;
    let refused = Scenario::load(&path).map(|_| ());
    fs::remove_dir_all(&dir)?;
    kept?;
    let error = refused
        .err()
        .ok_or("a tool call without split_into loaded")?;
    assert!(
        error.to_string().contains("missing field `split_into`"),
        "{error}"
    );
    Ok(())
}
