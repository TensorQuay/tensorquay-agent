//! A fake runtime's side of the workspace service (ADR-0030): the gateway's requests, and reply parts from JSON.
use serde_json::{Value, json};
use tqa_types::{ClientFrame, ServerFrame, WorkspaceOp};

use super::{Fixture, runtime::Runtime};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// The next workspace request on the link, skipping any other action.
pub async fn request(runtime: &mut Runtime) -> Result<(u64, WorkspaceOp)> {
    for _ in 0..16 {
        if let ClientFrame::WorkspaceRequest { request_id, op } = runtime.action().await? {
            return Ok((request_id, op));
        }
    }
    Err("no workspace request arrived".into())
}

/// The next workspace cancel on the link, skipping any other action.
pub async fn cancel(runtime: &mut Runtime) -> Result<u64> {
    for _ in 0..16 {
        if let ClientFrame::WorkspaceCancel { request_id } = runtime.action().await? {
            return Ok(request_id);
        }
    }
    Err("no workspace cancel arrived".into())
}

/// Sends `results` as the parts of request `id`, the last one ending it.
pub async fn reply(runtime: &mut Runtime, id: u64, results: &[Value]) -> Result<()> {
    let count = results.len();
    for (part, result) in results.iter().enumerate() {
        let frame = json!({
            "type": "workspace.reply", "request_id": id, "part": part, "last": part + 1 == count, "result": result,
        });
        runtime
            .send(&ServerFrame::parse(frame.to_string().as_bytes())?)
            .await?;
    }
    Ok(())
}

/// A file chunk of `data` (already base64) for a file of `size` bytes.
pub fn content(data: &str, size: u64, eof: bool) -> Value {
    json!({"kind": "content", "size": size, "modified": 1_700_000_000, "inode": 42, "data": data, "eof": eof})
}

/// One directory entry.
pub fn item(name: &str, kind: &str, size: u64) -> Value {
    json!({"name": name, "kind": kind, "size": size, "modified": 1_700_000_000})
}

/// A response's status, headers and body.
pub struct Answer {
    pub status: u16,
    pub headers: reqwest::header::HeaderMap,
    pub body: Vec<u8>,
}

impl Answer {
    pub fn json(&self) -> Result<Value> {
        Ok(serde_json::from_slice(&self.body)?)
    }
}

/// Starts a GET the fake runtime answers meanwhile.
pub fn get(
    f: &Fixture,
    path: &str,
    cookie: &str,
) -> tokio::task::JoinHandle<std::result::Result<Answer, reqwest::Error>> {
    let request = f
        .client
        .get(f.url(path))
        .header("origin", f.origin())
        .header("cookie", cookie);
    tokio::spawn(async move {
        let response = request.send().await?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = response.bytes().await?.to_vec();
        Ok(Answer {
            status,
            headers,
            body,
        })
    })
}

/// A file's SHA-256, as manifests and blob names spell it.
pub fn sha(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    tqa_tools::workspace_tree::manifest::hex(&sha2::Sha256::digest(bytes))
}

/// A stamp that changes with the content, as a real one changes with each write.
fn stamp(bytes: &[u8]) -> Value {
    let digest = sha(bytes);
    let inode = u64::from_str_radix(&digest[..12], 16).unwrap_or_default();
    json!([bytes.len(), 0, 0, 0, inode])
}

/// Answers the next workspace request from `files` as a runtime would (a manifest, or blobs by hash), and returns it.
pub async fn serve(
    runtime: &mut Runtime,
    files: &std::collections::BTreeMap<String, Vec<u8>>,
) -> Result<WorkspaceOp> {
    let (asked, op) = request(runtime).await?;
    answer(runtime, asked, &op, files).await?;
    Ok(op)
}

/// Answers request `asked` for `op` from `files`: a manifest, or the blobs it can produce.
pub async fn answer(
    runtime: &mut Runtime,
    asked: u64,
    op: &WorkspaceOp,
    files: &std::collections::BTreeMap<String, Vec<u8>>,
) -> Result<()> {
    match op {
        WorkspaceOp::Manifest { max_text_bytes, .. } => {
            let entries: Vec<Value> = files
                .iter()
                .map(|(path, bytes)| {
                    let mut entry = json!({"path": path, "size": bytes.len(), "stamp": stamp(bytes),
                        "text": std::str::from_utf8(bytes).is_ok() && !bytes.contains(&0)});
                    if bytes.len() as u64 <= u64::from(*max_text_bytes) {
                        entry["hash"] = json!(sha(bytes));
                    }
                    entry
                })
                .collect();
            let part = json!({"kind": "manifest_part", "entries": entries, "truncated": false});
            reply(runtime, asked, &[part]).await?;
        }
        WorkspaceOp::Blobs { hashes } => {
            use base64::Engine as _;
            let parts: Vec<Value> = hashes
                .iter()
                .filter_map(|hash| files.values().find(|bytes| sha(bytes) == *hash))
                .map(|bytes| {
                    json!({"kind": "blob_part", "hash": sha(bytes), "offset": 0,
                        "data": base64::engine::general_purpose::STANDARD.encode(bytes), "complete": true})
                })
                .collect();
            if parts.is_empty() {
                let none = json!({"kind": "error", "code": "not_found", "message": "none", "retryable": false});
                reply(runtime, asked, &[none]).await?;
            } else {
                reply(runtime, asked, &parts).await?;
            }
        }
        other => return Err(format!("not a run-changes request: {other:?}").into()),
    }
    Ok(())
}

/// The next action that is not a workspace request or cancel.
pub async fn next_action(runtime: &mut Runtime) -> Result<ClientFrame> {
    for _ in 0..16 {
        match runtime.action().await? {
            ClientFrame::WorkspaceRequest { .. } | ClientFrame::WorkspaceCancel { .. } => {}
            other => return Ok(other),
        }
    }
    Err("no other action arrived".into())
}
