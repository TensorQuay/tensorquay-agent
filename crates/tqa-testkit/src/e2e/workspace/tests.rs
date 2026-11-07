use super::*;
use axum::{Router, extract::Query, http::Uri, response::IntoResponse};
use serde_json::json;
use std::{collections::HashMap, time::Duration};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// A files API over a fixed tree; `names` overrides the root listing to inject a hostile name.
async fn files_api(root_names: Option<Vec<&'static str>>) -> Result<String, std::io::Error> {
    let app = Router::new().fallback(
        move |uri: Uri, Query(query): Query<HashMap<String, String>>| {
            let root_names = root_names.clone();
            async move {
                let path = query.get("path").cloned().unwrap_or_default();
                if uri.path().ends_with("/download") {
                    return format!("content of {path}").into_response();
                }
                let items = match (path.as_str(), &root_names) {
                    ("", Some(names)) => names
                        .iter()
                        .map(|name| json!({"name": name, "kind": "file", "size": 1}))
                        .collect(),
                    ("", None) => vec![
                        json!({"name": "a.txt", "kind": "file", "size": 1}),
                        json!({"name": "src", "kind": "dir", "size": 0}),
                    ],
                    ("src", _) => vec![json!({"name": "b.py", "kind": "file", "size": 1})],
                    _ => vec![],
                };
                axum::Json(json!({"path": path, "items": items, "next_after": null}))
                    .into_response()
            }
        },
    );
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    tokio::spawn(async move { axum::serve(listener, app).await });
    Ok(format!("http://{address}"))
}

async fn copy(
    root_names: Option<Vec<&'static str>>,
    into: &Path,
) -> Result<Result<usize, CopyError>, Box<dyn std::error::Error>> {
    let base = files_api(root_names).await?;
    let token = tempfile::NamedTempFile::new()?;
    std::fs::write(token.path(), "token")?;
    let client = ApiClient::new(&base, token.path(), Duration::from_secs(5))?;
    let id: ConversationId = serde_json::from_value(json!("0192a7d4-0000-7000-8000-000000000001"))?;
    Ok(download(&client, &id, into).await)
}

#[tokio::test]
async fn tk_u_007_the_copy_mirrors_the_workspace_and_refuses_hostile_names() -> TestResult {
    let into = tempfile::tempdir()?;
    assert!(matches!(copy(None, into.path()).await?, Ok(2)));
    assert_eq!(
        std::fs::read_to_string(into.path().join("src/b.py"))?,
        "content of src/b.py"
    );
    for hostile in ["..", "a/b", ""] {
        let into = tempfile::tempdir()?;
        assert!(
            matches!(
                copy(Some(vec![hostile]), into.path()).await?,
                Err(CopyError::Unsafe)
            ),
            "{hostile:?}"
        );
    }
    Ok(())
}

#[test]
fn tk_u_007_protected_paths_are_compared_and_restored_from_the_seed() -> TestResult {
    let (seed, copy) = (tempfile::tempdir()?, tempfile::tempdir()?);
    std::fs::create_dir_all(seed.path().join("tests"))?;
    std::fs::write(seed.path().join("tests/test_a.py"), "assert f() == 1\n")?;
    let protected = vec!["tests/test_a.py".to_owned(), "tests/absent.py".to_owned()];
    assert!(
        tampered(seed.path(), copy.path(), &protected)?,
        "a deleted test is a change"
    );
    restore(seed.path(), copy.path(), &protected)?;
    assert!(!tampered(seed.path(), copy.path(), &protected)?);
    std::fs::write(copy.path().join("tests/test_a.py"), "assert True\n")?;
    std::fs::write(copy.path().join("tests/absent.py"), "")?;
    assert!(tampered(seed.path(), copy.path(), &protected)?);
    restore(seed.path(), copy.path(), &protected)?;
    assert_eq!(
        std::fs::read_to_string(copy.path().join("tests/test_a.py"))?,
        "assert f() == 1\n"
    );
    assert!(!copy.path().join("tests/absent.py").exists());
    Ok(())
}

#[test]
fn tk_u_007_sharing_the_copy_adds_write_access_and_keeps_execute_bits() -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    let copy = tempfile::tempdir()?;
    let script = copy.path().join("build.sh");
    std::fs::write(&script, "#!/bin/sh\n")?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))?;
    share(copy.path())?;
    assert_eq!(
        std::fs::metadata(&script)?.permissions().mode() & 0o777,
        0o766
    );
    assert_eq!(
        std::fs::metadata(copy.path())?.permissions().mode() & 0o777,
        0o777
    );
    Ok(())
}
