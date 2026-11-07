use super::*;
use axum::{Router, body::Bytes, http::HeaderMap, http::Uri, response::IntoResponse};
use std::sync::{Arc, Mutex, PoisonError};

type TestResult = Result<(), Box<dyn std::error::Error>>;
type Seen = Arc<Mutex<Vec<(String, Option<String>, Option<String>, Value)>>>;

const TOKEN: &str = "tqa_pat_example-not-a-secret";

/// A gateway stand-in that records path, bearer, idempotency key and body, and answers by path.
async fn gateway() -> Result<(String, Seen), Box<dyn std::error::Error>> {
    let seen = Seen::default();
    let record = seen.clone();
    let app = Router::new().fallback(move |uri: Uri, headers: HeaderMap, body: Bytes| {
        let record = record.clone();
        async move {
            let header = |name: &str| {
                headers
                    .get(name)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned)
            };
            let body = serde_json::from_slice(&body).unwrap_or(Value::Null);
            let path = uri.path().to_owned();
            record.lock().unwrap_or_else(PoisonError::into_inner).push((
                path.clone(),
                header("authorization"),
                header("idempotency-key"),
                body,
            ));
            match path.as_str() {
                "/api/v1/conversations" => {
                    axum::Json(json!({"id": "0192a7d4-0000-7000-8000-000000000001"}))
                        .into_response()
                }
                "/api/v1/missing" => axum::http::StatusCode::NOT_FOUND.into_response(),
                _ => axum::http::StatusCode::ACCEPTED.into_response(),
            }
        }
    });
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    tokio::spawn(async move { axum::serve(listener, app).await });
    Ok((format!("http://{address}"), seen))
}

fn token_file(contents: &str) -> Result<tempfile::NamedTempFile, std::io::Error> {
    let file = tempfile::NamedTempFile::new()?;
    std::fs::write(file.path(), contents)?;
    Ok(file)
}

#[tokio::test]
async fn tk_u_006_client_posts_with_its_token_and_distinct_keys() -> TestResult {
    let (base, seen) = gateway().await?;
    let file = token_file(&format!("{TOKEN}\n"))?;
    let client = ApiClient::new(&base, file.path(), Duration::from_secs(5))?;
    let request = NewConversation {
        title: "t",
        model_route: "fake",
        template: None,
        profile: None,
        workspace_id: None,
    };
    let id = client.create_conversation(&request).await?;
    let run: RunId = serde_json::from_value(json!("r-1"))?;
    client.cancel(&id, &run).await?;
    let seen = seen.lock().unwrap_or_else(PoisonError::into_inner).clone();
    assert_eq!(seen.len(), 2);
    assert!(
        seen.iter()
            .all(|(_, bearer, _, _)| bearer.as_deref() == Some(&format!("Bearer {TOKEN}")[..]))
    );
    assert_ne!(seen[0].2, seen[1].2);
    assert!(seen.iter().all(|(_, _, key, _)| key.is_some()));
    assert_eq!(
        seen[1].0,
        format!("/api/v1/conversations/{id}/runs/r-1/cancel")
    );
    Ok(())
}

#[tokio::test]
async fn tk_u_005_token_never_appears_in_debug_or_errors() -> TestResult {
    let (base, _) = gateway().await?;
    let file = token_file(TOKEN)?;
    let client = ApiClient::new(&base, file.path(), Duration::from_secs(5))?;
    let error = client
        .get::<Value>("/missing", &[])
        .await
        .err()
        .ok_or("expected an error")?;
    assert_eq!(error, ClientError::Status(404));
    for text in [
        format!("{client:?}"),
        format!("{error:?}"),
        error.to_string(),
    ] {
        assert!(!text.contains(TOKEN), "{text}");
    }
    for bad in ["", "\n", "two words", "tab\there"] {
        let file = token_file(bad)?;
        let refused = ApiClient::new(&base, file.path(), Duration::from_secs(5));
        assert!(matches!(refused, Err(ClientError::TokenFile)), "{bad:?}");
    }
    Ok(())
}

/// Each request's method, URI and bearer.
type Deletions = Arc<Mutex<Vec<(String, String, Option<String>)>>>;

/// A deletion's answer: `busy` is still in use.
fn deletion(path: &str) -> axum::http::StatusCode {
    if path.ends_with("/busy") {
        axum::http::StatusCode::CONFLICT
    } else {
        axum::http::StatusCode::NO_CONTENT
    }
}

// TEST: TK-U-026
#[tokio::test]
async fn tk_u_026_a_task_and_a_workspace_are_deleted_and_a_refusal_is_an_error() -> TestResult {
    let seen: Deletions = Arc::default();
    let record = seen.clone();
    let app = Router::new().fallback(
        move |method: axum::http::Method, uri: Uri, headers: HeaderMap| {
            let record = record.clone();
            async move {
                let bearer = headers
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned);
                record.lock().unwrap_or_else(PoisonError::into_inner).push((
                    method.to_string(),
                    uri.to_string(),
                    bearer,
                ));
                deletion(uri.path())
            }
        },
    );
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    tokio::spawn(async move { axum::serve(listener, app).await });
    let file = token_file(TOKEN)?;
    let client = ApiClient::new(
        &format!("http://{address}"),
        file.path(),
        Duration::from_secs(5),
    )?;
    client
        .delete_workspace("0192a7d4-0000-7000-8000-00000000000a")
        .await?;
    client
        .delete_conversation(&"0192a7d4-0000-7000-8000-00000000000b".parse()?)
        .await?;
    assert_eq!(
        client.delete_workspace("busy").await,
        Err(ClientError::Status(409))
    );
    let seen = seen.lock().unwrap_or_else(PoisonError::into_inner).clone();
    assert_eq!(
        seen.first(),
        Some(&(
            "DELETE".to_owned(),
            "/api/v1/workspaces/0192a7d4-0000-7000-8000-00000000000a?with_conversations=true"
                .to_owned(),
            Some(format!("Bearer {TOKEN}"))
        ))
    );
    assert_eq!(
        seen.get(1)
            .map(|(method, uri, _)| (method.as_str(), uri.as_str())),
        Some((
            "DELETE",
            "/api/v1/conversations/0192a7d4-0000-7000-8000-00000000000b"
        ))
    );
    assert_eq!(seen.len(), 3);
    assert_eq!(
        seen.get(1)
            .map(|(method, uri, _)| (method.as_str(), uri.as_str())),
        Some((
            "DELETE",
            "/api/v1/conversations/0192a7d4-0000-7000-8000-00000000000b"
        ))
    );
    assert_eq!(seen.len(), 3);
    Ok(())
}
