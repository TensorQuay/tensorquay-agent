//! The files API as the files suites call it (`files.rs`, `files_previews.rs`): a signed-in request, its JSON body or
//! refusal code, a new task, and uploads into it.
#![allow(dead_code)]
use crate::support::Fixture;
use reqwest::{Method, Response};
use serde_json::{Value, json};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn call(f: &Fixture, cookie: &str, method: Method, path: &str) -> reqwest::RequestBuilder {
    f.client
        .request(method, f.url(path))
        .header("origin", f.origin())
        .header("cookie", cookie)
}

pub async fn body(response: Response) -> Result<(u16, Value)> {
    let status = response.status().as_u16();
    let bytes = response.bytes().await?;
    Ok((
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    ))
}

pub async fn code(response: Response) -> Result<(u16, String)> {
    let (status, value) = body(response).await?;
    Ok((
        status,
        value["error"]["code"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
    ))
}

pub async fn task(f: &Fixture, cookie: &str) -> Result<String> {
    let (status, created) = body(
        call(f, cookie, Method::POST, "/api/v1/conversations")
            .header("idempotency-key", uuid::Uuid::now_v7().to_string())
            .json(&json!({"title":"Files","model_route":"local"}))
            .send()
            .await?,
    )
    .await?;
    assert_eq!(status, 201, "{created}");
    Ok(created["id"].as_str().ok_or("id")?.to_owned())
}

pub async fn upload(
    f: &Fixture,
    cookie: &str,
    task: &str,
    name: &str,
    bytes: Vec<u8>,
) -> Result<Response> {
    Ok(call(
        f,
        cookie,
        Method::POST,
        &format!("/api/v1/conversations/{task}/uploads"),
    )
    .query(&[("name", name)])
    .body(bytes)
    .send()
    .await?)
}

/// A new task with `files` uploaded under `uploads/`; returns its id.
pub async fn task_with(f: &Fixture, cookie: &str, files: &[(&str, Vec<u8>)]) -> Result<String> {
    let id = task(f, cookie).await?;
    for (name, bytes) in files {
        assert_eq!(
            upload(f, cookie, &id, name, bytes.clone()).await?.status(),
            201
        );
    }
    Ok(id)
}
