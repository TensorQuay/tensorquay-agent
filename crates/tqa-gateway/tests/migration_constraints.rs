//! GW-U-225: a migration that redefines a `CHECK (column IN (...))` constraint keeps every value an earlier migration
//! allowed. Branches extend these lists side by side; a migration that copied an older list dropped `agent_teams` from
//! `admin_audit_area_check`, so every agent-team settings write failed its audit row and answered 503 (19 Nov 2026).
//! A migration that narrows a list on purpose says so in a comment: `-- narrows <constraint name>`.
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;

/// Narrowings made on purpose before this test, in migrations that can't change (an applied migration's checksum is
/// checked): 0061 removed approvals and their parking setting; 0063 moved the sandbox sizes into each environment.
const NARROWED: &[(&str, &str)] = &[
    ("0061_approvals_removed.sql", "settings_key_check"),
    ("0063_environment_sizes.sql", "settings_key_check"),
];

/// Each `CONSTRAINT name CHECK (column IN ('a', 'b', ...))` in `sql`, with its values; any other shape is skipped.
fn lists(sql: &str) -> Vec<(String, BTreeSet<String>)> {
    let mut found = Vec::new();
    let mut rest = sql;
    while let Some(at) = rest.find("CONSTRAINT ") {
        rest = &rest[at + "CONSTRAINT ".len()..];
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let Some(check) = rest[name.len()..].trim_start().strip_prefix("CHECK") else {
            continue;
        };
        let Some(inner) = check.trim_start().strip_prefix('(') else {
            continue;
        };
        let column: String = inner
            .trim_start()
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let after = inner.trim_start()[column.len()..].trim_start();
        let Some(list) = after
            .strip_prefix("IN")
            .map(str::trim_start)
            .and_then(|l| l.strip_prefix('('))
        else {
            continue;
        };
        let Some(close) = list.find(')') else {
            continue;
        };
        let values = list[..close]
            .split(',')
            .map(|value| value.trim().trim_matches('\'').to_owned())
            .filter(|value| !value.is_empty())
            .collect();
        if !column.is_empty() {
            found.push((name, values));
        }
    }
    found
}

/// What `file` drops from `name`'s earlier values, unless it says it narrows the list or is a known narrowing.
fn loss(
    sql: &str,
    file: &str,
    name: &str,
    earlier: &(BTreeSet<String>, String),
    values: &BTreeSet<String>,
) -> Option<String> {
    let (before, from) = earlier;
    let lost: Vec<_> = before.difference(values).cloned().collect();
    let allowed = sql.contains(&format!("-- narrows {name}")) || NARROWED.contains(&(file, name));
    (!lost.is_empty() && !allowed)
        .then(|| format!("{file} drops {lost:?} from {name} (allowed since {from})"))
}

// TEST: GW-U-225
#[test]
fn gw_u_225_a_redefined_check_list_keeps_every_earlier_value() -> Result<(), Box<dyn Error>> {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let mut files: Vec<_> = std::fs::read_dir(&directory)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "sql"))
        .collect();
    files.sort();
    let mut seen: BTreeMap<String, (BTreeSet<String>, String)> = BTreeMap::new();
    let mut dropped = Vec::new();
    let mut redefinitions = 0;
    for path in &files {
        let sql = std::fs::read_to_string(path)?;
        let file = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        for (name, values) in lists(&sql) {
            if let Some(earlier) = seen.get(&name) {
                redefinitions += 1;
                dropped.extend(loss(&sql, &file, &name, earlier, &values));
            }
            seen.insert(name, (values, file.clone()));
        }
    }
    assert!(
        redefinitions > 5,
        "the parser reads the migrations' lists ({redefinitions} redefinitions)"
    );
    assert!(dropped.is_empty(), "{}", dropped.join("\n"));
    Ok(())
}

// TEST: GW-U-225
#[test]
fn gw_u_225_the_parser_reads_the_shapes_the_migrations_use() {
    let sql = "ALTER TABLE t DROP CONSTRAINT t_area_check,\n    ADD CONSTRAINT t_area_check\n        CHECK (area IN \
               ('a', 'b',\n                        'c'));\nALTER TABLE u ADD CONSTRAINT u_kind_check CHECK (kind IN ('x'));\n\
               ALTER TABLE v ADD CONSTRAINT v_len_check CHECK (char_length(name) <= 10);";
    let found = lists(sql);
    assert_eq!(found.len(), 2, "{found:?}");
    assert_eq!(found[0].0, "t_area_check");
    assert_eq!(
        found[0].1,
        ["a", "b", "c"].iter().map(|v| (*v).to_owned()).collect()
    );
    assert_eq!(found[1].0, "u_kind_check");
}
