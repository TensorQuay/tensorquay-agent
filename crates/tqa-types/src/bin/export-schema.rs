//! Export or check the deterministic protocol-v1 JSON Schema, or with `--decision` the `decision/v1` one.

use std::{error::Error, ffi::OsString, fmt::Write as _, fs, path::PathBuf, process::ExitCode};

use tqa_types::{decision_schema, protocol_schema};

fn run(arguments: &[OsString]) -> Result<(), Box<dyn Error>> {
    let (decision, arguments) = match arguments {
        [flag, rest @ ..] if flag == "--decision" => (true, rest),
        _ => (false, arguments),
    };
    let (check, path) = match arguments {
        [path] if !path.to_string_lossy().starts_with('-') => (false, PathBuf::from(path)),
        [flag, path] if flag == "--check" => (true, PathBuf::from(path)),
        _ => {
            return Err(
                "usage: export-schema [--decision] <path> | [--decision] --check <path>".into(),
            );
        }
    };
    let schema = if decision {
        decision_schema()
    } else {
        protocol_schema()
    };
    let generated = serde_json::to_string_pretty(&schema)? + "\n";
    if check {
        let committed = fs::read_to_string(&path)?;
        if committed != generated {
            return Err(unified_diff(&path.to_string_lossy(), &committed, &generated)?.into());
        }
    } else {
        fs::write(path, generated)?;
    }
    Ok(())
}

fn unified_diff(path: &str, before: &str, after: &str) -> Result<String, std::fmt::Error> {
    let mut diff = String::new();
    writeln!(diff, "--- {path} (committed)")?;
    writeln!(diff, "+++ {path} (generated)")?;
    writeln!(
        diff,
        "@@ -{},{} +1,{} @@",
        usize::from(!before.is_empty()),
        before.lines().count(),
        after.lines().count()
    )?;
    for line in before.split_inclusive('\n') {
        write!(diff, "-{line}")?;
        if !line.ends_with('\n') {
            writeln!(diff, "\n\\ No newline at end of file")?;
        }
    }
    for line in after.lines() {
        writeln!(diff, "+{line}")?;
    }
    Ok(diff)
}

// A command-line exporter reports usage, I/O errors and schema diffs to its caller.
#[allow(clippy::print_stderr)]
fn main() -> ExitCode {
    match run(&std::env::args_os().skip(1).collect::<Vec<_>>()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
