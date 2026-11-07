use super::*;
use axum::{Router, http::StatusCode, http::Uri};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Counts a call to `name` and returns how many there have been.
fn bump(calls: &Mutex<HashMap<String, u32>>, name: &str) -> u32 {
    let mut calls = calls.lock().unwrap_or_else(PoisonError::into_inner);
    let seen = calls.entry(name.to_owned()).or_default();
    *seen += 1;
    *seen
}

/// `slow` is busy twice (its sandbox still idling out), `stuck` always, and `gone` was already deleted.
fn answer(name: &str, seen: u32) -> StatusCode {
    match (name, seen) {
        ("slow", 1..=2) | ("stuck", _) => StatusCode::CONFLICT,
        ("gone", _) => StatusCode::NOT_FOUND,
        _ => StatusCode::NO_CONTENT,
    }
}

// TEST: TK-U-027
#[tokio::test]
async fn tk_u_027_the_sweep_retries_a_busy_workspace_until_its_bound() -> TestResult {
    let calls: Arc<Mutex<HashMap<String, u32>>> = Arc::default();
    let count = calls.clone();
    let app = Router::new().fallback(move |uri: Uri| {
        let count = count.clone();
        async move {
            let name = uri.path().rsplit('/').next().unwrap_or_default().to_owned();
            let seen = bump(&count, &name);
            answer(&name, seen)
        }
    });
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    tokio::spawn(async move { axum::serve(listener, app).await });
    let file = tempfile::NamedTempFile::new()?;
    std::fs::write(file.path(), "tqa_pat_example-not-a-secret")?;
    let client = ApiClient::new(
        &format!("http://{address}"),
        file.path(),
        Duration::from_secs(5),
    )?;
    let workspaces = ["done", "slow", "stuck", "gone"].map(str::to_owned);
    let left = sweep(
        &client,
        &workspaces,
        Duration::from_millis(300),
        Duration::from_millis(20),
    )
    .await;
    assert_eq!(left, ["stuck"]);
    let calls = calls.lock().unwrap_or_else(PoisonError::into_inner).clone();
    assert_eq!((calls["done"], calls["slow"], calls["gone"]), (1, 3, 1));
    assert!(
        calls["stuck"] > 3,
        "retried until the bound: {}",
        calls["stuck"]
    );
    // Nothing to do returns at once.
    assert!(
        sweep(&client, &[], Duration::from_mins(1), Duration::from_secs(1))
            .await
            .is_empty()
    );
    Ok(())
}
