//! The performance protocol's shared parts (TEST-PLAN §15.1): nearest-rank percentiles, and a result refused without
//! its environment.
use super::{REQUIRED, percentile, result_path, validate_environment, write_raw, write_result};
use serde_json::json;

#[test]
fn percentiles_are_nearest_rank() {
    let fifty: Vec<u64> = (1..=50).collect();
    assert_eq!(percentile(&fifty, 50), Ok(25));
    assert_eq!(
        percentile(&fifty, 99),
        Ok(50),
        "p99 of 50 samples is the maximum"
    );
    assert_eq!(percentile(&[7], 50), Ok(7));
    assert_eq!(percentile(&[3, 1, 2], 100), Ok(3), "unsorted input");
    assert_eq!(percentile(&[4_i64, -3, 0], 50), Ok(0), "signed differences");
    assert!(
        percentile::<u64>(&[], 50).is_err(),
        "no samples, no percentile"
    );
    assert!(percentile(&fifty, 0).is_err() && percentile(&fifty, 101).is_err());
}

#[test]
fn a_result_without_its_environment_is_refused() {
    let mut environment = serde_json::Map::new();
    for key in REQUIRED {
        environment.insert((*key).to_owned(), json!("x"));
    }
    assert!(validate_environment(&json!(environment)).is_ok());
    for key in REQUIRED {
        let mut missing = environment.clone();
        missing.remove(*key);
        let refused = validate_environment(&json!(missing));
        assert!(refused.is_err(), "{key}");
        assert!(
            refused.err().is_some_and(|error| error.contains(key)),
            "{key} is named"
        );
    }
    assert!(validate_environment(&json!("not an object")).is_err());
}

#[test]
fn results_go_under_tests_perf_results_by_date() {
    let path = result_path(
        std::path::Path::new("/repo"),
        "PERF-002",
        "2026-10-01T14:05:09Z",
    );
    assert_eq!(
        path,
        std::path::Path::new(
            "/repo/tests/perf/results/2026-10-01/PERF-002-2026-10-01T14-05-09Z.json"
        )
    );
}

#[test]
fn a_result_carries_its_verdict_and_the_reasons_it_failed() -> Result<(), Box<dyn std::error::Error>>
{
    let root = tempfile::tempdir()?;
    let mut environment = serde_json::Map::new();
    for key in REQUIRED {
        environment.insert((*key).to_owned(), json!("x"));
    }
    let mut written = Vec::new();
    for (second, failures) in [(1, vec![]), (2, vec!["p99 9 ms exceeds 5 ms".to_owned()])] {
        environment.insert("utc".into(), json!(format!("2026-10-01T12:00:0{second}Z")));
        let path = write_result(
            root.path(),
            "PERF-002",
            &json!(environment),
            &json!({"p99": 9}),
            &failures,
        )?;
        written.push(serde_json::from_slice::<serde_json::Value>(
            &std::fs::read(path)?,
        )?);
    }
    assert_eq!(written[0]["verdict"], "pass");
    assert_eq!(written[0]["failures"], json!([]));
    assert_eq!(written[1]["verdict"], "fail");
    assert_eq!(written[1]["failures"], json!(["p99 9 ms exceeds 5 ms"]));
    assert_eq!(
        (
            written[1]["id"].as_str(),
            written[1]["result"]["p99"].as_u64()
        ),
        (Some("PERF-002"), Some(9))
    );
    Ok(())
}

#[test]
fn raw_data_goes_beside_its_result_named_with_its_hash() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let utc = "2026-10-02T06:00:00Z";
    let entry = write_raw(
        root.path(),
        "PERF-002",
        utc,
        "lag",
        &json!({"lags_ns": [3, 1, 2]}),
    )?;
    let path = result_path(root.path(), "PERF-002", utc)
        .with_file_name("PERF-002-2026-10-02T06-00-00Z.lag.json");
    let bytes = std::fs::read(&path)?;
    assert_eq!(bytes, br#"{"lags_ns":[3,1,2]}"#);
    assert_eq!(entry["file"], "PERF-002-2026-10-02T06-00-00Z.lag.json");
    assert_eq!(entry["bytes"], bytes.len());
    // sha256 of the bytes above, from `sha256sum`.
    assert_eq!(
        entry["sha256"],
        "f1a97d947bb27c9746e7e45d34b098419ef3f6e3b57c81ef5c8e51c52e1353cc"
    );
    assert!(
        write_raw(root.path(), "PERF-002", utc, "lag", &json!([])).is_err(),
        "never over an existing file"
    );
    for name in ["", "Lag", "../lag", "lag.json"] {
        assert!(
            write_raw(root.path(), "PERF-002", utc, name, &json!([])).is_err(),
            "{name:?}"
        );
    }
    Ok(())
}
