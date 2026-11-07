//! GW-U-165 (SEC-033's follow-up): no log line writes a secret reference except through `secrets::loggable`, so a
//! name shaped like a key (one stored before SEC-033's check, from a seed file) never reaches the log. It reads the
//! gateway's sources: every log macro's field that carries a reference must pass it through `loggable(`.
use std::path::{Path, PathBuf};

/// Each `.rs` file under `dir`, tests excluded (they log nothing of a running gateway).
fn sources(dir: &Path, found: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            sources(&path, found)?;
        } else if path.extension().is_some_and(|extension| extension == "rs")
            && !path.to_string_lossy().ends_with("_tests.rs")
        {
            found.push(path);
        }
    }
    Ok(())
}

/// The fields a log macro may carry a secret reference in.
const FIELDS: [&str; 2] = ["secret_ref", "key_ref"];

// TEST: GW-U-165
#[test]
fn gw_u_165_every_logged_secret_reference_goes_through_loggable()
-> Result<(), Box<dyn std::error::Error>> {
    let mut files = Vec::new();
    sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    )?;
    let mut bare = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file)?;
        let lines: Vec<&str> = text.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            if !line.contains("tracing::") || !line.contains('!') {
                continue;
            }
            // The macro's arguments: up to the line closing it (a log call spans at most a few lines here).
            let call: String = lines[index..lines.len().min(index + 8)]
                .iter()
                .take_while_inclusive(|line| !line.trim_end().ends_with(");"))
                .copied()
                .collect::<Vec<_>>()
                .join(" ");
            for field in bare_fields(&call) {
                bare.push(format!("{}:{}: {field}", file.display(), index + 1));
            }
        }
    }
    assert!(
        bare.is_empty(),
        "secret references logged without secrets::loggable:\n{}",
        bare.join("\n")
    );
    Ok(())
}

/// The reference fields in one log call whose value doesn't go through `loggable`.
fn bare_fields(call: &str) -> Vec<&'static str> {
    let mut bare = Vec::new();
    for field in FIELDS {
        for (at, _) in call.match_indices(field) {
            let before = call[..at].chars().last().unwrap_or(' ');
            if before.is_alphanumeric() || before == '_' {
                continue;
            }
            let Some(value) = call[at + field.len()..].trim_start().strip_prefix('=') else {
                continue;
            };
            let value = value.trim_start().trim_start_matches(['%', '?']);
            if !value.starts_with("loggable(") && !value.starts_with("crate::secrets::loggable(") {
                bare.push(field);
            }
        }
    }
    bare
}

/// `Iterator::take_while` that also yields the first item failing the test (the closing line).
trait TakeWhileInclusive: Iterator + Sized {
    fn take_while_inclusive<P: FnMut(&Self::Item) -> bool>(self, keep: P) -> Inclusive<Self, P> {
        Inclusive {
            inner: self,
            keep,
            done: false,
        }
    }
}
impl<I: Iterator> TakeWhileInclusive for I {}
struct Inclusive<I, P> {
    inner: I,
    keep: P,
    done: bool,
}
impl<I: Iterator, P: FnMut(&I::Item) -> bool> Iterator for Inclusive<I, P> {
    type Item = I::Item;
    fn next(&mut self) -> Option<I::Item> {
        if self.done {
            return None;
        }
        let item = self.inner.next()?;
        if !(self.keep)(&item) {
            self.done = true;
        }
        Some(item)
    }
}
